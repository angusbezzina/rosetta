//! The tree-sitter query `rule.glossary-terms` runs, generated from every
//! settled term's Avoid words so the gate and the glossary cannot drift.
//!
//! The regexes are Rust `regex` syntax (Whetstone's ast enforcer), written
//! inside a tree-sitter string literal, so each backslash and quote is escaped
//! once more for the query.

use super::model::{Glossary, TermStatus};

/// The node kinds that hold copy a person sees in a TSX file.
const COPY_NODES: &str = "[(jsx_text) (string_fragment)]";

/// A string with a slash and no space is a module path or a URL, never copy:
/// `import { Popover } from "./Overlays.tsx"` names a file, not a thing.
const NOT_COPY: &str = r"^\S*/\S*$";

fn query_string(regex: &str) -> String {
    regex.replace('\\', "\\\\").replace('"', "\\\"")
}

/// Builds the query, or `None` when no settled term avoids a word.
pub fn glossary_query(glossary: &Glossary) -> Option<String> {
    let avoided: Vec<_> = glossary
        .terms
        .iter()
        .filter(|term| term.status == TermStatus::Settled)
        .flat_map(|term| term.avoid_words())
        .collect();
    if avoided.is_empty() {
        return None;
    }

    // A plural is the same word in copy, so each word matches with an optional s.
    let words: Vec<String> = avoided
        .iter()
        .map(|entry| format!("{}s?", regex::escape(&entry.word.to_lowercase())))
        .collect();
    let matched = if words.len() == 1 {
        format!(r"(?i)\b{}\b", words.join(""))
    } else {
        format!(r"(?i)\b({})\b", words.join("|"))
    };

    let mut exemptions: Vec<String> = Vec::new();
    for entry in &avoided {
        let word = entry.word.to_lowercase();
        let suffix = format!(" {word}");
        let mut qualifiers: Vec<String> = Vec::new();
        for phrase in entry.except.as_deref().unwrap_or_default() {
            let phrase = phrase.to_lowercase();
            match phrase.strip_suffix(&suffix) {
                Some(qualifier) if !qualifier.trim().is_empty() => {
                    qualifiers.push(regex::escape(qualifier.trim()));
                }
                _ => exemptions.push(regex::escape(&phrase)),
            }
        }
        if !qualifiers.is_empty() {
            exemptions.push(format!(
                "({}) {}",
                qualifiers.join("|"),
                regex::escape(&word)
            ));
        }
    }

    let mut query = format!(
        "({COPY_NODES} @match (#match? @match \"{}\")",
        query_string(&matched)
    );
    if !exemptions.is_empty() {
        query.push_str(&format!(
            " (#not-match? @match \"{}\")",
            query_string(&format!("(?i){}", exemptions.join("|")))
        ));
    }
    query.push_str(&format!(
        " (#not-match? @match \"{}\")",
        query_string(NOT_COPY)
    ));
    query.push(')');
    Some(query)
}
