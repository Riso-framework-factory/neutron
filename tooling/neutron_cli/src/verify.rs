use crate::config::Config;
use crate::layout::{Destination, Layout, strip_path_prefix};
use crate::rewrite::PathFixer;
use crate::sync::{self, ChangeKind};
use crate::upstream::Upstream;
use anyhow::{Context as _, Result, bail};
use serde::Deserialize;
use std::collections::{HashMap, HashSet};
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use walkdir::WalkDir;

pub struct Finding {
    pub code: &'static str,
    pub message: String,
}

impl fmt::Display for Finding {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}: {}", self.code, self.message)
    }
}

/// Where a package lives in Neutron; decides which dependencies it may have.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, clap::ValueEnum)]
pub enum Group {
    Core,
    /// A crate in `riso/` that keeps an upstream crate name and replaces it everywhere.
    Shim,
    Riso,
    Crate,
    Ide,
    Apps,
    Tooling,
    Other,
}

pub struct NeutronPackage {
    pub name: String,
    pub group: Group,
    dependencies: Vec<NeutronDependency>,
}

struct NeutronDependency {
    name: String,
    kind: Option<String>,
    path: Option<PathBuf>,
}

pub fn run(
    root: &Path,
    config: &Config,
    upstream: &Upstream,
    layout: &Layout,
) -> Result<Vec<Finding>> {
    let mut findings = Vec::new();

    // V0/V1: a dry-run sync must not change anything.
    let outcome = sync::run(root, config, upstream, layout, true)?;
    for change in &outcome.changes {
        let action = match change.kind {
            ChangeKind::Write => "differs from",
            ChangeKind::Delete => "is not in",
        };
        if change.destination == Some(Destination::Core) {
            findings.push(Finding {
                code: "V1",
                message: format!(
                    "{} {action} upstream zed@{}",
                    change.path,
                    upstream.short_commit()
                ),
            });
        } else {
            findings.push(Finding {
                code: "V0",
                message: format!(
                    "{} {action} the sync output; run `cargo neutron sync`",
                    change.path
                ),
            });
        }
    }

    check_core_paths(upstream, layout, config, &mut findings);

    let upstream_names: HashSet<&str> = upstream
        .packages
        .iter()
        .map(|package| package.name.as_str())
        .collect();
    match neutron_packages(root, &upstream_names) {
        Ok(packages) => {
            check_dependency_direction(&packages, config, &mut findings);
            check_classification(upstream, layout, &packages, &mut findings);
        }
        Err(error) => findings.push(Finding {
            code: "V2",
            message: format!("cannot read the Neutron workspace: {error:#}"),
        }),
    }

    for package in crate::lockfile::drift(root, upstream)? {
        findings.push(Finding {
            code: "V8",
            message: format!(
                "Cargo.lock does not lock upstream's {package}; run `cargo neutron sync`"
            ),
        });
    }
    if !crate::lockfile::is_complete(root)? {
        findings.push(Finding {
            code: "V8",
            message: "Cargo.lock is missing workspace members; run `cargo neutron sync`"
                .to_string(),
        });
    }
    check_conflict_markers(root, &mut findings)?;
    check_logger_initialization(root, layout, &mut findings)?;
    check_modification_headers(root, config, upstream, &mut findings)?;
    Ok(findings)
}

/// V3: core crates are copied verbatim, so none of their relative paths may need a rewrite.
fn check_core_paths(
    upstream: &Upstream,
    layout: &Layout,
    config: &Config,
    findings: &mut Vec<Finding>,
) {
    let fixer = PathFixer::new(upstream, layout, &config.rewrite.literals);
    let core_directories: Vec<&str> = layout.core_crates().map(|(from, _)| from).collect();
    for file in &upstream.files {
        if !PathFixer::applies_to(&file.path)
            || config.policy.allow_core_path_escapes.contains(&file.path)
            || !core_directories
                .iter()
                .any(|directory| strip_path_prefix(&file.path, directory).is_some())
        {
            continue;
        }
        let Ok(content) = upstream.read(&file.path) else {
            continue;
        };
        let Ok(text) = String::from_utf8(content) else {
            continue;
        };
        for rewrite in fixer.fix_source(&file.path, &text).1 {
            findings.push(Finding {
                code: "V3",
                message: format!(
                    "{} refers to \"{}\", which resolves to \"{}\" in Neutron; core cannot be rewritten, \
                     so move the target into core or move this crate back to apps/ide",
                    rewrite.file, rewrite.from, rewrite.to
                ),
            });
        }
    }
}

