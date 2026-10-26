//! GLOSSARY.md from the glossary: the same glossary always yields the same
//! bytes.

use super::model::{AvoidWord, Glossary, GlossaryTerm, TermStatus};
use rosetta_core::text::utf16_len;

pub const GLOSSARY_SOURCE: &str = ".rosetta/glossary.json";
pub const GLOSSARY_OUTPUT: &str = "GLOSSARY.md";
pub const WRAP_COLUMNS: usize = 80;

fn generated_notice(source: &str) -> String {
    format!(
        "This file is generated from `{source}` by `task glossary:render` \
         and is not edited by hand."
    )
}

const AVOID_EXPLANATION: &str = "An `_Avoid_` line lists words used for the term that are not its name. \
     `rule.glossary-terms` flags them in shown copy; a phrase in quotes after \
     `except` is another product's own name and is allowed.";

/// Greedy word wrap; a word longer than the width keeps a line to itself.
pub fn wrap(text: &str, width: usize) -> String {
    let mut lines: Vec<String> = Vec::new();
    let mut line = String::new();
    for word in text.split_whitespace() {
        if line.is_empty() {
            line.push_str(word);
        } else if utf16_len(&line) + 1 + utf16_len(word) <= width {
            line.push(' ');
            line.push_str(word);
        } else {
            lines.push(std::mem::take(&mut line));
            line.push_str(word);
        }
    }
    if !line.is_empty() {
        lines.push(line);
    }
    lines.join("\n")
}

fn render_avoid_word(entry: &AvoidWord) -> String {
    match entry.except.as_deref() {
        None | Some([]) => entry.word.clone(),
        Some(except) => {
            let names: Vec<String> = except.iter().map(|name| format!("\"{name}\"")).collect();
            format!(
                "{} (except another product's own: {})",
                entry.word,
                names.join(", ")
            )
        }
    }
}

// The Avoid line is never wrapped, so each exempt name stays on the term's line
// where a search for the avoided word finds it.
fn render_term(term: &GlossaryTerm) -> String {
    let mut lines = vec![
        format!("**{}**:", term.name),
        wrap(&term.definition, WRAP_COLUMNS),
    ];
    if term.status == TermStatus::Proposed {
        lines.push("_Proposed_".to_owned());
    }
    let avoid = term.avoid_words();
    if !avoid.is_empty() {
        let words: Vec<String> = avoid.iter().map(render_avoid_word).collect();
        lines.push(format!("_Avoid_: {}", words.join(", ")));
    }
    lines.join("\n")
}

fn title(context: &str) -> String {
    let mut chars = context.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

/// Renders GLOSSARY.md; the same glossary always yields the same bytes.
pub fn render_glossary(glossary: &Glossary) -> String {
    render_glossary_for(glossary, GLOSSARY_SOURCE)
}

/// Renders a glossary while retaining the path used by an embedding repository.
pub fn render_glossary_for(glossary: &Glossary, source: &str) -> String {
    let mut blocks = vec![
        format!("# {}", title(&glossary.context)),
        wrap(&generated_notice(source), WRAP_COLUMNS),
        wrap(&glossary.purpose, WRAP_COLUMNS),
    ];
    if glossary.terms.is_empty() {
        blocks.push(
            "_Starter_: No domain terms have been reviewed yet. Add definitions after reviewing the repository with a person or agent."
                .to_owned(),
        );
        return format!("{}\n", blocks.join("\n\n"));
    }
    blocks.push(wrap(AVOID_EXPLANATION, WRAP_COLUMNS));
    for section in &glossary.sections {
        blocks.push(format!("## {}", section.title));
        for term in &glossary.terms {
            if term.section == section.id {
                blocks.push(render_term(term));
            }
        }
    }
    format!("{}\n", blocks.join("\n\n"))
}
