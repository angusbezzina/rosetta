//! The glossary's rules, checked over the file as written before it is
//! trusted as a [`Glossary`](super::Glossary), so one run names every problem
//! at the path a person edits.

use std::collections::{BTreeMap, BTreeSet};

use super::code_names::CodeNames;
use super::epic::is_epic_ref;
use super::model::{
    DEFINITION_MAX_CHARS, DEFINITION_MAX_SENTENCES, GLOSSARY_SCHEMA, Problem, TermStatus,
    VIRGIL_GLOSSARY_SCHEMA,
};
use super::source::{GlossarySource, Object, SourceRefs};
use rosetta_core::text::{is_kebab, utf16_len};

/// What the validator needs from outside the JSON, so tests can supply it.
pub struct ValidationContext<'a> {
    pub rule_ids: BTreeSet<String>,
    pub principle_ids: BTreeSet<String>,
    pub rule_source: Option<String>,
    pub principle_source: Option<String>,
    pub doc_exists: Box<dyn Fn(&str) -> bool + 'a>,
    /// When present, every `code` entry must name something that exists.
    pub code_names: Option<CodeNames>,
    /// Context names declared by path_scopes in rosetta.toml.
    pub known_contexts: BTreeSet<String>,
}

/// A sentence ends at a terminal mark followed by whitespace or the end.
pub fn count_sentences(text: &str) -> usize {
    let chars: Vec<char> = text.trim().chars().collect();
    chars
        .iter()
        .enumerate()
        .filter(|(index, c)| {
            matches!(c, '.' | '!' | '?')
                && chars.get(index + 1).is_none_or(|next| next.is_whitespace())
        })
        .count()
}

fn term_id_ok(id: &str) -> bool {
    id.strip_prefix("term.").is_some_and(is_kebab)
}

fn iso_date_ok(text: &str) -> bool {
    let bytes = text.as_bytes();
    bytes.len() == 10
        && bytes.iter().enumerate().all(|(index, byte)| match index {
            4 | 7 => *byte == b'-',
            _ => byte.is_ascii_digit(),
        })
}

