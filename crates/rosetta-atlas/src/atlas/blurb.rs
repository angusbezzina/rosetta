//! What a file or directory does, in its author's own first sentence: a
//! Rust module's `//!` doc, a script's leading doc comment, a Markdown
//! file's first paragraph, a manifest's `description`. Read whenever a
//! bundle is built, for any repository, so a part says what it is without
//! anyone curating it.

use std::path::Path;

/// A first sentence longer than this is cut at its first clause.
const MAX_CHARS: usize = 140;
/// Only a file's opening is read; a doc comment sits at its top.
const HEAD_BYTES: usize = 16 * 1024;

/// The first sentence of what `path` says about itself, from its text.
pub fn of_file(path: &str, text: &str) -> Option<String> {
    let name = path.rsplit('/').next().unwrap_or(path);
    let extension = name.rsplit_once('.').map(|(_, ext)| ext).unwrap_or("");
    let prose = match (name, extension) {
        ("Cargo.toml", _) => toml_description(text),
        ("package.json", _) => json_description(text),
        (_, "rs") => rust_doc(text),
        (_, "ts" | "tsx" | "js" | "jsx" | "mjs" | "cjs" | "swift" | "kt" | "go" | "proto") => {
            slash_doc(text)
        }
        (_, "md" | "mdx") => markdown_paragraph(text),
        (_, "py" | "sh" | "bash" | "zsh" | "yml" | "yaml" | "toml") => hash_doc(text),
        _ => None,
    }?;
    first_sentence(&prose)
}

/// What a directory says about itself: its README, else its manifest, else
/// the module file that roots it.
pub fn of_dir(root: &Path, dir: &str) -> Option<String> {
    const ROOTS: [&str; 10] = [
        "README.md",
        "Cargo.toml",
        "package.json",
        "src/lib.rs",
        "src/main.rs",
        "lib.rs",
        "mod.rs",
        "main.rs",
        "index.ts",
        "src/index.ts",
    ];
    ROOTS.iter().find_map(|file| {
        let path = format!("{dir}/{file}");
        of_file(&path, &read_head(&root.join(&path))?)
    })
}

/// A file's or directory's blurb, read from disk.
pub fn of_path(root: &Path, path: &str) -> Option<String> {
    let full = root.join(path);
    if full.is_dir() {
        of_dir(root, path)
    } else {
        of_file(path, &read_head(&full)?)
    }
}

fn read_head(path: &Path) -> Option<String> {
    use std::io::Read;
    let mut bytes = Vec::with_capacity(HEAD_BYTES);
    std::fs::File::open(path)
        .ok()?
        .take(HEAD_BYTES as u64)
        .read_to_end(&mut bytes)
        .ok()?;
    if bytes.contains(&0) {
        return None;
    }
    Some(String::from_utf8_lossy(&bytes).into_owned())
}

/// The `//!` lines that open a Rust file, after its attributes.
fn rust_doc(text: &str) -> Option<String> {
    let mut lines = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if let Some(doc) = line.strip_prefix("//!") {
            lines.push(doc.trim());
        } else if lines.is_empty() && (line.is_empty() || line.starts_with("#![")) {
            continue;
        } else {
            break;
        }
    }
    paragraph(&lines)
}

/// The block or line comment that opens a C-family file, before any code;
/// a licence header, a directive or a `"use …"` line is passed over.
fn slash_doc(text: &str) -> Option<String> {
    let mut lines = Vec::new();
    let mut in_block = false;
    for line in text.lines() {
        let line = line.trim();
        if in_block {
            let (body, closed) = match line.split_once("*/") {
                Some((body, _)) => (body, true),
                None => (line, false),
            };
            lines.push(body.trim_start_matches('*').trim());
            if closed {
                if let Some(found) = paragraph(&lines).filter(|p| !is_boilerplate(p)) {
                    return Some(found);
                }
                lines.clear();
                in_block = false;
            }
        } else if let Some(rest) = line.strip_prefix("/*") {
            let rest = rest.trim_start_matches('*');
            match rest.split_once("*/") {
                Some((body, _)) => {
                    if let Some(found) = paragraph(&[body.trim()]).filter(|p| !is_boilerplate(p)) {
                        return Some(found);
                    }
                }
                None => {
                    in_block = true;
                    lines.push(rest.trim());
                }
            }
        } else if let Some(doc) = line.strip_prefix("//") {
            let doc = doc.trim_start_matches('/').trim();
            if !is_boilerplate(doc) {
                lines.push(doc);
            }
        } else if line.is_empty() || line.starts_with("#!") || line.starts_with("\"use ") {
            if !lines.is_empty() {
                break;
            }
        } else {
            break;
        }
    }
    paragraph(&lines)
}

/// The `#` lines that open a script or config, after a shebang.
fn hash_doc(text: &str) -> Option<String> {
    let mut lines = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with("#!") || (lines.is_empty() && (line.is_empty() || line == "---")) {
            continue;
        }
        match line.strip_prefix('#') {
            Some(doc) if !doc.starts_with('[') => lines.push(doc.trim()),
            _ => break,
        }
    }
    paragraph(&lines).filter(|p| !is_boilerplate(p))
}

/// A Markdown file's first paragraph of prose, past its headings, badges,
/// front matter and HTML.
fn markdown_paragraph(text: &str) -> Option<String> {
    let mut lines = Vec::new();
    let mut front_matter = false;
    let mut fenced = false;
    for (index, raw) in text.lines().enumerate() {
        let line = raw.trim();
        if index == 0 && line == "---" {
            front_matter = true;
            continue;
        }
        if front_matter {
            front_matter = line != "---";
            continue;
        }
        if line.starts_with("```") {
            fenced = !fenced;
            continue;
        }
        let skip = fenced
            || line.starts_with('#')
            || line.starts_with('<')
            || line.starts_with("[![")
            || line.starts_with("![")
            || line.starts_with('|')
            || line.starts_with('>')
            || line.starts_with("- ")
            || line.starts_with("* ");
        if line.is_empty() || skip {
            if !lines.is_empty() {
                break;
            }
            continue;
        }
        lines.push(line);
    }
    paragraph(&lines).map(|p| plain_markdown(&p))
}

