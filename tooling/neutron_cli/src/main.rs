// A synchronous command-line tool: blocking on child processes is intended, unlike in the GPUI
// apps that Zed's clippy.toml is written for.
#![allow(clippy::disallowed_methods)]

mod agents;
mod config;
mod layout;
mod lockfile;
mod manifest;
mod rewrite;
mod shim;
mod sync;
mod upstream;
mod verify;

use anyhow::{Context as _, Result, bail};
use clap::{Args, Parser, Subcommand};
use config::{CONFIG_FILE, Config};
use layout::{Destination, Layout};
use std::collections::HashSet;
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use sync::{ChangeKind, SyncOutcome};
use upstream::Upstream;
use verify::{Finding, Group};

#[derive(Parser)]
#[command(
    name = "cargo neutron",
    about = "Syncs Neutron with upstream Zed and checks its invariants"
)]
struct Cli {
    /// Neutron checkout. Defaults to the nearest ancestor of the current directory with `upstream.toml`.
    #[arg(long, global = true)]
    root: Option<PathBuf>,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Copy an upstream Zed checkout into Neutron according to `upstream.toml`.
    Sync(SyncArgs),
    /// Check that Neutron matches upstream and follows its dependency rules.
    Verify(VerifyArgs),
    /// Run `verify`, then type-check core, shims and riso with all targets and the IDE canary.
    /// Run it before merging a sync pull request.
    Doctor(DoctorArgs),
    /// Print `-p <crate>` arguments for every workspace crate in the given groups.
    Members {
        #[arg(required = true, value_enum)]
        groups: Vec<Group>,
    },
}

#[derive(Args)]
struct UpstreamArgs {
    /// Path to a checkout of zed-industries/zed at the commit to sync.
    #[arg(long)]
    upstream: PathBuf,
    /// Accept uncommitted changes to tracked files in the upstream checkout.
    #[arg(long)]
    allow_dirty: bool,
}

#[derive(Args)]
struct SyncArgs {
    #[command(flatten)]
    upstream: UpstreamArgs,
    /// Report what would change without writing anything.
    #[arg(long)]
    dry_run: bool,
    /// Write a markdown report (used as the body of the sync pull request).
    #[arg(long)]
    report: Option<PathBuf>,
    /// Append `key=value` outputs for GitHub Actions to this file.
    #[arg(long)]
    github_output: Option<PathBuf>,
}

#[derive(Args)]
struct DoctorArgs {
    #[command(flatten)]
    upstream: UpstreamArgs,
    /// Skip `cargo check -p zed`, the slowest step.
    #[arg(long)]
    skip_ide: bool,
}

#[derive(Args)]
struct VerifyArgs {
    #[command(flatten)]
    upstream: UpstreamArgs,
    /// Append the findings to this markdown report.
    #[arg(long)]
    append_report: Option<PathBuf>,
}

fn main() -> ExitCode {
    match run(Cli::parse()) {
        Ok(code) => code,
        Err(error) => {
            eprintln!("error: {error:#}");
            ExitCode::FAILURE
        }
    }
}

fn run(cli: Cli) -> Result<ExitCode> {
    let root = match cli.root {
        Some(root) => root,
        None => find_root()?,
    };
    match cli.command {
        Command::Sync(arguments) => sync_command(&root, arguments),
        Command::Verify(arguments) => verify_command(&root, arguments),
        Command::Doctor(arguments) => doctor_command(&root, arguments),
        Command::Members { groups } => members_command(&root, &groups),
    }
}

fn find_root() -> Result<PathBuf> {
    let current = std::env::current_dir().context("reading the current directory")?;
    current
        .ancestors()
        .find(|directory| directory.join(CONFIG_FILE).is_file())
        .map(Path::to_path_buf)
        .with_context(|| {
            format!(
                "no {CONFIG_FILE} found in {} or its parents",
                current.display()
            )
        })
}

