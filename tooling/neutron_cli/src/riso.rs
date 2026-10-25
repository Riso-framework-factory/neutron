//! Keeps files in `riso/` that were taken from upstream Zed in step with their origin.
//!
//! Each file declared in `[[riso.files]]` follows one upstream file. The first sync copies it with a
//! "Modified from Zed" header; after that, `merge` files get upstream changes 3-way merged into
//! them, and `watch` files only get the upstream commits reported, because they were changed too much
//! for a merge to work. Someone ports those changes by hand and runs `cargo neutron ack`.

use crate::config::{RisoFile, RisoMode};
use crate::layout::{Destination, strip_path_prefix};
use crate::merge::{self, MergeLog, Version};
use crate::upstream::{Upstream, git};
use anyhow::{Result, bail};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

pub const HEADER_MARKER: &str = "Modified from Zed";

pub struct RisoOutcome {
    pub log: MergeLog,
    /// `(riso path, new base)` for files that are now in step with upstream.
    pub new_bases: Vec<(String, String)>,
    /// Watch-mode files with upstream commits still to port.
    pub pending: Vec<PendingFile>,
    pub warnings: Vec<String>,
}

pub struct PendingFile {
    pub path: String,
    pub origin: String,
    /// `<short sha> <subject>` of every upstream commit that touched the origin since `base`.
    pub commits: Vec<String>,
}

pub fn validate(files: &[RisoFile]) -> Result<()> {
    let mut seen = std::collections::BTreeSet::new();
    for file in files {
        if strip_path_prefix(&file.path, "riso").is_none() {
            bail!("[[riso.files]] path {} must be under riso/", file.path);
        }
        if !seen.insert(file.path.as_str()) {
            bail!("[[riso.files]] lists {} twice", file.path);
        }
    }
    Ok(())
}

pub fn sync(
    root: &Path,
    upstream: &Upstream,
    files: &[RisoFile],
    dry_run: bool,
) -> Result<RisoOutcome> {
    validate(files)?;
    let mut outcome = RisoOutcome {
        log: MergeLog::default(),
        new_bases: Vec::new(),
        pending: Vec::new(),
        warnings: Vec::new(),
    };
    for file in files {
        if upstream.tracked_file(&file.origin).is_none() {
            outcome.warnings.push(format!(
                "{}: its origin {} no longer exists upstream; find where it moved and update [[riso.files]]",
                file.path, file.origin
            ));
            continue;
        }
        if !root.join(&file.path).exists() {
            bootstrap(root, upstream, file, dry_run, &mut outcome)?;
            continue;
        }
        if file.base.is_empty() {
            bail!(
                "[[riso.files]] {} has no `base`; set it to the upstream commit it was copied from",
                file.path
            );
        }
        if file.base == upstream.commit {
            continue;
        }
        match file.mode {
            RisoMode::Merge => {
                let conflicts_before = outcome.log.conflicts.len();
                merge::merge_path(
                    root,
                    upstream,
                    &file.base,
                    &file.origin,
                    &file.path,
                    Destination::Riso,
                    dry_run,
                    &mut outcome.log,
                )?;
                if outcome.log.conflicts.len() == conflicts_before {
                    outcome
                        .new_bases
                        .push((file.path.clone(), upstream.commit.clone()));
                }
            }
            RisoMode::Watch => {
                let commits = commits_touching(upstream, &file.base, &file.origin)?;
                if !commits.is_empty() {
                    outcome.pending.push(PendingFile {
                        path: file.path.clone(),
                        origin: file.origin.clone(),
                        commits,
                    });
                }
            }
        }
    }
    Ok(outcome)
}

/// Upstream commits that touched `origin` after `base`, newest first.
pub fn commits_touching(upstream: &Upstream, base: &str, origin: &str) -> Result<Vec<String>> {
    if base.is_empty() || base == upstream.commit {
        return Ok(Vec::new());
    }
    let range = format!("{base}..HEAD");
    let log = git(
        &upstream.root,
        &["log", "--format=%h %s", &range, "--", origin],
    )?;
    Ok(log.lines().map(str::to_string).collect())
}

fn bootstrap(
    root: &Path,
    upstream: &Upstream,
    file: &RisoFile,
    dry_run: bool,
    outcome: &mut RisoOutcome,
) -> Result<()> {
    let Some(version) = merge::current_upstream_version(upstream, &file.origin)? else {
        return Ok(());
    };
    let Some(header) = header_for(&file.path, &file.origin) else {
        bail!(
            "{}: cannot add a \"{HEADER_MARKER}\" header to this file type; take only .rs and .toml files",
            file.path
        );
    };
    let mut bytes = header.into_bytes();
    bytes.extend_from_slice(&version.bytes);
    let version = Version {
        bytes,
        kind: version.kind,
    };
    merge::write_version(
        root,
        &file.path,
        Some(&version),
        Destination::Riso,
        dry_run,
        &mut outcome.log,
    )?;
    outcome
        .new_bases
        .push((file.path.clone(), upstream.commit.clone()));
    Ok(())
}

/// The GPLv3 section 5(a) notice that a file was modified, with the date it was taken.
fn header_for(path: &str, origin: &str) -> Option<String> {
    let comment = if path.ends_with(".rs") {
        "//"
    } else if path.ends_with(".toml") {
        "#"
    } else {
        return None;
    };
    Some(format!(
        "{comment} {HEADER_MARKER} ({origin}) for Neutron, {}. `cargo neutron sync` tracks upstream changes.\n",
        today()
    ))
}

/// Today's UTC date as `YYYY-MM-DD`, without pulling in a date library.
fn today() -> String {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs());
    let (year, month, day) = civil_from_days((seconds / 86_400) as i64);
    format!("{year:04}-{month:02}-{day:02}")
}

/// Converts days since 1970-01-01 to a proleptic Gregorian date (Howard Hinnant's algorithm).
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let shifted = days + 719_468;
    let era = shifted.div_euclid(146_097);
    let day_of_era = shifted.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_index = (5 * day_of_year + 2) / 153;
    let day = (day_of_year - (153 * month_index + 2) / 5 + 1) as u32;
    let month = if month_index < 10 {
        month_index + 3
    } else {
        month_index - 9
    } as u32;
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_days_to_dates() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(20_386), (2025, 10, 25));
        assert_eq!(civil_from_days(-1), (1969, 12, 31));
    }

    #[test]
    fn writes_headers_for_rust_and_toml() {
        assert!(
            header_for("riso/a/src/a.rs", "crates/a/src/a.rs")
                .is_some_and(|header| header.starts_with("// Modified from Zed"))
        );
        assert!(
            header_for("riso/a/Cargo.toml", "crates/a/Cargo.toml")
                .is_some_and(|header| header.starts_with("# Modified from Zed"))
        );
        assert!(header_for("riso/a/README.md", "crates/a/README.md").is_none());
    }
}
