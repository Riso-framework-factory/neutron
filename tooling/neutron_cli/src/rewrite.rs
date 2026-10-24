//! Fixes relative paths in files that move to a different place than the files they point at.
//!
//! Upstream code reaches outside its own crate with string literals such as
//! `include_bytes!("../../../assets/fonts/x.ttf")`, `crate_relative = "../../assets"` or
//! `root_relative = "crates/grammars/src"`. A literal is rewritten only when it resolves to a
//! tracked upstream path whose Neutron location moved differently from the referring file.

use crate::config::{LiteralBase, LiteralRewrite};
use crate::layout::{Layout, parent_path, relative_path, resolve_path, strip_path_prefix};
use crate::upstream::Upstream;
use regex::Regex;
use std::ops::Range;
use std::sync::LazyLock;

static INCLUDE_LITERAL: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"include(?:_str|_bytes)?!\s*\(\s*"((?:\.\./)+[^"\\]*)""#).expect("valid regex")
});
static RELATIVE_LITERAL: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#""((?:\.\./)+[^"\\]*)""#).expect("valid regex"));
static ROOT_RELATIVE_LITERAL: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"root_relative\s*=\s*"([^"\\]+)""#).expect("valid regex"));

#[derive(Debug, Clone)]
pub struct Rewrite {
    pub file: String,
    pub from: String,
    pub to: String,
}

pub struct PathFixer<'a> {
    upstream: &'a Upstream,
    layout: &'a Layout,
    explicit: &'a [LiteralRewrite],
}

struct Candidate {
    range: Range<usize>,
    base: LiteralBase,
    forced: bool,
}

impl<'a> PathFixer<'a> {
    pub fn new(upstream: &'a Upstream, layout: &'a Layout, explicit: &'a [LiteralRewrite]) -> Self {
        Self {
            upstream,
            layout,
            explicit,
        }
    }

    pub fn applies_to(path: &str) -> bool {
        path.ends_with(".rs") || path.ends_with(".toml")
    }

    /// Rewrites the literals of the upstream file `file` for its Neutron location.
    pub fn fix_source(&self, file: &str, content: &str) -> (String, Vec<Rewrite>) {
        let mut candidates = Vec::new();
        for captures in INCLUDE_LITERAL.captures_iter(content) {
            if let Some(literal) = captures.get(1) {
                candidates.push(Candidate {
                    range: literal.range(),
                    base: LiteralBase::File,
                    forced: false,
                });
            }
        }
        for captures in RELATIVE_LITERAL.captures_iter(content) {
            if let Some(literal) = captures.get(1)
                && !candidates
                    .iter()
                    .any(|candidate| candidate.range == literal.range())
            {
                candidates.push(Candidate {
                    range: literal.range(),
                    base: LiteralBase::Crate,
                    forced: false,
                });
            }
        }
        for captures in ROOT_RELATIVE_LITERAL.captures_iter(content) {
            if let Some(literal) = captures.get(1) {
                candidates.push(Candidate {
                    range: literal.range(),
                    base: LiteralBase::Root,
                    forced: false,
                });
            }
        }
        for explicit in self
            .explicit
            .iter()
            .filter(|explicit| explicit.file == file)
        {
            // Matched anywhere, so a rule can target part of a string such as
            // "cargo:rerun-if-changed=../../.git/logs/HEAD".
            for (start, _) in content.match_indices(explicit.literal.as_str()) {
                candidates.push(Candidate {
                    range: start..start + explicit.literal.len(),
                    base: explicit.base,
                    forced: true,
                });
            }
        }
        candidates.sort_by_key(|candidate| (candidate.range.start, !candidate.forced));
        candidates.dedup_by_key(|candidate| candidate.range.start);

        let mut output = String::with_capacity(content.len());
        let mut rewrites = Vec::new();
        let mut copied_up_to = 0;
        for candidate in candidates {
            if candidate.range.start < copied_up_to {
                continue;
            }
            let literal = &content[candidate.range.clone()];
            if let Some(replacement) =
                self.remap_literal(file, literal, candidate.base, candidate.forced)
            {
                output.push_str(&content[copied_up_to..candidate.range.start]);
                output.push_str(&replacement);
                copied_up_to = candidate.range.end;
                rewrites.push(Rewrite {
                    file: file.to_string(),
                    from: literal.to_string(),
                    to: replacement,
                });
            }
        }
        output.push_str(&content[copied_up_to..]);
        (output, rewrites)
    }

    /// The new target of the upstream symlink `link`, if it has to change.
    pub fn fix_symlink(&self, link: &str, target: &str) -> Option<String> {
        let directory = parent_path(link);
        let resolved = resolve_path(directory, target)?;
        if !self.upstream.exists(&resolved) {
            return None;
        }
        let new_directory = self.layout.map(directory)?.path;
        let new_target = self.layout.map(&resolved)?.path;
        if resolve_path(&new_directory, target).as_deref() == Some(new_target.as_str()) {
            return None;
        }
        Some(relative_path(&new_directory, &new_target))
    }

    fn remap_literal(
        &self,
        file: &str,
        literal: &str,
        base: LiteralBase,
        forced: bool,
    ) -> Option<String> {
        let base_directory = match base {
            LiteralBase::Root => "",
            LiteralBase::File => parent_path(file),
            LiteralBase::Crate => self
                .upstream
                .package_containing(file)
                .map_or_else(|| parent_path(file), |package| package.dir.as_str()),
        };
        let target = resolve_path(base_directory, literal)?;
        if !forced {
            if !self.upstream.exists(&target) {
                return None;
            }
            // Relative literals like "../.." name an ancestor of the file and are almost always
            // test data. Root-relative literals legitimately name the file's own crate.
            if target.is_empty()
                || (base != LiteralBase::Root && strip_path_prefix(file, &target).is_some())
            {
                return None;
            }
        }

        let new_target = self.layout.map(&target)?.path;
        let new_base = match base {
            LiteralBase::Root => String::new(),
            LiteralBase::File | LiteralBase::Crate => self.layout.map(base_directory)?.path,
        };
        // Keep the literal when it still reaches the right place from its new location.
        if resolve_path(&new_base, literal).as_deref() == Some(new_target.as_str()) {
            return None;
        }
        let mut replacement = match base {
            LiteralBase::Root => new_target,
            LiteralBase::File | LiteralBase::Crate => relative_path(&new_base, &new_target),
        };
        if literal.ends_with('/') && !replacement.ends_with('/') {
            replacement.push('/');
        }
        (replacement != literal).then_some(replacement)
    }
}