fn sync_command(root: &Path, arguments: SyncArgs) -> Result<ExitCode> {
    let config = Config::load(root)?;
    let upstream = Upstream::load(&arguments.upstream.upstream, arguments.upstream.allow_dirty)?;
    let layout = Layout::new(&config, &upstream)?;
    let previous_commit = config.upstream.commit.clone();

    let outcome = sync::run(root, &config, &upstream, &layout, arguments.dry_run)?;
    if !arguments.dry_run {
        Config::record_upstream_commit(root, &upstream.commit, &upstream.commit_date)?;
    }

    let summary = summarize(&outcome);
    println!(
        "{} zed@{} ({}): {summary}",
        if arguments.dry_run {
            "Would sync"
        } else {
            "Synced"
        },
        upstream.short_commit(),
        upstream.commit_date,
    );
    for warning in &outcome.warnings {
        eprintln!("warning: {warning}");
    }
    for conflict in &outcome.conflicts {
        eprintln!("conflict: {conflict}");
    }

    if let Some(path) = &arguments.report {
        let report = sync_report(&config, &upstream, &previous_commit, &outcome)?;
        fs::write(path, report).with_context(|| format!("writing {}", path.display()))?;
    }
    if let Some(path) = &arguments.github_output {
        let mut output = String::new();
        writeln!(output, "sha={}", upstream.commit)?;
        writeln!(output, "short_sha={}", upstream.short_commit())?;
        writeln!(output, "changed={}", !outcome.changes.is_empty())?;
        writeln!(output, "conflicts={}", !outcome.conflicts.is_empty())?;
        writeln!(output, "triage={}", !outcome.warnings.is_empty())?;
        append(path, &output)?;
    }
    Ok(ExitCode::SUCCESS)
}

fn verify_command(root: &Path, arguments: VerifyArgs) -> Result<ExitCode> {
    let config = Config::load(root)?;
    let upstream = Upstream::load(&arguments.upstream.upstream, arguments.upstream.allow_dirty)?;
    if !config.upstream.commit.is_empty() && config.upstream.commit != upstream.commit {
        bail!(
            "upstream.toml records zed@{} but the checkout is at zed@{}; check out the recorded commit",
            config.upstream.commit,
            upstream.commit
        );
    }
    let layout = Layout::new(&config, &upstream)?;
    let findings = verify::run(root, &config, &upstream, &layout)?;

    for finding in &findings {
        println!("{finding}");
    }
    if let Some(path) = &arguments.append_report {
        append(path, &verify_report(&findings)?)?;
    }
    if findings.is_empty() {
        println!("verify: ok (zed@{})", upstream.short_commit());
        Ok(ExitCode::SUCCESS)
    } else {
        println!("verify: {} finding(s)", findings.len());
        Ok(ExitCode::FAILURE)
    }
}

fn doctor_command(root: &Path, arguments: DoctorArgs) -> Result<ExitCode> {
    let verified = verify_command(
        root,
        VerifyArgs {
            upstream: UpstreamArgs {
                upstream: arguments.upstream.upstream,
                allow_dirty: arguments.upstream.allow_dirty,
            },
            append_report: None,
        },
    )?;
    if verified != ExitCode::SUCCESS {
        return Ok(verified);
    }

    let config = Config::load(root)?;
    let packages = verify::neutron_packages(root, &HashSet::new())?;
    let mut check = vec!["check".to_string(), "--all-targets".to_string()];
    let mut names: Vec<&str> = packages
        .values()
        .filter(|package| matches!(package.group, Group::Core | Group::Riso))
        .map(|package| package.name.as_str())
        .collect();
    names.sort_unstable();
    for name in names {
        check.push("-p".to_string());
        check.push(name.to_string());
    }
    for feature in &config.doctor.check_features {
        check.push("--features".to_string());
        check.push(feature.clone());
    }
    let mut steps = vec![("core, shims and riso", check)];
    if !arguments.skip_ide {
        steps.push((
            "IDE canary",
            vec!["check".to_string(), "-p".to_string(), "zed".to_string()],
        ));
    }

    for (label, cargo_arguments) in steps {
        println!("doctor: cargo check ({label})");
        let status = std::process::Command::new("cargo")
            .current_dir(root)
            .args(&cargo_arguments)
            .status()
            .context("running cargo")?;
        if !status.success() {
            println!("doctor: cargo check ({label}) failed: {status}");
            return Ok(ExitCode::FAILURE);
        }
    }
    println!("doctor: ok");
    Ok(ExitCode::SUCCESS)
}

fn members_command(root: &Path, groups: &[Group]) -> Result<ExitCode> {
    // Shims are recognized by their upstream crate names, which only upstream.toml's core list
    // and the riso directory tell us without an upstream checkout; treat every riso crate as riso.
    let packages = verify::neutron_packages(root, &HashSet::new())?;
    let mut names: Vec<&str> = packages
        .values()
        .filter(|package| {
            groups.contains(&package.group)
                || (package.group == Group::Riso && groups.contains(&Group::Shim))
        })
        .map(|package| package.name.as_str())
        .collect();
    names.sort_unstable();
    let arguments: Vec<String> = names.iter().map(|name| format!("-p {name}")).collect();
    println!("{}", arguments.join(" "));
    Ok(ExitCode::SUCCESS)
}

