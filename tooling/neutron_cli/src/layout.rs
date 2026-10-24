use crate::config::Config;
use crate::upstream::Upstream;
use anyhow::{Result, bail};
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Destination {
    /// Verbatim copy under `core/`; never rewritten.
    Core,
    /// Copied to the Neutron root (assets, toolchain files, upstream tooling).
    Root,
    /// Mirrored into the IDE canary; relative paths are fixed up.
    Ide,
    /// A Neutron-owned crate in `riso/` that keeps the upstream name; upstream changes are
    /// 3-way merged into it instead of being copied.
    Shim,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mapped {
    pub path: String,
    pub destination: Destination,
}

struct Rule {
    from: String,
    to: String,
    destination: Destination,
}

/// Maps upstream paths to Neutron paths, following `upstream.toml`.
pub struct Layout {
    rules: Vec<Rule>,
    exclude: Vec<String>,
    unmanaged: Vec<String>,
    ide_root: String,
}

impl Layout {
    pub fn new(config: &Config, upstream: &Upstream) -> Result<Self> {
        let mut rules = Vec::new();
        let mut core_destinations: HashMap<String, String> = HashMap::new();
        let mut core_packages = Vec::new();
        for name in &config.core.crates {
            let Some(package) = upstream.package_named(name) else {
                bail!("core crate `{name}` in upstream.toml is not an upstream workspace package");
            };
            core_packages.push(package);
        }
        for package in &core_packages {
            // A package nested in another package's directory moves with its parent.
            let Some(parent) = upstream
                .package_containing(parent_path(&package.dir))
                .filter(|parent| parent.dir != package.dir)
            else {
                continue;
            };
            if !core_packages.iter().any(|core| core.dir == parent.dir) {
                bail!(
                    "core crate `{}` lives inside `{}`, which is not in core; move them together",
                    package.name,
                    parent.name
                );
            }
        }
        for package in core_packages {
            let nested = upstream
                .package_containing(parent_path(&package.dir))
                .is_some_and(|parent| parent.dir != package.dir);
            if nested {
                continue;
            }
            let name = &package.name;
            // Keep the directory name rather than the package name: sibling references such as
            // `../gpui` use directory names.
            let directory_name = package.dir.rsplit('/').next().unwrap_or(&package.dir);
            let destination = format!("core/{directory_name}");
            if let Some(other) = core_destinations.insert(destination.clone(), name.clone()) {
                bail!("core crates `{other}` and `{name}` would both be copied to {destination}");
            }
            rules.push(Rule {
                from: package.dir.clone(),
                to: destination,
                destination: Destination::Core,
            });
        }
        for (name, shim) in &config.shim {
            let directory_name = shim.origin.rsplit('/').next().unwrap_or(&shim.origin);
            if directory_name != name {
                bail!(
                    "shim `{name}` must keep the directory name of its origin `{}`",
                    shim.origin
                );
            }
            rules.push(Rule {
                from: shim.origin.clone(),
                to: format!("riso/{directory_name}"),
                destination: Destination::Shim,
            });
        }
        for (from, to) in &config.layout.root {
            rules.push(Rule {
                from: from.clone(),
                to: to.clone(),
                destination: Destination::Root,
            });
        }
        rules.sort_by_key(|rule| std::cmp::Reverse(rule.from.len()));

        let ide_root = config.layout.ide.trim_end_matches('/').to_string();
        for managed in rules
            .iter()
            .filter(|rule| rule.destination != Destination::Shim)
            .map(|rule| rule.to.as_str())
            .chain([ide_root.as_str()])
        {
            let first = managed.split('/').next().unwrap_or_default();
            if managed.is_empty()
                || first == ".git"
                || config
                    .layout
                    .unmanaged
                    .iter()
                    .any(|path| strip_path_prefix(managed, path).is_some())
            {
                bail!(
                    "`{managed}` cannot be a sync destination: the sync deletes files it does not own there"
                );
            }
        }

        Ok(Self {
            rules,
            exclude: config.layout.exclude.clone(),
            unmanaged: config.layout.unmanaged.clone(),
            ide_root,
        })
    }

    /// Where an upstream path (file or directory) lives in Neutron, or `None` if it is excluded.
    pub fn map(&self, path: &str) -> Option<Mapped> {
        if self
            .exclude
            .iter()
            .any(|excluded| strip_path_prefix(path, excluded).is_some())
        {
            return None;
        }
        if self.is_unmanaged(path) {
            return Some(Mapped {
                path: path.to_string(),
                destination: Destination::Root,
            });
        }
        for rule in &self.rules {
            if let Some(rest) = strip_path_prefix(path, &rule.from) {
                return Some(Mapped {
                    path: join_path(&rule.to, rest),
                    destination: rule.destination,
                });
            }
        }
        Some(Mapped {
            path: join_path(&self.ide_root, path),
            destination: Destination::Ide,
        })
    }

    pub fn is_unmanaged(&self, path: &str) -> bool {
        self.unmanaged
            .iter()
            .any(|unmanaged| strip_path_prefix(path, unmanaged).is_some())
    }

    /// Neutron directories whose whole content is owned by the sync.
    pub fn managed_roots(&self) -> Vec<String> {
        let mut roots: Vec<String> = self
            .rules
            .iter()
            .filter(|rule| rule.destination != Destination::Shim)
            .map(|rule| rule.to.clone())
            .collect();
        roots.push(self.ide_root.clone());
        roots
    }

    /// The destination kind of the managed directory that contains the Neutron path `path`.
    pub fn map_neutron_destination(&self, path: &str) -> Option<Destination> {
        self.rules
            .iter()
            .filter(|rule| strip_path_prefix(path, &rule.to).is_some())
            .max_by_key(|rule| rule.to.len())
            .map(|rule| rule.destination)
            .or_else(|| strip_path_prefix(path, &self.ide_root).map(|_| Destination::Ide))
    }

    pub fn ide_root(&self) -> &str {
        &self.ide_root
    }

    /// `(upstream directory, Neutron directory)` of every core crate.
    pub fn core_crates(&self) -> impl Iterator<Item = (&str, &str)> {
        self.rules
            .iter()
            .filter(|rule| rule.destination == Destination::Core)
            .map(|rule| (rule.from.as_str(), rule.to.as_str()))
    }
}

/// `Some(rest)` if `path` is `prefix` or lies inside it, comparing whole components.
pub fn strip_path_prefix<'a>(path: &'a str, prefix: &str) -> Option<&'a str> {
    if prefix.is_empty() {
        return Some(path);
    }
    let rest = path.strip_prefix(prefix)?;
    if rest.is_empty() {
        Some("")
    } else {
        rest.strip_prefix('/')
    }
}

