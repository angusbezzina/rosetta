//! Evidence-bounded copy lint. Syntax trees select copy; settled vocabulary
//! supplies the rule. Finding offsets always refer to the original source.

use std::collections::BTreeSet;
use std::path::{Component, Path};
use std::process::Command;

use rayon::prelude::*;
use regex::Regex;
use serde::Serialize;
use tree_sitter::{Node, Parser};

use crate::model::{Glossary, TermStatus};
use rosetta_core::config::{Config, ConfigError};

#[derive(Debug, thiserror::Error)]
pub enum LintError {
    #[error("{0}: {1}")]
    Io(String, #[source] std::io::Error),
    #[error("{0}: path must be a file inside the repository")]
    UnsafePath(String),
    #[error("{0}: invalid UTF-8 source")]
    Utf8(String),
    #[error("git {0}: {1}")]
    Git(String, String),
    #[error("parser for {0} could not be initialized")]
    Parser(String),
    #[error(transparent)]
    Config(#[from] ConfigError),
    #[error("avoid word {0:?}: {1}")]
    Pattern(String, #[source] regex::Error),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Finding {
    pub path: String,
    pub line: usize,
    pub column: usize,
    pub term_id: String,
    pub term: String,
    pub avoided: String,
    pub message: String,
}

struct Rule {
    term_id: String,
    term: String,
    avoided: String,
    contexts: Vec<String>,
    matched: Regex,
    except: Vec<Regex>,
}

pub struct Linter {
    rules: Vec<Rule>,
}

impl Linter {
    pub fn new(glossary: &Glossary) -> Result<Self, LintError> {
        let mut rules = Vec::new();
        for term in glossary
            .terms
            .iter()
            .filter(|term| term.status == TermStatus::Settled)
        {
            for avoid in term.avoid_words() {
                let matched = Regex::new(&format!(r"(?i)\b{}s?\b", regex::escape(&avoid.word)))
                    .map_err(|error| LintError::Pattern(avoid.word.clone(), error))?;
                let except = avoid
                    .except
                    .as_deref()
                    .unwrap_or_default()
                    .iter()
                    .map(|phrase| {
                        // An exception naming "Notion workspace" also covers
                        // "Notion workspaces", just as the Avoid rule does.
                        let phrase_words = crate::names::split_words(phrase);
                        let avoided_words = crate::names::split_words(&avoid.word);
                        let plural =
                            !avoided_words.is_empty() && phrase_words.ends_with(&avoided_words);
                        Regex::new(&format!(
                            r"(?i)\b{}{}\b",
                            regex::escape(phrase),
                            if plural { "s?" } else { "" }
                        ))
                        .map_err(|error| LintError::Pattern(phrase.clone(), error))
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                rules.push(Rule {
                    term_id: term.id.clone(),
                    term: term.name.clone(),
                    avoided: avoid.word.clone(),
                    contexts: term.contexts.clone(),
                    matched,
                    except,
                });
            }
        }
        Ok(Self { rules })
    }

    pub fn lint_text(
        &self,
        path: &str,
        text: &str,
        contexts: &BTreeSet<String>,
    ) -> Result<Vec<Finding>, LintError> {
        let ranges = copy_ranges(path, text)?;
        let lines = std::iter::once(0)
            .chain(
                text.bytes()
                    .enumerate()
                    .filter_map(|(at, byte)| (byte == b'\n').then_some(at + 1)),
            )
            .collect::<Vec<_>>();
        let mut findings = Vec::new();
        for range in ranges {
            let copy = &text[range.clone()];
            // A module path or URL by itself is never visible prose.
            if !copy.chars().any(char::is_whitespace) && copy.contains('/') {
                continue;
            }
            for rule in &self.rules {
                if !rule.contexts.is_empty()
                    && !rule
                        .contexts
                        .iter()
                        .any(|context| contexts.contains(context))
                {
                    continue;
                }
                let exemptions = rule
                    .except
                    .iter()
                    .flat_map(|pattern| pattern.find_iter(copy).map(|hit| hit.range()))
                    .collect::<Vec<_>>();
                for hit in rule.matched.find_iter(copy) {
                    if exemptions.iter().any(|exemption| {
                        exemption.start <= hit.start() && exemption.end >= hit.end()
                    }) {
                        continue;
                    }
                    let at = range.start + hit.start();
                    let line_index = lines
                        .partition_point(|start| *start <= at)
                        .saturating_sub(1);
                    let line_start = lines[line_index];
                    findings.push(Finding {
                        path: path.to_owned(),
                        line: line_index + 1,
                        column: text[line_start..at].chars().count() + 1,
                        term_id: rule.term_id.clone(),
                        term: rule.term.clone(),
                        avoided: rule.avoided.clone(),
                        message: format!("Use {} instead of {} in copy", rule.term, rule.avoided),
                    });
                }
            }
        }
        findings.sort_by(|left, right| {
            (&left.path, left.line, left.column, &left.term_id).cmp(&(
                &right.path,
                right.line,
                right.column,
                &right.term_id,
            ))
        });
        Ok(findings)
    }

    pub fn lint_files(
        &self,
        root: &Path,
        config: &Config,
        paths: &[String],
    ) -> Result<Vec<Finding>, LintError> {
        let root = root
            .canonicalize()
            .map_err(|error| LintError::Io(root.display().to_string(), error))?;
        let mut batches = paths
            .par_iter()
            .filter(|path| supported(path))
            .map(|path| {
                let relative = Path::new(path);
                if !relative
                    .components()
                    .all(|component| matches!(component, Component::Normal(_)))
                {
                    return Err(LintError::UnsafePath(path.clone()));
                }
                let absolute = root
                    .join(relative)
                    .canonicalize()
                    .map_err(|error| LintError::Io(path.clone(), error))?;
                if !absolute.starts_with(&root) || !absolute.is_file() {
                    return Err(LintError::UnsafePath(path.clone()));
                }
                let bytes =
                    std::fs::read(&absolute).map_err(|error| LintError::Io(path.clone(), error))?;
                let text = String::from_utf8(bytes).map_err(|_| LintError::Utf8(path.clone()))?;
                let contexts = config.contexts_for_path(path)?;
                self.lint_text(path, &text, &contexts)
            })
            .collect::<Result<Vec<_>, _>>()?;
        let mut findings = batches.drain(..).flatten().collect::<Vec<_>>();
        findings.sort_by(|left, right| {
            (&left.path, left.line, left.column, &left.term_id).cmp(&(
                &right.path,
                right.line,
                right.column,
                &right.term_id,
            ))
        });
        Ok(findings)
    }
}

fn supported(path: &str) -> bool {
    [".tsx", ".jsx", ".md", ".rs", ".py"]
        .iter()
        .any(|suffix| path.ends_with(suffix))
}

fn git(root: &Path, arguments: &[&str]) -> Result<Vec<u8>, LintError> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(arguments)
        .output()
        .map_err(|error| LintError::Io("git".into(), error))?;
    if !output.status.success() {
        return Err(LintError::Git(
            arguments.join(" "),
            String::from_utf8_lossy(&output.stderr).trim().to_owned(),
        ));
    }
    Ok(output.stdout)
}

/// Tracked copy files, or tracked files changed since the merge-base with ref.
pub fn selected_files(
    root: &Path,
    base: Option<&str>,
    config: &Config,
) -> Result<Vec<String>, LintError> {
    let paths = tracked_files(root, base)?;
    let include = config
        .lint
        .include
        .iter()
        .map(|pattern| {
            glob::Pattern::new(pattern)
                .map_err(|error| LintError::Git("lint.include".into(), error.to_string()))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let exclude = config
        .lint
        .exclude
        .iter()
        .map(|pattern| {
            glob::Pattern::new(pattern)
                .map_err(|error| LintError::Git("lint.exclude".into(), error.to_string()))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let mut paths = paths
        .into_iter()
        .filter(|path| {
            supported(path)
                && path != "GLOSSARY.md"
                && path != "ATLAS.md"
                && !path.starts_with(".rosetta/")
                && root.join(path).exists()
        })
        .filter(|path| {
            (include.is_empty() || include.iter().any(|pattern| pattern.matches(path)))
                && !exclude.iter().any(|pattern| pattern.matches(path))
        })
        .collect::<Vec<_>>();
    paths.sort();
    paths.dedup();
    Ok(paths)
}

/// All tracked paths, or paths changed since the merge base. Copy and naming
/// checks use the same changed-file boundary.
pub(crate) fn tracked_files(root: &Path, base: Option<&str>) -> Result<Vec<String>, LintError> {
    let output = if let Some(base) = base {
        // --merge-base compares the merge base to the working tree in one
        // process, including staged and unstaged changes.
        git(
            root,
            &["diff", "--merge-base", "--name-only", "-z", base, "--"],
        )?
    } else {
        git(root, &["ls-files", "-z"])?
    };
    let mut paths = output
        .split(|byte| *byte == 0)
        .filter(|part| !part.is_empty())
        .filter_map(|part| String::from_utf8(part.to_vec()).ok())
        .collect::<Vec<_>>();
    paths.sort();
    paths.dedup();
    Ok(paths)
}

fn copy_ranges(path: &str, text: &str) -> Result<Vec<std::ops::Range<usize>>, LintError> {
    if path.ends_with(".md") {
        return Ok(markdown_ranges(text));
    }
    if ![".tsx", ".jsx", ".rs", ".py"]
        .iter()
        .any(|suffix| path.ends_with(suffix))
    {
        return Ok(std::iter::once(0..text.len()).collect());
    }
    let language = if path.ends_with(".tsx") || path.ends_with(".jsx") {
        tree_sitter_typescript::language_tsx()
    } else if path.ends_with(".rs") {
        tree_sitter_rust::language()
    } else {
        tree_sitter_python::language()
    };
    let mut parser = Parser::new();
    parser
        .set_language(&language)
        .map_err(|_| LintError::Parser(path.to_owned()))?;
    let tree = parser
        .parse(text, None)
        .ok_or_else(|| LintError::Parser(path.to_owned()))?;
    let mut ranges = Vec::new();
    visit(tree.root_node(), path, text, &mut ranges);
    Ok(ranges)
}

fn visit(node: Node<'_>, path: &str, text: &str, ranges: &mut Vec<std::ops::Range<usize>>) {
    let kind = node.kind();
    if kind == "jsx_attribute" {
        let name = node
            .named_child(0)
            .and_then(|name| name.utf8_text(text.as_bytes()).ok());
        if name.is_some_and(|name| {
            matches!(
                name,
                "id" | "htmlFor"
                    | "className"
                    | "key"
                    | "href"
                    | "src"
                    | "type"
                    | "role"
                    | "name"
                    | "value"
                    | "testID"
            ) || name.starts_with("data-")
                || name.starts_with("on")
        }) {
            return;
        }
    }
    // Comparison values such as `state === "recap"` are internal enum-like
    // identifiers, even when they occur in TSX files.
    if kind == "binary_expression"
        && node
            .child_by_field_name("operator")
            .is_some_and(|operator| matches!(operator.kind(), "==" | "===" | "!=" | "!=="))
    {
        return;
    }
    let copy = if path.ends_with(".tsx") || path.ends_with(".jsx") {
        kind == "jsx_text" || kind == "string_fragment"
    } else if path.ends_with(".rs") {
        kind == "string_literal" || kind == "raw_string_literal"
    } else {
        kind == "string" || kind == "string_content"
    };
    if copy {
        ranges.push(node.byte_range());
        return;
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        visit(child, path, text, ranges);
    }
}

fn markdown_ranges(text: &str) -> Vec<std::ops::Range<usize>> {
    let mut ranges = Vec::new();
    let mut fence: Option<(u8, usize)> = None;
    let mut offset = 0;
    for line in text.split_inclusive('\n') {
        let trimmed = line.trim_start();
        let marker = trimmed.as_bytes().first().copied();
        let count = marker.map_or(0, |byte| {
            trimmed
                .as_bytes()
                .iter()
                .take_while(|current| **current == byte)
                .count()
        });
        if let Some((byte, width)) = fence {
            if marker == Some(byte) && count >= width {
                fence = None;
            }
        } else if matches!(marker, Some(b'`' | b'~')) && count >= 3 {
            fence = marker.map(|byte| (byte, count));
        } else {
            let mut masked = line.as_bytes().to_vec();
            let mut at = 0;
            while at < masked.len() {
                if masked[at] == b'`' {
                    let width = masked[at..]
                        .iter()
                        .take_while(|byte| **byte == b'`')
                        .count();
                    let close = masked[at + width..]
                        .windows(width)
                        .position(|part| part.iter().all(|byte| *byte == b'`'));
                    let end = close.map_or(masked.len(), |found| at + width + found + width);
                    masked[at..end].fill(0);
                    at = end;
                } else if masked[at] == b']' && masked.get(at + 1) == Some(&b'(') {
                    let end = masked[at + 2..]
                        .iter()
                        .position(|byte| *byte == b')')
                        .map_or(masked.len(), |found| at + 3 + found);
                    masked[at..end].fill(0);
                    at = end;
                } else if masked[at] == b'<' {
                    if let Some(end) = masked[at..].iter().position(|byte| *byte == b'>') {
                        masked[at..at + end + 1].fill(0);
                        at += end + 1;
                    } else {
                        at += 1;
                    }
                } else {
                    at += 1;
                }
            }
            let mut start = None;
            for (index, byte) in masked.iter().enumerate() {
                if *byte != 0 && start.is_none() {
                    start = Some(index);
                }
                if *byte == 0
                    && let Some(from) = start.take()
                {
                    ranges.push(offset + from..offset + index);
                }
            }
            if let Some(from) = start {
                ranges.push(offset + from..offset + masked.len());
            }
        }
        offset += line.len();
    }
    ranges
}

#[cfg(test)]
mod tests {
    use super::{Linter, selected_files};
    use crate::model::Glossary;
    use rosetta_core::config::Config;
    use std::collections::BTreeSet;

    fn linter() -> Result<Linter, Box<dyn std::error::Error>> {
        let glossary: Glossary = serde_json::from_value(serde_json::json!({
            "schema": "rosetta.glossary/v1", "context": "fixture", "purpose": "fixture",
            "sections": [{"id": "terms", "title": "Terms"}],
            "terms": [
                {"id": "term.group", "name": "Group", "section": "terms", "definition": "A shared place.", "status": "settled", "avoid": [{"word": "workspace", "except": ["Linear workspace", "Notion workspace"]}]},
                {"id": "term.task", "name": "Task", "section": "terms", "definition": "A tracked job.", "status": "settled", "avoid": [{"word": "bead", "except": ["import-beads"]}]},
                {"id": "term.provider", "name": "AI provider", "section": "terms", "definition": "A model connection.", "status": "settled", "avoid": [{"word": "LLM profile"}]},
                {"id": "term.pay", "name": "Payment", "section": "terms", "definition": "A transfer.", "status": "settled", "contexts": ["payments"], "avoid": [{"word": "charge"}]},
                {"id": "term.summary", "name": "Summary", "section": "terms", "definition": "A report.", "status": "proposed", "avoid": [{"word": "recap"}]}
            ]
        }))?;
        Ok(Linter::new(&glossary)?)
    }

    #[test]
    fn syntax_copy_and_exemptions_keep_precise_locations() -> Result<(), Box<dyn std::error::Error>>
    {
        let lint = linter()?;
        let empty = BTreeSet::new();
        let tsx = "import x from './workspace/view';\n<div>workspace workspaces Linear workspace Notion workspaces bead import-beads recap</div>";
        let found = lint.lint_text("view.tsx", tsx, &empty)?;
        assert_eq!(
            found
                .iter()
                .map(|item| (item.term_id.as_str(), item.line))
                .collect::<Vec<_>>(),
            vec![("term.group", 2), ("term.group", 2), ("term.task", 2)]
        );
        assert_eq!(found[0].column, 6);
        assert!(
            lint.lint_text(
                "view.tsx",
                "const state = kind === \"recap\";\n<div>No recap yet.</div>",
                &empty
            )?
            .iter()
            .all(|item| item.line == 2)
        );
        let attributes = "<label htmlFor=\"workspace-mail\" id=\"workspace-mail\" aria-label=\"Workspace mail\">Workspace mail</label>";
        assert_eq!(lint.lint_text("view.tsx", attributes, &empty)?.len(), 2);
        assert_eq!(
            lint.lint_text(
                "view.rs",
                "let copy = \"workspace\"; let path = \"./workspace/file\";",
                &empty
            )?
            .len(),
            1
        );
        assert_eq!(
            lint.lint_text(
                "view.py",
                "message = '''workspace and workspaces'''\npath = './workspace/file'",
                &empty
            )?
            .len(),
            2
        );
        Ok(())
    }

    #[test]
    fn markdown_prose_keeps_phrases_but_skips_code_and_destinations()
    -> Result<(), Box<dyn std::error::Error>> {
        let lint = linter()?;
        let text = "An LLM profile uses a workspace. Linear workspace is exempt.\n`workspace` [Link](https://example.com/workspace)\n```rust\n\"workspace\"\n```\nAnother workspace.\n";
        let found = lint.lint_text("guide.md", text, &BTreeSet::new())?;
        assert_eq!(
            found
                .iter()
                .map(|item| (item.term_id.as_str(), item.line))
                .collect::<Vec<_>>(),
            vec![("term.provider", 1), ("term.group", 1), ("term.group", 6)]
        );
        Ok(())
    }

    #[test]
    fn scoped_terms_need_their_path_context() -> Result<(), Box<dyn std::error::Error>> {
        let lint = linter()?;
        assert!(
            lint.lint_text("other.md", "A charge.", &BTreeSet::new())?
                .is_empty()
        );
        assert_eq!(
            lint.lint_text(
                "billing.md",
                "A charge.",
                &BTreeSet::from(["payments".to_owned()])
            )?
            .len(),
            1
        );
        Ok(())
    }

    #[test]
    fn changed_only_uses_merge_base_and_excludes_generated_documents()
    -> Result<(), Box<dyn std::error::Error>> {
        let temp = tempfile::tempdir()?;
        let git = |args: &[&str]| -> Result<(), Box<dyn std::error::Error>> {
            let status = std::process::Command::new("git")
                .arg("-C")
                .arg(temp.path())
                .args(args)
                .status()?;
            assert!(status.success());
            Ok(())
        };
        git(&["init", "-q"])?;
        std::fs::write(temp.path().join("view.tsx"), "<p>Group</p>")?;
        std::fs::write(temp.path().join("GLOSSARY.md"), "workspace")?;
        git(&["add", "."])?;
        git(&[
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.invalid",
            "commit",
            "-qm",
            "base",
        ])?;
        git(&["branch", "base"])?;
        std::fs::write(temp.path().join("view.tsx"), "<p>workspace</p>")?;
        assert_eq!(
            selected_files(temp.path(), Some("base"), &Config::default())?,
            ["view.tsx"]
        );
        assert_eq!(
            selected_files(temp.path(), None, &Config::default())?,
            ["view.tsx"]
        );
        std::fs::remove_file(temp.path().join("view.tsx"))?;
        assert!(selected_files(temp.path(), Some("base"), &Config::default())?.is_empty());
        Ok(())
    }
}
