//! Keeps Neutron's `Cargo.lock` on exactly the versions upstream Zed tested.
//!
//! The lock cannot be a byte-for-byte copy of upstream's because Neutron adds workspace members
//! (riso crates, apps). Instead, every package upstream locks must appear with the same version and
//! source, and Neutron may only add entries.

use crate::upstream::Upstream;
use anyhow::{Context as _, Result, bail};
use serde::Deserialize;
use std::collections::BTreeSet;
use std::fs;
use std::path::Path;
use std::process::Command;

#[derive(Deserialize)]
struct Lockfile {
    #[serde(default)]
    package: Vec<LockedPackage>,
}

#[derive(Deserialize, PartialEq, Eq, PartialOrd, Ord)]
struct LockedPackage {
    name: String,
    version: String,
    source: Option<String>,
}

/// Upstream packages that are missing from Neutron's lock or locked at another version or source.
pub fn drift(root: &Path, upstream: &Upstream) -> Result<Vec<String>> {
    let upstream_lock = String::from_utf8(upstream.read("Cargo.lock")?)
        .context("upstream Cargo.lock is not UTF-8")?;
    let neutron_path = root.join("Cargo.lock");
    let Ok(neutron_lock) = fs::read_to_string(&neutron_path) else {
        return Ok(vec!["Cargo.lock is missing".to_string()]);
    };
    let upstream_packages = parse(&upstream_lock).context("parsing upstream Cargo.lock")?;
    let neutron_packages = parse(&neutron_lock).context("parsing Cargo.lock")?;
    Ok(upstream_packages
        .difference(&neutron_packages)
        .map(|package| match &package.source {
            Some(source) => format!("{} {} ({source})", package.name, package.version),
            None => format!("{} {} (workspace)", package.name, package.version),
        })
        .collect())
}

/// Replaces a drifted lock with upstream's and lets cargo add Neutron's own members, without
/// changing any upstream version. Returns whether the lock was rewritten.
pub fn sync(root: &Path, upstream: &Upstream, dry_run: bool) -> Result<bool> {
    let drifted = !drift(root, upstream)?.is_empty();
    if !drifted && is_complete(root)? {
        return Ok(false);
    }
    if dry_run {
        return Ok(true);
    }
    if drifted {
        let path = root.join("Cargo.lock");
        fs::write(&path, upstream.read("Cargo.lock")?)
            .with_context(|| format!("writing {}", path.display()))?;
    }
    // `--workspace` only adds or updates workspace members; external versions stay as locked.
    let status = Command::new("cargo")
        .current_dir(root)
        .args(["update", "--workspace", "--quiet"])
        .status()
        .context("running cargo update --workspace")?;
    if !status.success() {
        bail!("cargo update --workspace failed in {}", root.display());
    }
    let remaining = drift(root, upstream)?;
    if !remaining.is_empty() {
        bail!(
            "Cargo.lock still differs from upstream after `cargo update --workspace`: {}",
            remaining.join(", ")
        );
    }
    Ok(true)
}

/// Whether the lock already covers every workspace member, so cargo would not have to change it.
pub fn is_complete(root: &Path) -> Result<bool> {
    let output = Command::new("cargo")
        .current_dir(root)
        .args(["metadata", "--format-version", "1", "--locked"])
        .output()
        .context("running cargo metadata --locked")?;
    Ok(output.status.success())
}

fn parse(text: &str) -> Result<BTreeSet<LockedPackage>> {
    let lockfile: Lockfile = toml::from_str(text)?;
    Ok(lockfile.package.into_iter().collect())
}