pub fn join_path(base: &str, rest: &str) -> String {
    match (base.is_empty(), rest.is_empty()) {
        (true, _) => rest.to_string(),
        (false, true) => base.to_string(),
        (false, false) => format!("{base}/{rest}"),
    }
}

pub fn parent_path(path: &str) -> &str {
    path.rsplit_once('/').map_or("", |(parent, _)| parent)
}

/// Lexically resolves `relative` against the directory `base`. Returns `None` if the result
/// would leave the repository root.
pub fn resolve_path(base: &str, relative: &str) -> Option<String> {
    let mut components: Vec<&str> = base.split('/').filter(|part| !part.is_empty()).collect();
    for part in relative.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                components.pop()?;
            }
            part => components.push(part),
        }
    }
    Some(components.join("/"))
}

/// Relative path from the directory `from` to `to`, both relative to the repository root.
pub fn relative_path(from: &str, to: &str) -> String {
    let from_parts: Vec<&str> = from.split('/').filter(|part| !part.is_empty()).collect();
    let to_parts: Vec<&str> = to.split('/').filter(|part| !part.is_empty()).collect();
    let common = from_parts
        .iter()
        .zip(&to_parts)
        .take_while(|(left, right)| left == right)
        .count();
    let mut parts: Vec<&str> = vec![".."; from_parts.len() - common];
    parts.extend(&to_parts[common..]);
    if parts.is_empty() {
        ".".to_string()
    } else {
        parts.join("/")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefixes_match_whole_components() {
        assert_eq!(
            strip_path_prefix("assets/icons/a.svg", "assets"),
            Some("icons/a.svg")
        );
        assert_eq!(strip_path_prefix("assets", "assets"), Some(""));
        assert_eq!(strip_path_prefix("assets_extra/a", "assets"), None);
    }

    #[test]
    fn resolves_and_relativizes() {
        assert_eq!(
            resolve_path("crates/gpui/src", "../../../assets/fonts"),
            Some("assets/fonts".to_string())
        );
        assert_eq!(resolve_path("crates", "../../x"), None);
        assert_eq!(
            relative_path("apps/ide/crates/gpui/src", "assets/fonts"),
            "../../../../../assets/fonts"
        );
        assert_eq!(relative_path("core/gpui", "core/gpui_util"), "../gpui_util");
    }
}