/// Checks the file as written. Returns every problem rather than the first, so
/// one run names all that needs fixing.
pub fn validate_glossary(input: &GlossarySource, context: &ValidationContext<'_>) -> Vec<Problem> {
    let mut problems = Vec::new();
    let mut fail = |path: String, why: String| problems.push(Problem { path, why });

    let Some(Object(input)) = input.0.valid() else {
        fail("$".into(), "must be an object".into());
        return problems;
    };
    let schema = input.schema.valid().map(String::as_str);
    if schema != Some(GLOSSARY_SCHEMA) && schema != Some(VIRGIL_GLOSSARY_SCHEMA) {
        fail("schema".into(), format!("must be \"{GLOSSARY_SCHEMA}\""));
    }
    if input.context.text().is_none() {
        fail("context".into(), "must be a non-empty string".into());
    }
    if input.purpose.text().is_none() {
        fail("purpose".into(), "must be a non-empty string".into());
    }

    // An unfamiliar repository has no reviewed vocabulary yet. An entirely
    // empty model is an honest starter; a half-filled one is still invalid.
    if input.sections.valid().is_some_and(Vec::is_empty)
        && input.terms.valid().is_some_and(Vec::is_empty)
    {
        return problems;
    }

    let mut section_ids: Vec<String> = Vec::new();
    match input.sections.valid() {
        Some(sections) if !sections.is_empty() => {
            for (index, section) in sections.iter().enumerate() {
                let at = format!("sections[{index}]");
                let Some(Object(section)) = section.valid() else {
                    fail(at, "must be an object".into());
                    continue;
                };
                match section.id.text() {
                    Some(id) if is_kebab(id) => {
                        if section_ids.iter().any(|known| known == id) {
                            fail(format!("{at}.id"), format!("duplicates section \"{id}\""));
                        } else {
                            section_ids.push(id.to_owned());
                        }
                    }
                    _ => fail(format!("{at}.id"), "must be kebab-case".into()),
                }
                if section.title.text().is_none() {
                    fail(format!("{at}.title"), "must be a non-empty string".into());
                }
            }
        }
        _ => fail("sections".into(), "must be a non-empty array".into()),
    }

    let terms = match input.terms.valid() {
        Some(terms) if !terms.is_empty() => terms,
        _ => {
            fail("terms".into(), "must be a non-empty array".into());
            return problems;
        }
    };

    let mut term_ids = BTreeSet::new();
    let mut names: BTreeMap<String, Vec<(String, BTreeSet<String>)>> = BTreeMap::new();
    let mut used_sections = BTreeSet::new();
    let mut avoid_words: Vec<(String, String)> = Vec::new();

    for (index, term) in terms.iter().enumerate() {
        let at = format!("terms[{index}]");
        let Some(Object(term)) = term.valid() else {
            fail(at, "must be an object".into());
            continue;
        };

        match term.id.text() {
            Some(id) if term_id_ok(id) => {
                if !term_ids.insert(id.to_owned()) {
                    fail(format!("{at}.id"), format!("duplicates term \"{id}\""));
                }
            }
            _ => fail(
                format!("{at}.id"),
                "must be \"term.\" followed by kebab-case".into(),
            ),
        }

        let scoped = match term.contexts.valid() {
            None if term.contexts.is_present() => {
                fail(
                    format!("{at}.contexts"),
                    "must be an array of context names".into(),
                );
                BTreeSet::new()
            }
            None => BTreeSet::new(),
            Some(values) => {
                let mut scoped = BTreeSet::new();
                for (scope_index, value) in values.iter().enumerate() {
                    match value.text() {
                        Some(name) if context.known_contexts.contains(name) => {
                            if !scoped.insert(name.to_owned()) {
                                fail(
                                    format!("{at}.contexts[{scope_index}]"),
                                    format!("duplicates context {name:?}"),
                                );
                            }
                        }
                        Some(name) => fail(
                            format!("{at}.contexts[{scope_index}]"),
                            format!(
                                "unknown context {name:?}; declare it in rosetta.toml path_scopes"
                            ),
                        ),
                        None => fail(
                            format!("{at}.contexts[{scope_index}]"),
                            "must be a non-empty context name".into(),
                        ),
                    }
                }
                if values.is_empty() {
                    fail(
                        format!("{at}.contexts"),
                        "remove an empty contexts array for a repository-wide term".into(),
                    );
                }
                scoped
            }
        };

        match term.name.text() {
            None => fail(format!("{at}.name"), "must be a non-empty string".into()),
            Some(name) => {
                let key = name.to_lowercase();
                if let Some((first, _)) = names.get(&key).and_then(|earlier| {
                    earlier.iter().find(|(_, contexts)| {
                        scoped.is_empty() || contexts.is_empty() || !scoped.is_disjoint(contexts)
                    })
                }) {
                    fail(
                        format!("{at}.name"),
                        format!("duplicates the name of {first}"),
                    );
                }
                names.entry(key).or_default().push((at.clone(), scoped));
            }
        }

        match term.section.text() {
            Some(section) if section_ids.iter().any(|known| known == section) => {
                used_sections.insert(section.to_owned());
            }
            _ => fail(
                format!("{at}.section"),
                format!(
                    "must name a declared section, got {}",
                    term.section.as_json()
                ),
            ),
        }

        match term.definition.text() {
            None => fail(
                format!("{at}.definition"),
                "must be a non-empty string".into(),
            ),
            Some(definition) => {
                let length = utf16_len(definition);
                if length > DEFINITION_MAX_CHARS {
                    fail(
                        format!("{at}.definition"),
                        format!("is {length} characters; at most {DEFINITION_MAX_CHARS}"),
                    );
                }
                let sentences = count_sentences(definition);
                if sentences > DEFINITION_MAX_SENTENCES {
                    fail(
                        format!("{at}.definition"),
                        format!("is {sentences} sentences; at most {DEFINITION_MAX_SENTENCES}"),
                    );
                }
            }
        }

        let status = term.status.valid().map(String::as_str);
        if !TermStatus::ALL
            .iter()
            .any(|known| Some(known.as_str()) == status)
        {
            let all: Vec<&str> = TermStatus::ALL.iter().map(|s| s.as_str()).collect();
            fail(
                format!("{at}.status"),
                format!("must be one of {}", all.join(", ")),
            );
        }

        if term.avoid.is_present() {
            match term.avoid.valid() {
                None => fail(format!("{at}.avoid"), "must be an array".into()),
                Some(entries) => {
                    for (avoid_index, entry) in entries.iter().enumerate() {
                        let avoid_at = format!("{at}.avoid[{avoid_index}]");
                        let entry = entry.valid().map(|Object(entry)| entry);
                        let Some((entry, word)) =
                            entry.and_then(|entry| Some((entry, entry.word.text()?)))
                        else {
                            fail(
                                format!("{avoid_at}.word"),
                                "must be a non-empty string".into(),
                            );
                            continue;
                        };
                        avoid_words.push((format!("{avoid_at}.word"), word.to_owned()));
                        if entry.except.is_present() && entry.except.texts().is_none() {
                            fail(
                                format!("{avoid_at}.except"),
                                "must be an array of non-empty strings".into(),
                            );
                        }
                    }
                }
            }
        }

        if term.code.is_present() {
            match term.code.texts() {
                None => fail(
                    format!("{at}.code"),
                    "must be an array of non-empty strings".into(),
                ),
                Some(names) => {
                    if let Some(known) = &context.code_names {
                        for (code_index, name) in names.iter().enumerate() {
                            if !known.resolves(name) {
                                fail(
                                    format!("{at}.code[{code_index}]"),
                                    format!("\"{name}\" is not {}", known.source_kinds()),
                                );
                            }
                        }
                    }
                }
            }
        }

        if term.refs.is_present() {
            match term.refs.valid() {
                None => fail(format!("{at}.refs"), "must be an object".into()),
                Some(Object(refs)) => check_refs(&at, refs, context, &mut fail),
            }
        }

        if term.decided.is_present() {
            let decided = term.decided.valid().map(|Object(decided)| decided);
            let on = decided.and_then(|decided| decided.on.text());
            if !on.is_some_and(iso_date_ok) {
                fail(
                    format!("{at}.decided.on"),
                    "must be a YYYY-MM-DD date".into(),
                );
            } else if let Some(decided) = decided
                && decided.page.is_present()
                && decided.page.text().is_none()
            {
                fail(
                    format!("{at}.decided.page"),
                    "must be a non-empty string".into(),
                );
            }
        }

        if term.epic.is_present() && !term.epic.valid().is_some_and(|epic| is_epic_ref(epic)) {
            fail(
                format!("{at}.epic"),
                "must be \"epic:<ULID>\" or an imported tracker key such as ABC-1a2b3".into(),
            );
        }
    }

    for id in &section_ids {
        if !used_sections.contains(id) {
            fail("sections".into(), format!("section \"{id}\" has no terms"));
        }
    }

    // An avoided word that is also a term's name would ban the term itself.
    for (path, word) in avoid_words {
        if names.contains_key(&word.to_lowercase()) {
            fail(path, format!("\"{word}\" is a term's name"));
        }
    }

    problems
}

