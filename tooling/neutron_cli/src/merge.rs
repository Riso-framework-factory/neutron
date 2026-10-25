//! 3-way merging of single files from upstream into Neutron-owned copies (shims and riso files).

use crate::layout::Destination;
use crate::sync::{Change, ChangeKind, Entry, needs_write, write_entry};
use crate::upstream::{FileKind, Upstream};
use anyhow::{Context as _, Result, bail};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

/// What a merge changed and what still needs a person.
#[derive(Default)]
pub struct MergeLog {
    pub changes: Vec<Change>,
    /// Files left with conflict markers or needing a decision.
    pub conflicts: Vec<String>,
}

/// A file's content plus whether it is a symlink, so that symlinks compare by target.
#[derive(Clone, PartialEq, Eq)]
pub struct Version {
    pub bytes: Vec<u8>,
    pub kind: FileKind,
}

/// Merges the upstream changes to `upstream_path` between `base_commit` and the checked-out commit
/// into the Neutron file `neutron_path`.
#[allow(clippy::too_many_arguments)]
pub fn merge_path(
    root: &Path,
    upstream: &Upstream,
    base_commit: &str,
    upstream_path: &str,
    neutron_path: &str,
    destination: Destination,
    dry_run: bool,
    log: &mut MergeLog,
) -> Result<()> {
    let base = base_version(upstream, base_commit, upstream_path)?;
    let theirs = current_upstream_version(upstream, upstream_path)?;
    if base == theirs {
        return Ok(());
    }
    let ours = neutron_version(&root.join(neutron_path))?;
    if ours == theirs {
        return Ok(());
    }
    if ours == base {
        return write_version(
            root,
            neutron_path,
            theirs.as_ref(),
            destination,
            dry_run,
            log,
        );
    }
    match (&base, &theirs, &ours) {
        (_, Some(theirs), Some(ours))
            if theirs.kind != FileKind::Symlink && ours.kind != FileKind::Symlink =>
        {
            let base_bytes = base.as_ref().map_or(&[][..], |base| base.bytes.as_slice());
            let (merged, clean) = merge_file(&ours.bytes, base_bytes, &theirs.bytes)?;
            let merged = Version {
                bytes: merged,
                kind: ours.kind,
            };
            write_version(root, neutron_path, Some(&merged), destination, dry_run, log)?;
            if !clean {
                log.conflicts.push(format!(
                    "{neutron_path}: conflicting changes with upstream {upstream_path}; resolve the \
                     conflict markers, then run `cargo neutron ack {neutron_path}`"
                ));
            }
        }
        (_, None, Some(_)) => log.conflicts.push(format!(
            "{neutron_path}: {upstream_path} was deleted upstream but is modified here; delete it or keep it deliberately"
        )),
        (_, Some(_), None) => log.conflicts.push(format!(
            "{neutron_path}: removed here but {upstream_path} changed upstream; check whether the change matters"
        )),
        _ => log
            .conflicts
            .push(format!("{neutron_path}: conflicting symlink changes")),
    }
    Ok(())
}

pub fn base_version(upstream: &Upstream, base: &str, path: &str) -> Result<Option<Version>> {
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

pub fn current_upstream_version(upstream: &Upstream, path: &str) -> Result<Option<Version>> {
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

pub fn neutron_version(path: &Path) -> Result<Option<Version>> {
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

pub fn write_version(
    root: &Path,
    path: &str,
    version: Option<&Version>,
    destination: Destination,
    dry_run: bool,
    log: &mut MergeLog,
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
            log.changes.push(Change {
                path: path.to_string(),
                kind: ChangeKind::Write,
                destination: Some(destination),
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
            log.changes.push(Change {
                path: path.to_string(),
                kind: ChangeKind::Delete,
                destination: Some(destination),
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
