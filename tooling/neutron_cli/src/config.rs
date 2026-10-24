use anyhow::{Context as _, Result};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

pub const CONFIG_FILE: &str = "upstream.toml";
pub const OVERLAY_FILE: &str = "neutron.workspace.toml";

#[derive(Debug, Deserialize)]
pub struct Config {
    pub upstream: UpstreamSection,
    pub layout: LayoutSection,
    #[serde(default)]
    pub core: CoreSection,
    #[serde(default)]
    pub policy: PolicySection,
    #[serde(default)]
    pub rewrite: RewriteSection,
    #[serde(default)]
    pub agents: AgentsSection,
    /// Crates in `riso/` that keep an upstream crate's name and replace it everywhere.
    #[serde(default)]
    pub shim: BTreeMap<String, ShimSection>,
    #[serde(default)]
    pub doctor: DoctorSection,
}

#[derive(Debug, Default, Deserialize)]
pub struct DoctorSection {
    /// Features `cargo neutron doctor` enables when checking core, for upstream crates whose tests
    /// only build when another workspace crate enables a feature.
    #[serde(default)]
    pub check_features: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ShimSection {
    /// Upstream directory the shim was derived from.
    pub origin: String,
    /// Upstream commit whose `origin` the shim's files were last merged with.
    #[serde(default)]
    pub base: String,
}

#[derive(Debug, Deserialize)]
pub struct UpstreamSection {
    pub repo: String,
    #[serde(rename = "ref")]
    pub reference: String,
    #[serde(default)]
    pub commit: String,
}

#[derive(Debug, Deserialize)]
pub struct LayoutSection {
    /// Destination of every upstream path that no other rule maps.
    pub ide: String,
    #[serde(default)]
    pub exclude: Vec<String>,
    /// Paths that exist at the Neutron root but are never written or cleaned by the sync, such as
    /// `.git`. Literals pointing into them resolve to the same path at the Neutron root.
    #[serde(default)]
    pub unmanaged: Vec<String>,
    /// Upstream paths copied to the Neutron root, keyed by upstream path.
    #[serde(default)]
    pub root: BTreeMap<String, String>,
}

#[derive(Debug, Default, Deserialize)]
pub struct CoreSection {
    /// Upstream crates the framework itself needs. Core is their dependency closure, so a crate
    /// that is merely useful never ends up in core by accident.
    #[serde(default)]
    pub roots: Vec<String>,
}

#[derive(Debug, Default, Deserialize)]
pub struct PolicySection {
    /// Dev-dependencies of core crates that may point outside core, as `"from -> to"`.
    #[serde(default)]
    pub allow_core_dev_deps: Vec<String>,
    /// Upstream files in core whose out-of-crate paths are known not to resolve in Neutron.
    #[serde(default)]
    pub allow_core_path_escapes: Vec<String>,
}

#[derive(Debug, Default, Deserialize)]
pub struct RewriteSection {
    #[serde(default)]
    pub literals: Vec<LiteralRewrite>,
}

/// A string literal that the automatic path fixer cannot detect on its own, for example a path
/// that is resolved against the current directory or a file that does not exist in upstream.
#[derive(Debug, Clone, Deserialize)]
pub struct LiteralRewrite {
    pub file: String,
    pub literal: String,
    pub base: LiteralBase,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LiteralBase {
    /// Relative to the repository root.
    Root,
    /// Relative to the directory of the package that contains the file.
    Crate,
    /// Relative to the directory of the file.
    File,
}

#[derive(Debug, Default, Deserialize)]
pub struct AgentsSection {
    /// Top-level sections of upstream `.rules` copied into `AGENTS.md`.
    #[serde(default)]
    pub sections: Vec<String>,
    /// Lines of the copied sections that contain any of these strings are dropped, for rules that
    /// only make sense in the Zed repository.
    #[serde(default)]
    pub drop_lines_containing: Vec<String>,
    /// Skills copied from upstream `.agents/skills`.
    #[serde(default)]
    pub skills: Vec<String>,
}

impl Config {
    pub fn load(root: &Path) -> Result<Self> {
        let path = root.join(CONFIG_FILE);
        let text =
            fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
        toml::from_str(&text).with_context(|| format!("parsing {}", path.display()))
    }

    /// Records the upstream commit a shim was last merged with.
    pub fn record_shim_base(root: &Path, shim: &str, base: &str) -> Result<()> {
        let path = root.join(CONFIG_FILE);
        let text =
            fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
        let mut document = text
            .parse::<toml_edit::DocumentMut>()
            .with_context(|| format!("parsing {}", path.display()))?;
        let table = document["shim"][shim]
            .as_table_mut()
            .with_context(|| format!("`[shim.{shim}]` must be a table"))?;
        set_string_keeping_decor(table, "base", base);
        let updated = document.to_string();
        if updated != text {
            fs::write(&path, updated).with_context(|| format!("writing {}", path.display()))?;
        }
        Ok(())
    }

    /// Records the synced upstream commit in `upstream.toml`, keeping its comments and layout.
    pub fn record_upstream_commit(root: &Path, commit: &str, commit_date: &str) -> Result<()> {
        let path = root.join(CONFIG_FILE);
        let text =
            fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
        let mut document = text
            .parse::<toml_edit::DocumentMut>()
            .with_context(|| format!("parsing {}", path.display()))?;
        let upstream = document["upstream"]
            .as_table_mut()
            .context("`[upstream]` must be a table")?;
        set_string_keeping_decor(upstream, "commit", commit);
        set_string_keeping_decor(upstream, "commit_date", commit_date);
        let updated = document.to_string();
        if updated != text {
            fs::write(&path, updated).with_context(|| format!("writing {}", path.display()))?;
        }
        Ok(())
    }
}

fn set_string_keeping_decor(table: &mut toml_edit::Table, key: &str, value: &str) {
    match table.get_mut(key).and_then(|item| item.as_value_mut()) {
        Some(existing) => {
            let decor = existing.decor().clone();
            *existing = toml_edit::Value::from(value);
            *existing.decor_mut() = decor;
        }
        None => {
            table.insert(key, toml_edit::value(value));
        }
    }
}