fn check_refs(
    at: &str,
    refs: &SourceRefs,
    context: &ValidationContext<'_>,
    fail: &mut impl FnMut(String, String),
) {
    for (key, field, known, source) in [
        (
            "rules",
            &refs.rules,
            &context.rule_ids,
            &context.rule_source,
        ),
        (
            "principles",
            &refs.principles,
            &context.principle_ids,
            &context.principle_source,
        ),
    ] {
        if !field.is_present() {
            continue;
        }
        let Some(ids) = field.texts() else {
            fail(format!("{at}.refs.{key}"), "must be an array of ids".into());
            continue;
        };
        for (ref_index, id) in ids.iter().enumerate() {
            if !known.contains(*id) {
                let why = source.as_ref().map_or_else(
                    || format!("\"{id}\" has no configured {key} provider; set refs.rulebook in .rosetta/rosetta.toml"),
                    |path| format!("\"{id}\" is not in {path}"),
                );
                fail(format!("{at}.refs.{key}[{ref_index}]"), why);
            }
        }
    }
    if refs.docs.is_present() {
        match refs.docs.texts() {
            None => fail(
                format!("{at}.refs.docs"),
                "must be an array of repo paths".into(),
            ),
            Some(docs) => {
                for (ref_index, doc) in docs.iter().enumerate() {
                    if !(context.doc_exists)(doc) {
                        fail(
                            format!("{at}.refs.docs[{ref_index}]"),
                            format!("\"{doc}\" does not exist in the repo"),
                        );
                    }
                }
            }
        }
    }
    for key in &refs.other.0 {
        fail(format!("{at}.refs.{key}"), "is not a known ref kind".into());
    }
}