fn summarize(outcome: &SyncOutcome) -> String {
    let written = outcome
        .changes
        .iter()
        .filter(|change| change.kind == ChangeKind::Write)
        .count();
    let deleted = outcome.changes.len() - written;
    format!(
        "{written} file(s) written, {deleted} deleted, {} warning(s); {} path literal(s) rewritten \
         across the mirror, {} of them in files this sync changed",
        outcome.warnings.len(),
        outcome.rewrites.len(),
        rewrites_in_changed_files(outcome).len()
    )
}

/// Rewrites in files that this sync wrote. The others were already applied by an earlier sync.
fn rewrites_in_changed_files(outcome: &SyncOutcome) -> Vec<&rewrite::Rewrite> {
    let changed: HashSet<&str> = outcome
        .changes
        .iter()
        .filter(|change| change.kind == ChangeKind::Write)
        .map(|change| change.path.as_str())
        .collect();
    outcome
        .rewrites
        .iter()
        .filter(|rewrite| changed.contains(rewrite.neutron_path.as_str()))
        .collect()
}

fn sync_report(
    config: &Config,
    upstream: &Upstream,
    previous_commit: &str,
    outcome: &SyncOutcome,
) -> Result<String> {
    let mut report = String::new();
    writeln!(report, "# Sync zed@{}\n", upstream.short_commit())?;
    writeln!(
        report,
        "- Upstream: {} @ `{}` ({}), tracking `{}`",
        config.upstream.repo, upstream.commit, upstream.commit_date, config.upstream.reference
    )?;
    match upstream.commits_since(previous_commit) {
        Some(count) => {
            writeln!(
                report,
                "- Previous sync: `{previous_commit}` ({count} upstream commits)"
            )?;
        }
        None if previous_commit.is_empty() => {
            writeln!(report, "- Previous sync: none")?;
        }
        None => {
            writeln!(
                report,
                "- Previous sync: `{previous_commit}` (not an ancestor of this commit)"
            )?;
        }
    }
    writeln!(report, "- Result: {}", summarize(outcome))?;

    let core_changes: Vec<&str> = outcome
        .changes
        .iter()
        .filter(|change| change.destination == Some(Destination::Core))
        .map(|change| change.path.as_str())
        .collect();
    let mut core_crates: Vec<&str> = core_changes
        .iter()
        .filter_map(|path| path.strip_prefix("core/"))
        .filter_map(|path| path.split('/').next())
        .collect();
    core_crates.dedup();
    writeln!(
        report,
        "- Core files changed: {} in {} crate(s){}",
        core_changes.len(),
        core_crates.len(),
        if core_crates.is_empty() {
            String::new()
        } else {
            format!(": {}", core_crates.join(", "))
        }
    )?;

    if !outcome.conflicts.is_empty() {
        writeln!(report, "\n## Shim conflicts\n")?;
        for conflict in &outcome.conflicts {
            writeln!(report, "- {conflict}")?;
        }
    }
    if !outcome.warnings.is_empty() {
        writeln!(report, "\n## Needs triage\n")?;
        for warning in &outcome.warnings {
            writeln!(report, "- {warning}")?;
        }
    }
    let changed_rewrites = rewrites_in_changed_files(outcome);
    if !changed_rewrites.is_empty() {
        writeln!(
            report,
            "\n<details><summary>Path literals rewritten in changed files</summary>\n"
        )?;
        for rewrite in changed_rewrites {
            writeln!(
                report,
                "- `{}`: `{}` → `{}`",
                rewrite.file, rewrite.from, rewrite.to
            )?;
        }
        writeln!(report, "\n</details>")?;
    }
    Ok(report)
}

fn verify_report(findings: &[Finding]) -> Result<String> {
    let mut report = String::from("\n## Verify\n\n");
    if findings.is_empty() {
        report.push_str("All checks passed.\n");
    }
    for finding in findings {
        writeln!(report, "- {finding}")?;
    }
    Ok(report)
}

fn append(path: &Path, text: &str) -> Result<()> {
    use std::io::Write as _;
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .with_context(|| format!("opening {}", path.display()))?;
    file.write_all(text.as_bytes())
        .with_context(|| format!("writing {}", path.display()))
}
