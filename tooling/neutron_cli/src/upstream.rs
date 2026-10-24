use anyhow::{Context as _, Result, bail};
use serde::Deserialize;
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// A clean checkout of upstream Zed: its tracked files and workspace packages.
pub struct Upstream {
    pub root: PathBuf,
    pub commit: String,
    pub commit_date: String,
    pub files: Vec<TrackedFile>,
    pub packages: Vec<Package>,
    /// Every tracked file and every directory that contains one.
    existing: HashSet<String>,
}

pub struct TrackedFile {
    /// Path relative to the upstream root, `/`-separated.
    pub path: String,
    pub kind: FileKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileKind {
    Regular,
    Executable,
    Symlink,
}

pub struct Package {
    pub name: String,
    /// Package directory relative to the upstream root, `/`-separated.
    pub dir: String,
    /// Names of the normal and build dependencies (dev-dependencies excluded).
    pub dependencies: Vec<String>,
}

impl Upstream {
    pub fn load(root: &Path, allow_dirty: bool) -> Result<Self> {
        let root = root
            .canonicalize()
            .with_context(|| format!("upstream checkout {} not found", root.display()))?;
        let commit = git(&root, &["rev-parse", "HEAD"])?.trim().to_string();
        let commit_date = git(&root, &["log", "-1", "--format=%cs", "HEAD"])?
            .trim()
            .to_string();

        let status = git(&root, &["status", "--porcelain", "--untracked-files=no"])?;
        if !status.trim().is_empty() && !allow_dirty {
            bail!(
                "upstream checkout {} has uncommitted changes to tracked files; \
                 commit or stash them, or pass --allow-dirty",
                root.display()
            );
        }

        let files = list_tracked_files(&root)?;
        let mut existing = HashSet::new();
        for file in &files {
            existing.insert(file.path.clone());
            let mut path = file.path.as_str();
            while let Some((parent, _)) = path.rsplit_once('/') {
                if !existing.insert(parent.to_string()) {
                    break;
                }
                path = parent;
            }
        }

        let packages = workspace_packages(&root)?;
        Ok(Self {
            root,
            commit,
            commit_date,
            files,
            packages,
            existing,
        })
    }

    pub fn short_commit(&self) -> &str {
        self.commit.get(..10).unwrap_or(&self.commit)
    }

    /// Whether `path` is a tracked file or a directory containing tracked files.
    pub fn exists(&self, path: &str) -> bool {
        self.existing.contains(path)
    }

    pub fn read(&self, path: &str) -> Result<Vec<u8>> {
        let full_path = self.root.join(path);
        fs::read(&full_path).with_context(|| format!("reading {}", full_path.display()))
    }

    pub fn read_link(&self, path: &str) -> Result<String> {
        let full_path = self.root.join(path);
        let target = fs::read_link(&full_path)
            .with_context(|| format!("reading symlink {}", full_path.display()))?;
        target
            .to_str()
            .map(|target| target.replace('\\', "/"))
            .with_context(|| format!("symlink {} is not valid UTF-8", full_path.display()))
    }

    /// The innermost package whose directory contains `path`.
    pub fn package_containing(&self, path: &str) -> Option<&Package> {
        self.packages
            .iter()
            .filter(|package| crate::layout::strip_path_prefix(path, &package.dir).is_some())
            .max_by_key(|package| package.dir.len())
    }

    pub fn package_named(&self, name: &str) -> Option<&Package> {
        self.packages.iter().find(|package| package.name == name)
    }

    /// Number of commits between `from` and the checked-out commit, if `from` is known upstream.
    pub fn commits_since(&self, from: &str) -> Option<usize> {
        if from.is_empty() {
            return None;
        }
        let range = format!("{from}..HEAD");
        git(&self.root, &["rev-list", "--count", &range])
            .ok()
            .and_then(|count| count.trim().parse().ok())
    }

    /// Contents of `path` at `commit`, or `None` if it did not exist there.
    pub fn read_at(&self, commit: &str, path: &str) -> Result<Option<Vec<u8>>> {
        let spec = format!("{commit}:{path}");
        let output = Command::new("git")
            .arg("-C")
            .arg(&self.root)
            .args(["cat-file", "blob", &spec])
            .output()
            .with_context(|| format!("running git cat-file blob {spec}"))?;
        Ok(output.status.success().then_some(output.stdout))
    }

    /// Files under `directory` at `commit`.
    pub fn files_at(&self, commit: &str, directory: &str) -> Result<Vec<String>> {
        let listing = git(
            &self.root,
            &[
                "ls-tree",
                "-r",
                "-z",
                "--name-only",
                commit,
                "--",
                directory,
            ],
        )?;
        Ok(listing
            .split('\0')
            .filter(|path| !path.is_empty())
            .map(str::to_string)
            .collect())
    }

    pub fn tracked_file(&self, path: &str) -> Option<&TrackedFile> {
        self.files.iter().find(|file| file.path == path)
    }
}

pub fn git(root: &Path, args: &[&str]) -> Result<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .with_context(|| format!("running git {}", args.join(" ")))?;
    if !output.status.success() {
        bail!(
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    String::from_utf8(output.stdout)
        .with_context(|| format!("git {} printed invalid UTF-8", args.join(" ")))
}

fn list_tracked_files(root: &Path) -> Result<Vec<TrackedFile>> {
    let listing = git(root, &["ls-files", "--stage", "-z"])?;
    let mut files = Vec::new();
    for record in listing.split('\0').filter(|record| !record.is_empty()) {
        let (metadata, path) = record
            .split_once('\t')
            .with_context(|| format!("unexpected `git ls-files` record {record:?}"))?;
        let mode = metadata.split(' ').next().unwrap_or_default();
        let kind = match mode {
            "100644" => FileKind::Regular,
            "100755" => FileKind::Executable,
            "120000" => FileKind::Symlink,
            // Submodules have no content in this checkout.
            "160000" => continue,
            other => bail!("unsupported git file mode {other} for {path}"),
        };
        files.push(TrackedFile {
            path: path.to_string(),
            kind,
        });
    }
    Ok(files)
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
}

fn workspace_packages(root: &Path) -> Result<Vec<Package>> {
    // Run inside the checkout so rustup picks upstream's toolchain.
    let output = Command::new("cargo")
        .current_dir(root)
        .args(["metadata", "--no-deps", "--format-version", "1"])
        .output()
        .context("running cargo metadata in the upstream checkout")?;
    if !output.status.success() {
        bail!(
            "cargo metadata failed in {}: {}",
            root.display(),
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    let metadata: Metadata =
        serde_json::from_slice(&output.stdout).context("parsing upstream cargo metadata")?;

    let mut packages = Vec::new();
    for package in metadata.packages {
        let manifest_dir = package
            .manifest_path
            .parent()
            .context("manifest path has no parent")?;
        let dir = manifest_dir
            .strip_prefix(root)
            .with_context(|| format!("package {} is outside the upstream root", package.name))?;
        let dir = dir
            .to_str()
            .with_context(|| format!("package {} has a non-UTF-8 path", package.name))?
            .replace('\\', "/");
        let dependencies = package
            .dependencies
            .into_iter()
            .filter(|dependency| dependency.kind.as_deref() != Some("dev"))
            .map(|dependency| dependency.name)
            .collect();
        packages.push(Package {
            name: package.name,
            dir,
            dependencies,
        });
    }
    packages.sort_by(|left, right| left.dir.cmp(&right.dir));
    Ok(packages)
}