/// V2: `apps/<app> -> crate -> riso -> core`; nothing outside the IDE depends on the IDE.
fn check_dependency_direction(
    packages: &HashMap<PathBuf, NeutronPackage>,
    config: &Config,
    findings: &mut Vec<Finding>,
) {
    let allowed_dev: HashSet<(String, String)> = config
        .policy
        .allow_core_dev_deps
        .iter()
        .filter_map(|rule| rule.split_once("->"))
        .map(|(from, to)| (from.trim().to_string(), to.trim().to_string()))
        .collect();

    for package in packages.values() {
        for dependency in &package.dependencies {
            let Some(target) = dependency.path.as_ref().and_then(|path| packages.get(path)) else {
                continue;
            };
            let is_dev = dependency.kind.as_deref() == Some("dev");
            let allowed = match package.group {
                Group::Core => {
                    matches!(target.group, Group::Core | Group::Shim)
                        || (is_dev
                            && allowed_dev
                                .contains(&(package.name.clone(), dependency.name.clone())))
                }
                // Shims may use small riso crates such as `riso_identity`.
                Group::Shim => matches!(target.group, Group::Core | Group::Shim | Group::Riso),
                Group::Riso | Group::Crate | Group::Apps => target.group != Group::Ide,
                Group::Ide | Group::Tooling | Group::Other => true,
            };
            if !allowed {
                findings.push(Finding {
                    code: "V2",
                    message: format!(
                        "{} ({:?}) depends on {} ({:?}){}",
                        package.name,
                        package.group,
                        target.name,
                        target.group,
                        if is_dev { " as a dev-dependency" } else { "" }
                    ),
                });
            }
        }
    }
}

/// V6: every upstream package has a home in the Neutron workspace.
fn check_classification(
    upstream: &Upstream,
    layout: &Layout,
    packages: &HashMap<PathBuf, NeutronPackage>,
    findings: &mut Vec<Finding>,
) {
    let names: HashSet<&str> = packages
        .values()
        .map(|package| package.name.as_str())
        .collect();
    for package in &upstream.packages {
        if layout.map(&package.dir).is_none() {
            findings.push(Finding {
                code: "V6",
                message: format!(
                    "upstream package {} ({}) is excluded by upstream.toml",
                    package.name, package.dir
                ),
            });
        } else if !names.contains(package.name.as_str()) {
            findings.push(Finding {
                code: "V6",
                message: format!(
                    "upstream package {} is not a member of the Neutron workspace",
                    package.name
                ),
            });
        }
    }
}

pub const MODIFICATION_HEADER: &str = "Modified from Zed";

/// V7: shim source files that differ from upstream say so near the top (GPLv3 section 5(a)).
fn check_modification_headers(
    root: &Path,
    config: &Config,
    upstream: &Upstream,
    findings: &mut Vec<Finding>,
) -> Result<()> {
    for (name, shim) in &config.shim {
        if shim.base.is_empty() {
            continue;
        }
        for upstream_path in upstream.files_at(&shim.base, &shim.origin)? {
            if !upstream_path.ends_with(".rs") {
                continue;
            }
            let rest = strip_path_prefix(&upstream_path, &shim.origin).unwrap_or(&upstream_path);
            let path = root.join("riso").join(name).join(rest);
            let Ok(ours) = fs::read(&path) else {
                continue;
            };
            if upstream.read_at(&shim.base, &upstream_path)?.as_deref() == Some(ours.as_slice()) {
                continue;
            }
            let text = String::from_utf8_lossy(&ours);
            if !text
                .lines()
                .take(10)
                .any(|line| line.contains(MODIFICATION_HEADER))
            {
                findings.push(Finding {
                    code: "V7",
                    message: format!(
                        "{} differs from upstream but has no \"{MODIFICATION_HEADER} ...\" header in its first lines",
                        path.display()
                    ),
                });
            }
        }
    }
    Ok(())
}

