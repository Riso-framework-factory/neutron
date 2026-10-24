//! Keeps shims in `riso/` up to date with the upstream crates they replace.
//!
//! A shim starts as a copy of its upstream crate and is then edited by hand. On every sync, the
//! upstream changes since the shim's `base` commit are 3-way merged into it (`git merge-file`), the
//! way a vendor branch works.

use crate::config::ShimSection;
use crate::layout::{Destination, strip_path_prefix};
use crate::sync::{Change, ChangeKind, Entry, needs_write, write_entry};
use crate::upstream::{FileKind, Upstream};
use anyhow::{Context as _, Result, bail};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

pub struct ShimOutcome {
    pub changes: Vec<Change>,
    /// Files left with conflict markers or needing a decision.
    pub conflicts: Vec<String>,
    /// The commit to record as the shim's new base; `None` while conflicts remain.
    pub new_base: Option<String>,
}

/// A file's content plus whether it is a symlink, so that symlinks compare by target.
#[derive(Clone, PartialEq, Eq)]
struct Version {
    bytes: Vec<u8>,
    kind: FileKind,
}

pub fn merge(
    root: &Path,
    upstream: &Upstream,
    name: &str,
    shim: &ShimSection,
    dry_run: bool,
) -> Result<ShimOutcome> {
    let directory = format!("riso/{name}");
    let mut outcome = ShimOutcome {
        changes: Vec::new(),
        conflicts: Vec::new(),
        new_base: None,
    };

    if !root.join(&directory).exists() {
        // Bootstrap: a new shim starts as an exact copy of its upstream crate.
        for file in upstream_files(upstream, &shim.origin) {
            let version =
                current_upstream_version(upstream, &file)?.context("tracked file vanished")?;
            let path = shim_path(&directory, &shim.origin, &file);
            apply(root, &path, Some(&version), dry_run, &mut outcome)?;
        }
        outcome.new_base = Some(upstream.commit.clone());
        return Ok(outcome);
    }
    if shim.base.is_empty() {
        bail!(
            "[shim.{name}] has no `base`; set it to the upstream commit the shim was copied from"
        );
    }
    if shim.base == upstream.commit {
        return Ok(outcome);
    }

    let mut paths = upstream.files_at(&shim.base, &shim.origin)?;
    for file in upstream_files(upstream, &shim.origin) {
        if !paths.contains(&file) {
            paths.push(file);
        }
    }
    paths.sort();

    for upstream_path in paths {
        let base = base_version(upstream, &shim.base, &upstream_path)?;
        let theirs = current_upstream_version(upstream, &upstream_path)?;
        if base == theirs {
            continue;
        }
        let path = shim_path(&directory, &shim.origin, &upstream_path);
        let ours = neutron_version(&root.join(&path))?;
        if ours == theirs {
            continue;
        }
        if ours == base {
            apply(root, &path, theirs.as_ref(), dry_run, &mut outcome)?;
            continue;
        }
        match (&base, &theirs, &ours) {
            (_, Some(theirs), Some(ours)) if theirs.kind != FileKind::Symlink && ours.kind != FileKind::Symlink => {
                let base_bytes = base.as_ref().map_or(&[][..], |base| base.bytes.as_slice());
                let (merged, clean) = merge_file(&ours.bytes, base_bytes, &theirs.bytes)?;
                let merged = Version {
                    bytes: merged,
                    kind: ours.kind,
                };
                apply(root, &path, Some(&merged), dry_run, &mut outcome)?;
                if !clean {
                    outcome.conflicts.push(format!("{path}: conflicting changes, resolve the conflict markers"));
                }
            }
            (_, None, Some(_)) => outcome.conflicts.push(format!(
                "{path}: deleted upstream but modified in the shim; delete it or keep it deliberately"
            )),
            (_, Some(_), None) => outcome.conflicts.push(format!(
                "{path}: removed from the shim but changed upstream; check whether the change matters"
            )),
            _ => outcome.conflicts.push(format!("{path}: conflicting symlink changes")),
        }
    }

    if outcome.conflicts.is_empty() {
        outcome.new_base = Some(upstream.commit.clone());
    }
    Ok(outcome)
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

fn base_version(upstream: &Upstream, base: &str, path: &str) -> Result<Option<Version>> {
    let Some(bytes) = upstream.read_at(base, path)? else {
        return Ok(None);
    };
    // `git cat-file` does not report the mode, so assume it did not change since the base.
    let kind = match upstream.tracked_file(path) {
        Some(file) => file.kind,
        None => FileKind::Regular,
    };
    Ok(Some(Version { bytes, kind }))
}

fn current_upstream_version(upstream: &Upstream, path: &str) -> Result<Option<Version>> {
    let Some(file) = upstream.tracked_file(path) else {
        return Ok(None);
    };
    let bytes = match file.kind {
        FileKind::Symlink => upstream.read_link(path)?.into_bytes(),
        FileKind::Regular | FileKind::Executable => upstream.read(path)?,
    };
    Ok(Some(Version {
        bytes,
        kind: file.kind,
    }))
}

fn neutron_version(path: &Path) -> Result<Option<Version>> {
    let Ok(metadata) = fs::symlink_metadata(path) else {
        return Ok(None);
    };
    if metadata.file_type().is_symlink() {
        let target =
            fs::read_link(path).with_context(|| format!("reading symlink {}", path.display()))?;
        let target = target
            .to_str()
            .with_context(|| format!("symlink {} is not UTF-8", path.display()))?;
        return Ok(Some(Version {
            bytes: target.as_bytes().to_vec(),
            kind: FileKind::Symlink,
        }));
    }
    let bytes = fs::read(path).with_context(|| format!("reading {}", path.display()))?;
    let kind = if crate::sync::is_executable(&metadata) {
        FileKind::Executable
    } else {
        FileKind::Regular
    };
    Ok(Some(Version { bytes, kind }))
}

fn apply(
    root: &Path,
    path: &str,
    version: Option<&Version>,
    dry_run: bool,
    outcome: &mut ShimOutcome,
) -> Result<()> {
    let full_path = root.join(path);
    match version {
        Some(version) => {
            let entry = match version.kind {
                FileKind::Symlink => Entry::Symlink {
                    target: String::from_utf8(version.bytes.clone())
                        .context("symlink target is not UTF-8")?,
                },
                FileKind::Regular | FileKind::Executable => Entry::File {
                    content: version.bytes.clone(),
                    executable: version.kind == FileKind::Executable,
                },
            };
            if !needs_write(&full_path, &entry)? {
                return Ok(());
            }
            if !dry_run {
                write_entry(&full_path, &entry)?;
            }
            outcome.changes.push(Change {
                path: path.to_string(),
                kind: ChangeKind::Write,
                destination: Some(Destination::Shim),
            });
        }
        None => {
            if fs::symlink_metadata(&full_path).is_err() {
                return Ok(());
            }
            if !dry_run {
                fs::remove_file(&full_path)
                    .with_context(|| format!("deleting {}", full_path.display()))?;
            }
            outcome.changes.push(Change {
                path: path.to_string(),
                kind: ChangeKind::Delete,
                destination: Some(Destination::Shim),
            });
        }
    }
    Ok(())
}

/// Runs `git merge-file` and returns the merged bytes and whether the merge was clean.
fn merge_file(ours: &[u8], base: &[u8], theirs: &[u8]) -> Result<(Vec<u8>, bool)> {
    let ours_file = TempFile::new("ours", ours)?;
    let base_file = TempFile::new("base", base)?;
    let theirs_file = TempFile::new("upstream", theirs)?;
    let output = Command::new("git")
        .args([
            "merge-file",
            "-p",
            "-L",
            "neutron",
            "-L",
            "base",
            "-L",
            "upstream",
        ])
        .arg(&ours_file.path)
        .arg(&base_file.path)
        .arg(&theirs_file.path)
        .output()
        .context("running git merge-file")?;
    // The exit code is the number of conflicts; it is negative on errors.
    match output.status.code() {
        Some(0) => Ok((output.stdout, true)),
        Some(code) if code > 0 => Ok((output.stdout, false)),
        _ => bail!(
            "git merge-file failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ),
    }
}

struct TempFile {
    path: PathBuf,
}

impl TempFile {
    fn new(label: &str, content: &[u8]) -> Result<Self> {
        static COUNTER: AtomicUsize = AtomicUsize::new(0);
        let unique = COUNTER.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "neutron-merge-{}-{unique}-{label}",
            std::process::id()
        ));
        fs::write(&path, content).with_context(|| format!("writing {}", path.display()))?;
        Ok(Self { path })
    }
}

impl Drop for TempFile {
    fn drop(&mut self) {
        if let Err(error) = fs::remove_file(&self.path) {
            eprintln!("warning: could not remove {}: {error}", self.path.display());
        }
    }
}