fn toml_description(text: &str) -> Option<String> {
    #[derive(serde::Deserialize)]
    struct Manifest {
        package: Option<Package>,
    }
    #[derive(serde::Deserialize)]
    struct Package {
        description: Option<String>,
    }
    toml::from_str::<Manifest>(text).ok()?.package?.description
}

fn json_description(text: &str) -> Option<String> {
    #[derive(serde::Deserialize)]
    struct Manifest {
        description: Option<String>,
    }
    serde_json::from_str::<Manifest>(text).ok()?.description
}

fn paragraph(lines: &[&str]) -> Option<String> {
    // A blank doc line ends the opening paragraph.
    let first: Vec<&str> = lines
        .iter()
        .copied()
        .skip_while(|line| line.is_empty())
        .take_while(|line| !line.is_empty())
        .collect();
    let joined = first.join(" ");
    let joined = joined.trim();
    (!joined.is_empty()).then(|| joined.to_owned())
}

/// Licences, lint and tool directives, and generated-file banners say
/// nothing about what a file does.
fn is_boilerplate(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    [
        "copyright",
        "license",
        "spdx-",
        "eslint",
        "biome-ignore",
        "@ts-",
        "prettier",
        "generated",
        "do not edit",
        "@generated",
        "tslint",
        "jshint",
        "deno-lint",
        "@vitest",
        "@jest",
    ]
    .iter()
    .any(|marker| lower.contains(marker))
}

/// `[text](url)` as its text, and emphasis marks dropped; backticks stay,
/// since the viewer sets them as code.
fn plain_markdown(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(open) = rest.find('[') {
        let Some(close) = rest[open..].find("](").map(|i| open + i) else {
            break;
        };
        let Some(end) = rest[close..].find(')').map(|i| close + i) else {
            break;
        };
        out.push_str(&rest[..open]);
        out.push_str(&rest[open + 1..close]);
        rest = &rest[end + 1..];
    }
    out.push_str(rest);
    out.replace("**", "").replace("__", "")
}

/// The text up to its first full stop; past [`MAX_CHARS`], up to its first
/// clause break instead, so a blurb stays a few words.
fn first_sentence(text: &str) -> Option<String> {
    let text = text.trim();
    let mut end = text.len();
    let bytes = text.as_bytes();
    for (index, c) in text.char_indices() {
        let next = bytes.get(index + 1).copied();
        if matches!(c, '.' | '?' | '!') && next.is_none_or(|b| b == b' ') {
            end = index + 1;
            break;
        }
    }
    let mut sentence = text[..end].trim().to_owned();
    if sentence.chars().count() > MAX_CHARS {
        let clause = [": ", "; ", ", ", " (", " - "]
            .iter()
            .filter_map(|mark| sentence.find(mark))
            .filter(|at| *at >= 20)
            .min();
        if let Some(at) = clause {
            sentence.truncate(at);
            sentence.push('.');
        }
    }
    let sentence = sentence.trim_end_matches(':').trim().to_owned();
    (!sentence.is_empty()).then_some(sentence)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_rust_module_says_its_first_sentence() {
        let text = "#![allow(x)]\n//! The files below each member, read live.\n//! They change on every PR.\n\nuse a;\n";
        assert_eq!(
            of_file("c/src/walk.rs", text).as_deref(),
            Some("The files below each member, read live.")
        );
    }

    #[test]
    fn a_script_skips_its_licence_and_reads_its_doc() {
        let text = "// Copyright 2026 Naos\n\n/**\n * A repository's atlas in its channel.\n * More detail.\n */\nimport x from \"y\";\n";
        assert_eq!(
            of_file("a/AtlasPane.tsx", text).as_deref(),
            Some("A repository's atlas in its channel.")
        );
    }

    #[test]
    fn code_says_nothing_without_a_leading_comment() {
        assert_eq!(of_file("a/b.ts", "import x from \"y\";\n// later\n"), None);
        assert_eq!(of_file("a/b.rs", "use a;\n//! too late\n"), None);
    }

    #[test]
    fn markdown_takes_its_first_paragraph_as_plain_text() {
        let text = "# Title\n\n[![ci](b)](c)\n\nThe **merge** agent: it lands [pull requests](x) on `main`.\nSecond line.\n";
        assert_eq!(
            of_file("tools/cato/README.md", text).as_deref(),
            Some("The merge agent: it lands pull requests on `main`.")
        );
    }

    #[test]
    fn manifests_say_their_description() {
        assert_eq!(
            of_file(
                "c/Cargo.toml",
                "[package]\nname = \"c\"\ndescription = \"Reads things.\"\n"
            )
            .as_deref(),
            Some("Reads things.")
        );
        assert_eq!(
            of_file(
                "p/package.json",
                "{\"name\":\"p\",\"description\":\"Draws things\"}"
            )
            .as_deref(),
            Some("Draws things")
        );
    }

    #[test]
    fn a_long_sentence_is_cut_at_its_first_clause() {
        let long = format!(
            "The panel beside the diagram: {} and more.",
            "words ".repeat(30)
        );
        assert_eq!(
            first_sentence(&long).as_deref(),
            Some("The panel beside the diagram.")
        );
        assert_eq!(
            first_sentence("Version 1.2 ships.").as_deref(),
            Some("Version 1.2 ships.")
        );
    }
}
