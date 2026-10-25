//! Keeps shims in `riso/` up to date with the upstream crates they replace.
//!
//! A shim starts as a copy of its upstream crate and is then edited by hand. On every sync, the
//! upstream changes since the shim's `base` commit are 3-way merged into it (`git merge-file`), the
//! way a vendor branch works.

use crate::config::ShimSection;
use crate::layout::{Destination, strip_path_prefix};
use crate::merge::{self, MergeLog};
use crate::sync::Change;
use crate::upstream::Upstream;
use anyhow::{Context as _, Result, bail};
use std::path::Path;

pub struct ShimOutcome {
    pub changes: Vec<Change>,
    /// Files left with conflict markers or needing a decision.
    pub conflicts: Vec<String>,
    /// The commit to record as the shim's new base; `None` while conflicts remain.
    pub new_base: Option<String>,
}

pub fn merge(
    root: &Path,
    upstream: &Upstream,
    name: &str,
    shim: &ShimSection,
    dry_run: bool,
) -> Result<ShimOutcome> {
    let directory = format!("riso/{name}");
    let mut log = MergeLog::default();

    if !root.join(&directory).exists() {
        // Bootstrap: a new shim starts as an exact copy of its upstream crate.
        for file in upstream_files(upstream, &shim.origin) {
            let version = merge::current_upstream_version(upstream, &file)?
                .context("tracked file vanished")?;
            let path = shim_path(&directory, &shim.origin, &file);
            merge::write_version(
                root,
                &path,
                Some(&version),
                Destination::Shim,
                dry_run,
                &mut log,
            )?;
        }
        return Ok(ShimOutcome {
            changes: log.changes,
            conflicts: log.conflicts,
            new_base: Some(upstream.commit.clone()),
        });
    }
    if shim.base.is_empty() {
        bail!(
            "[shim.{name}] has no `base`; set it to the upstream commit the shim was copied from"
        );
    }
    if shim.base == upstream.commit {
        return Ok(ShimOutcome {
            changes: Vec::new(),
            conflicts: Vec::new(),
            new_base: None,
        });
    }

    let mut paths = upstream.files_at(&shim.base, &shim.origin)?;
    for file in upstream_files(upstream, &shim.origin) {
        if !paths.contains(&file) {
            paths.push(file);
        }
    }
    paths.sort();
    for upstream_path in paths {
        let path = shim_path(&directory, &shim.origin, &upstream_path);
        merge::merge_path(
            root,
            upstream,
            &shim.base,
            &upstream_path,
            &path,
            Destination::Shim,
            dry_run,
            &mut log,
        )?;
    }

    let new_base = log.conflicts.is_empty().then(|| upstream.commit.clone());
    Ok(ShimOutcome {
        changes: log.changes,
        conflicts: log.conflicts,
        new_base,
    })
}

fn upstream_files(upstream: &Upstream, origin: &str) -> Vec<String> {
    upstream
        .files
        .iter()
        .filter(|file| strip_path_prefix(&file.path, origin).is_some())
        .map(|file| file.path.clone())
        .collect()
}

fn shim_path(directory: &str, origin: &str, upstream_path: &str) -> String {
    let rest = strip_path_prefix(upstream_path, origin).unwrap_or(upstream_path);
    format!("{directory}/{rest}")
}