/// V9: Neutron code installs the logger through `riso_log`, which keeps `log_err()` targets in
/// Neutron's layout. Direct `zlog` initialization is only allowed in the IDE mirror and `riso_log`.
fn check_logger_initialization(
    root: &Path,
    layout: &Layout,
    findings: &mut Vec<Finding>,
) -> Result<()> {
    const ALLOWED: &[&str] = &["riso/log"];
    for top in ["riso", "crate", "apps"] {
        let directory = root.join(top);
        if !directory.is_dir() {
            continue;
        }
        let walker = WalkDir::new(&directory)
            .into_iter()
            .filter_entry(|entry| entry.file_name() != "target");
        for entry in walker {
            let entry = entry.with_context(|| format!("walking {}", directory.display()))?;
            if !entry.file_type().is_file()
                || entry
                    .path()
                    .extension()
                    .is_none_or(|extension| extension != "rs")
            {
                continue;
            }
            let relative = entry
                .path()
                .strip_prefix(root)
                .ok()
                .and_then(Path::to_str)
                .unwrap_or_default()
                .replace('\\', "/");
            if strip_path_prefix(&relative, layout.ide_root()).is_some()
                || ALLOWED
                    .iter()
                    .any(|allowed| strip_path_prefix(&relative, allowed).is_some())
            {
                continue;
            }
            let Ok(text) = fs::read_to_string(entry.path()) else {
                continue;
            };
            for (index, line) in text.lines().enumerate() {
                if line.contains("zlog::init(") || line.contains("zlog::try_init(") {
                    findings.push(Finding {
                        code: "V9",
                        message: format!(
                            "{relative}:{}: initialize logging with `riso_log::init()` instead of zlog",
                            index + 1
                        ),
                    });
                }
            }
        }
    }
    Ok(())
}

/// V4: unresolved merge conflicts in `riso/`.
fn check_conflict_markers(root: &Path, findings: &mut Vec<Finding>) -> Result<()> {
    let riso = root.join("riso");
    if !riso.is_dir() {
        return Ok(());
    }
    for entry in WalkDir::new(&riso)
        .into_iter()
        .filter_entry(|entry| entry.file_name() != "target")
    {
        let entry = entry.with_context(|| format!("walking {}", riso.display()))?;
        if !entry.file_type().is_file() {
            continue;
        }
        let Ok(text) = fs::read_to_string(entry.path()) else {
            continue;
        };
        if text
            .lines()
            .any(|line| line.starts_with("<<<<<<< ") || line.starts_with(">>>>>>> "))
        {
            findings.push(Finding {
                code: "V4",
                message: format!("{} contains merge conflict markers", entry.path().display()),
            });
        }
    }
    Ok(())
}

#[derive(Deserialize)]
struct Metadata {
    packages: Vec<MetadataPackage>,
}

#[derive(Deserialize)]
struct MetadataPackage {
    name: String,
    manifest_path: PathBuf,
    dependencies: Vec<MetadataDependency>,
}

#[derive(Deserialize)]
struct MetadataDependency {
    name: String,
    kind: Option<String>,
    path: Option<PathBuf>,
}

/// Neutron workspace packages keyed by their canonical directory.
pub fn neutron_packages(
    root: &Path,
    upstream_names: &HashSet<&str>,
) -> Result<HashMap<PathBuf, NeutronPackage>> {
    let root = root
        .canonicalize()
        .context("canonicalizing the Neutron root")?;
    let output = Command::new("cargo")
        .current_dir(&root)
        .args(["metadata", "--no-deps", "--format-version", "1"])
        .output()
        .context("running cargo metadata in the Neutron root")?;
    if !output.status.success() {
        bail!("{}", String::from_utf8_lossy(&output.stderr).trim());
    }
    let metadata: Metadata =
        serde_json::from_slice(&output.stdout).context("parsing cargo metadata")?;

    let mut packages = HashMap::new();
    for package in metadata.packages {
        let directory = package
            .manifest_path
            .parent()
            .context("manifest path has no parent")?
            .to_path_buf();
        let relative = directory
            .strip_prefix(&root)
            .ok()
            .and_then(Path::to_str)
            .unwrap_or_default()
            .replace('\\', "/");
        let group = group_of(&relative, &package.name, upstream_names);
        packages.insert(
            directory,
            NeutronPackage {
                name: package.name,
                group,
                dependencies: package
                    .dependencies
                    .into_iter()
                    .map(|dependency| NeutronDependency {
                        name: dependency.name,
                        kind: dependency.kind,
                        path: dependency.path,
                    })
                    .collect(),
            },
        );
    }
    Ok(packages)
}

fn group_of(relative_directory: &str, name: &str, upstream_names: &HashSet<&str>) -> Group {
    let top = relative_directory.split('/').next().unwrap_or_default();
    match top {
        "core" => Group::Core,
        "riso" if upstream_names.contains(name) => Group::Shim,
        "riso" => Group::Riso,
        "crate" => Group::Crate,
        "tooling" => Group::Tooling,
        "apps" if strip_path_prefix(relative_directory, "apps/ide").is_some() => Group::Ide,
        "apps" => Group::Apps,
        _ => Group::Other,
    }
}
