//! Ports Zed's agent rules (`.rules`) and skills (`.agents/skills`) into Neutron.

use crate::layout::Layout;
use crate::upstream::Upstream;
use anyhow::{Context as _, Result, bail};
use regex::Regex;
use std::sync::LazyLock;

pub const BEGIN_MARKER: &str = "<!-- upstream:begin";
pub const END_MARKER: &str = "<!-- upstream:end -->";

static REPOSITORY_PATH: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(^|[^/\w.\-])(\./)?((?:crates|tooling|script|extensions)/[A-Za-z0-9_.\-]+)")
        .expect("valid regex")
});

/// Replaces the generated block of Neutron's `AGENTS.md` with the configured upstream sections.
pub fn root_agents(
    existing: &str,
    upstream: &Upstream,
    layout: &Layout,
    sections: &[String],
    drop_lines_containing: &[String],
    warnings: &mut Vec<String>,
) -> Result<String> {
    let rules =
        String::from_utf8(upstream.read(".rules")?).context("upstream .rules is not UTF-8")?;
    let available = split_sections(&rules);

    let mut block = format!(
        "\n_Synced from `.rules` at zed@{}. Paths point at their Neutron locations._\n",
        upstream.short_commit()
    );
    for wanted in sections {
        match available.iter().find(|(title, _)| title == wanted) {
            Some((_, body)) => {
                let kept: String = body
                    .split_inclusive('\n')
                    .filter(|line| {
                        !drop_lines_containing
                            .iter()
                            .any(|dropped| line.contains(dropped.as_str()))
                    })
                    .collect();
                block.push('\n');
                block.push_str(&rewrite_repository_paths(kept.trim_end(), layout));
                block.push('\n');
            }
            None => warnings.push(format!(
                "section `# {wanted}` no longer exists in upstream .rules"
            )),
        }
    }
    block.push('\n');

    let Some(begin) = existing.find(BEGIN_MARKER) else {
        bail!("AGENTS.md has no `{BEGIN_MARKER} ... -->` marker");
    };
    let begin_line_end = existing[begin..]
        .find('\n')
        .map_or(existing.len(), |offset| begin + offset + 1);
    let Some(end) = existing[begin_line_end..]
        .find(END_MARKER)
        .map(|offset| begin_line_end + offset)
    else {
        bail!("AGENTS.md has no `{END_MARKER}` marker");
    };
    Ok(format!(
        "{}{}{}",
        &existing[..begin_line_end],
        block.trim_start_matches('\n'),
        &existing[end..]
    ))
}

/// Upstream skill files as `(Neutron path, content)`, with repository paths rewritten.
pub fn skill_files(
    upstream: &Upstream,
    layout: &Layout,
    skills: &[String],
    warnings: &mut Vec<String>,
) -> Result<Vec<(String, Vec<u8>)>> {
    let mut files = Vec::new();
    for skill in skills {
        let prefix = format!(".agents/skills/{skill}/");
        let mut found = false;
        for file in upstream
            .files
            .iter()
            .filter(|file| file.path.starts_with(&prefix))
        {
            found = true;
            let content = upstream.read(&file.path)?;
            let content = if file.path.ends_with(".md") {
                let text = String::from_utf8(content)
                    .with_context(|| format!("{} is not UTF-8", file.path))?;
                rewrite_repository_paths(&text, layout).into_bytes()
            } else {
                content
            };
            files.push((file.path.clone(), content));
        }
        if !found {
            warnings.push(format!(
                "skill `{skill}` no longer exists in upstream .agents/skills"
            ));
        }
    }
    Ok(files)
}

pub fn ide_notice(upstream: &Upstream) -> String {
    format!(
        "# apps/ide is generated\n\n\
         This directory mirrors `zed-industries/zed` at `{}`, written by `cargo neutron sync`.\n\
         Paths that point outside the mirror (for example into `assets/` or `core/`) are rewritten\n\
         to their Neutron locations.\n\n\
         It exists only to prove that `core/` and the shims in `riso/` still build the full Zed.\n\
         Do not edit files here and do not depend on its crates: the next sync overwrites every change.\n",
        upstream.commit
    )
}

/// Splits markdown into top-level `# ` sections, ignoring headings inside code fences.
fn split_sections(markdown: &str) -> Vec<(String, String)> {
    let mut sections: Vec<(String, String)> = Vec::new();
    let mut in_fence = false;
    for line in markdown.split_inclusive('\n') {
        if line.trim_start().starts_with("```") {
            in_fence = !in_fence;
        }
        if !in_fence && let Some(title) = line.strip_prefix("# ") {
            sections.push((title.trim().to_string(), line.to_string()));
        } else if let Some((_, body)) = sections.last_mut() {
            body.push_str(line);
        }
    }
    sections
}

fn rewrite_repository_paths(text: &str, layout: &Layout) -> String {
    REPOSITORY_PATH
        .replace_all(text, |captures: &regex::Captures| {
            let prefix = &captures[1];
            let dot = captures.get(2).map_or("", |dot| dot.as_str());
            let path = &captures[3];
            let mapped = layout
                .map(path)
                .map_or_else(|| path.to_string(), |mapped| mapped.path);
            format!("{prefix}{dot}{mapped}")
        })
        .into_owned()
}
