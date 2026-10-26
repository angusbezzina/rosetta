//! The glossary: `.rosetta/glossary.json` is the source; GLOSSARY.md and the
//! `rule.glossary-terms` query are generated from it and checked against it.

pub mod code_names;
pub mod emit;
pub mod epic;
pub mod lint;
pub mod model;
pub mod names;
pub mod query;
pub mod render;
pub mod source;
pub mod validate;

use std::collections::BTreeSet;
use std::path::Path;
use std::process::Command;

pub use code_names::CodeNames;
pub use model::{Glossary, GlossaryTerm, Problem, TermDecision, TermStatus};
pub use render::{GLOSSARY_OUTPUT, GLOSSARY_SOURCE, render_glossary, render_glossary_for, wrap};
pub use source::GlossarySource;
pub use validate::{ValidationContext, count_sentences, validate_glossary};

use rosetta_core::config::{self, Config, ConfigError, RefKind};
use rosetta_core::diff::unified_diff;
use rosetta_core::json_text;
use rosetta_core::rulebook::{self, GLOSSARY_RULE, Rulebook};
use rosetta_core::text::kebab_of;

#[derive(Debug, thiserror::Error)]
pub enum GlossaryError {
    #[error(transparent)]
    Config(#[from] ConfigError),
    #[error("{path}: {source}")]
    Io {
        path: String,
        source: std::io::Error,
    },
    #[error(transparent)]
    Rulebook(#[from] rulebook::RulebookError),
    #[error(transparent)]
    CodeNames(#[from] code_names::CodeNamesError),
    #[error(transparent)]
    Format(#[from] json_text::FormatError),
    #[error("{} problem(s) in {GLOSSARY_SOURCE}", .0.len())]
    Invalid(Vec<Problem>),
    #[error("no settled term avoids a word, so {GLOSSARY_RULE} has nothing to enforce")]
    NoAvoidWords,
    #[error("git ls-files: {0}")]
    Git(String),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

fn io(path: &str) -> impl FnOnce(std::io::Error) -> GlossaryError + '_ {
    move |source| GlossaryError::Io {
        path: path.to_owned(),
        source,
    }
}

/// The validation context for a repo: its rulebook, its files, and the code
/// names it can resolve.
pub fn repo_context(root: &Path) -> Result<ValidationContext<'_>, GlossaryError> {
    let config = Config::load(root, None)?;
    let rulebook = Rulebook::load_optional_for(root, &config)?;
    let mut rule_ids = rulebook
        .as_ref()
        .map(Rulebook::rule_ids)
        .unwrap_or_default();
    rule_ids.extend(config.ref_values(root, RefKind::Rule)?);
    let mut principle_ids = rulebook
        .as_ref()
        .map(Rulebook::principle_ids)
        .unwrap_or_default();
    principle_ids.extend(config.ref_values(root, RefKind::Principle)?);
    let document_ids = config.ref_values(root, RefKind::Document)?;
    Ok(ValidationContext {
        rule_ids,
        principle_ids,
        rule_source: config.ref_source(RefKind::Rule),
        principle_source: config.ref_source(RefKind::Principle),
        doc_exists: Box::new(move |path| config::document_ref_exists(root, &document_ids, path)),
        code_names: Some(CodeNames::from_repo_with_config(root, &config)?),
        known_contexts: config
            .path_scopes
            .iter()
            .map(|scope| scope.context.clone())
            .collect(),
    })
}

#[derive(Debug, Default)]
pub struct LoadResult {
    /// Present only when the source is valid.
    pub glossary: Option<Glossary>,
    pub problems: Vec<Problem>,
}

/// Reads and validates the source; a glossary is returned only when it is valid.
pub fn load_glossary(root: &Path, context: &ValidationContext<'_>) -> LoadResult {
    load_glossary_from(root, GLOSSARY_SOURCE, context)
}

/// Read a glossary from a selected repository-relative source path.
pub fn load_glossary_from(
    root: &Path,
    source: &str,
    context: &ValidationContext<'_>,
) -> LoadResult {
    let text = match std::fs::read_to_string(root.join(source)) {
        Ok(text) => text,
        Err(error) => {
            return LoadResult {
                glossary: None,
                problems: vec![Problem::new(source, error.to_string())],
            };
        }
    };
    parse_glossary_from(&text, source, context)
}

pub fn parse_glossary(text: &str, context: &ValidationContext<'_>) -> LoadResult {
    parse_glossary_from(text, GLOSSARY_SOURCE, context)
}

pub fn parse_glossary_from(
    text: &str,
    source_path: &str,
    context: &ValidationContext<'_>,
) -> LoadResult {
    let source = match GlossarySource::parse(text) {
        Ok(parsed) => parsed,
        Err(error) => {
            return LoadResult {
                glossary: None,
                problems: vec![Problem::new(source_path, error.to_string())],
            };
        }
    };
    let problems: Vec<Problem> = validate_glossary(&source, context)
        .into_iter()
        .map(|problem| Problem::new(format!("{}#{}", source_path, problem.path), problem.why))
        .collect();
    if !problems.is_empty() {
        return LoadResult {
            glossary: None,
            problems,
        };
    }
    match serde_json::from_str(text) {
        Ok(glossary) => LoadResult {
            glossary: Some(glossary),
            problems,
        },
        Err(error) => LoadResult {
            glossary: None,
            problems: vec![Problem::new(source_path, error.to_string())],
        },
    }
}

#[derive(Debug, Default)]
pub struct CheckReport {
    pub problems: Vec<Problem>,
    /// Shown after the problems: what a render would change.
    pub diffs: Vec<String>,
    /// Printed but never failing.
    pub warnings: Vec<Problem>,
}

impl CheckReport {
    pub fn passed(&self) -> bool {
        self.problems.is_empty()
    }
}

/// Validates the source and compares GLOSSARY.md and the rulebook query with
/// what a render would write.
pub fn check_glossary(
    root: &Path,
    context: &ValidationContext<'_>,
    epics: &dyn epic::EpicStatus,
) -> Result<CheckReport, GlossaryError> {
    check_glossary_from(root, GLOSSARY_SOURCE, context, epics)
}

pub fn check_glossary_from(
    root: &Path,
    source: &str,
    context: &ValidationContext<'_>,
    epics: &dyn epic::EpicStatus,
) -> Result<CheckReport, GlossaryError> {
    let LoadResult { glossary, problems } = load_glossary_from(root, source, context);
    let Some(glossary) = glossary else {
        return Ok(CheckReport {
            problems,
            ..CheckReport::default()
        });
    };
    let mut report = CheckReport {
        warnings: epic::closed_epic_warnings_for(&glossary, source, epics),
        ..CheckReport::default()
    };

    let rendered = render_glossary_for(&glossary, source);
    let committed = std::fs::read_to_string(root.join(GLOSSARY_OUTPUT)).unwrap_or_default();
    if committed != rendered {
        report.problems.push(Problem::new(
            GLOSSARY_OUTPUT,
            format!("differs from the render of {source}; run rosetta glossary render"),
        ));
        report.diffs.push(unified_diff(
            &committed,
            &rendered,
            &format!("{GLOSSARY_OUTPUT} (committed)"),
            &format!("{GLOSSARY_OUTPUT} (rendered)"),
        ));
    }

    check_emitted(root, &glossary, source, &mut report)?;

    if glossary.terms.is_empty() {
        return Ok(report);
    }

    // A repo without the rule has no query to keep in step.
    let config = Config::load(root, None)?;
    let rulebook = Rulebook::load_optional_for(root, &config)?;
    let Some(committed_query) = rulebook.as_ref().and_then(Rulebook::glossary_query) else {
        return Ok(report);
    };
    let expected = query::glossary_query(&glossary).ok_or(GlossaryError::NoAvoidWords)?;
    if committed_query != expected {
        let path = config
            .refs
            .rulebook
            .as_deref()
            .ok_or(rulebook::RulebookError::NotConfigured)?;
        report.problems.push(Problem::new(
            format!("{path}#{GLOSSARY_RULE}.enforcer.query"),
            format!("differs from the query generated from {source}; run rosetta glossary render"),
        ));
        report.diffs.push(unified_diff(
            committed_query,
            &expected,
            "query (committed)",
            "query (generated)",
        ));
    }
    Ok(report)
}

/// Write one adapter's canonical output. Existing generated Vale rules whose
/// terms were removed are retired; other files in the style directory stay.
pub fn emit_files(
    root: &Path,
    glossary: &Glossary,
    target: emit::Target,
) -> Result<Vec<String>, GlossaryError> {
    let files = emit::files(glossary, target)?;
    if target == emit::Target::Vale {
        let style_dir = root.join(emit::VALE_STYLE);
        if style_dir.is_dir() {
            for entry in std::fs::read_dir(&style_dir).map_err(io(emit::VALE_STYLE))? {
                let entry = entry.map_err(io(emit::VALE_STYLE))?;
                let path = entry.path();
                let relative = format!(
                    "{}/{}",
                    emit::VALE_STYLE,
                    entry.file_name().to_string_lossy()
                );
                if files.contains_key(&relative)
                    || !entry.file_name().to_string_lossy().starts_with("Term-")
                    || path.extension().and_then(|value| value.to_str()) != Some("yml")
                {
                    continue;
                }
                if std::fs::read_to_string(&path).is_ok_and(|text| {
                    text.starts_with("# Generated by Rosetta from the glossary.\n")
                }) {
                    std::fs::remove_file(&path).map_err(io(&relative))?;
                }
            }
        }
    }
    for (path, text) in &files {
        let output = root.join(path);
        if let Some(parent) = output.parent() {
            std::fs::create_dir_all(parent).map_err(io(path))?;
        }
        if std::fs::read_to_string(&output).ok().as_deref() != Some(text) {
            std::fs::write(output, text).map_err(io(path))?;
        }
    }
    Ok(files.into_keys().collect())
}

fn check_emitted(
    root: &Path,
    glossary: &Glossary,
    source: &str,
    report: &mut CheckReport,
) -> Result<(), GlossaryError> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args([
            "ls-files",
            "-z",
            "--",
            emit::VALE_STYLE,
            emit::VALE_ACCEPT,
            emit::WHETSTONE_QUERY,
            emit::WORD_LIST,
        ])
        .output()
        .map_err(|error| GlossaryError::Git(error.to_string()))?;
    if !output.status.success() {
        return Err(GlossaryError::Git(
            String::from_utf8_lossy(&output.stderr).trim().to_owned(),
        ));
    }
    let tracked = output
        .stdout
        .split(|byte| *byte == 0)
        .filter(|part| !part.is_empty())
        .map(|part| String::from_utf8_lossy(part).into_owned())
        .collect::<BTreeSet<_>>();
    for target in [
        emit::Target::Vale,
        emit::Target::Whetstone,
        emit::Target::Json,
    ] {
        let active = tracked.iter().any(|path| match target {
            emit::Target::Vale => {
                path.starts_with(&format!("{}/", emit::VALE_STYLE)) || path == emit::VALE_ACCEPT
            }
            emit::Target::Whetstone => path == emit::WHETSTONE_QUERY,
            emit::Target::Json => path == emit::WORD_LIST,
        });
        if !active {
            continue;
        }
        let expected = emit::files(glossary, target)?;
        let current_paths = tracked
            .iter()
            .filter(|path| match target {
                emit::Target::Vale => {
                    path.starts_with(&format!("{}/", emit::VALE_STYLE))
                        || *path == emit::VALE_ACCEPT
                }
                emit::Target::Whetstone => *path == emit::WHETSTONE_QUERY,
                emit::Target::Json => *path == emit::WORD_LIST,
            })
            .cloned()
            .collect::<BTreeSet<_>>();
        for (path, rendered) in &expected {
            let committed = std::fs::read_to_string(root.join(path)).unwrap_or_default();
            if committed != *rendered || !tracked.contains(path) {
                report.problems.push(Problem::new(path, format!("differs from the emit of {source}; run rosetta glossary emit {} and stage the result", target_name(target))));
                report.diffs.push(unified_diff(
                    &committed,
                    rendered,
                    &format!("{path} (committed)"),
                    &format!("{path} (generated)"),
                ));
            }
        }
        for path in current_paths {
            if !expected.contains_key(&path) {
                report.problems.push(Problem::new(
                    path,
                    "stale emitted file; run rosetta glossary emit vale and stage the deletion",
                ));
            }
        }
    }
    Ok(())
}

fn target_name(target: emit::Target) -> &'static str {
    match target {
        emit::Target::Vale => "vale",
        emit::Target::Whetstone => "whetstone",
        emit::Target::Json => "json",
    }
}

/// Writes GLOSSARY.md and the rulebook query from a valid source.
pub fn render_files(root: &Path, glossary: &Glossary) -> Result<(), GlossaryError> {
    render_files_from(root, GLOSSARY_SOURCE, glossary)
}

pub fn render_files_from(
    root: &Path,
    source: &str,
    glossary: &Glossary,
) -> Result<(), GlossaryError> {
    let output = root.join(GLOSSARY_OUTPUT);
    std::fs::write(&output, render_glossary_for(glossary, source)).map_err(io(GLOSSARY_OUTPUT))?;
    if glossary.terms.is_empty() {
        return Ok(());
    }
    let config = Config::load(root, None)?;
    let rulebook = Rulebook::load_optional_for(root, &config)?;
    if rulebook
        .as_ref()
        .and_then(Rulebook::glossary_query)
        .is_none()
    {
        return Ok(());
    }
    let expected = query::glossary_query(glossary).ok_or(GlossaryError::NoAvoidWords)?;
    let relative = config
        .refs
        .rulebook
        .as_deref()
        .ok_or(rulebook::RulebookError::NotConfigured)?;
    let path = root.join(relative);
    let text = std::fs::read_to_string(&path).map_err(io(relative))?;
    let updated = rulebook::replace_glossary_query(&text, &expected)?;
    if updated != text {
        std::fs::write(&path, updated).map_err(io(relative))?;
    }
    Ok(())
}

/// A term a plan has just settled on, written as proposed.
#[derive(Debug, Clone)]
pub struct NewTerm {
    pub name: String,
    pub definition: String,
    pub section: String,
    pub page: Option<String>,
    pub epic: Option<String>,
}

/// Appends a proposed term, validates the result, then writes the source in
/// its committed layout and renders GLOSSARY.md. Nothing is written when the
/// result is invalid. `today` is the `decided.on` date.
pub fn add_term(
    root: &Path,
    context: &ValidationContext<'_>,
    term: NewTerm,
    today: &str,
) -> Result<GlossaryTerm, GlossaryError> {
    add_term_from(root, GLOSSARY_SOURCE, context, term, today)
}

pub fn add_term_from(
    root: &Path,
    source: &str,
    context: &ValidationContext<'_>,
    term: NewTerm,
    today: &str,
) -> Result<GlossaryTerm, GlossaryError> {
    let LoadResult { glossary, problems } = load_glossary_from(root, source, context);
    let mut glossary = glossary.ok_or(GlossaryError::Invalid(problems))?;
    let added = GlossaryTerm {
        id: format!("term.{}", kebab_of(&term.name)),
        name: term.name,
        section: term.section,
        definition: term.definition,
        contexts: Vec::new(),
        avoid: None,
        code: None,
        refs: None,
        status: TermStatus::Proposed,
        decided: Some(TermDecision {
            on: today.to_owned(),
            page: term.page,
        }),
        epic: term.epic,
    };
    glossary.terms.push(added.clone());
    let text = json_text::to_pretty_string(&glossary)?;
    let LoadResult { glossary, problems } = parse_glossary_from(&text, source, context);
    let glossary = glossary.ok_or(GlossaryError::Invalid(problems))?;
    std::fs::write(root.join(source), &text).map_err(io(source))?;
    render_files_from(root, source, &glossary)?;
    Ok(added)
}

/// A term by id (`term.door`) or by name, case-insensitively.
pub fn find_term<'a>(glossary: &'a Glossary, key: &str) -> Option<&'a GlossaryTerm> {
    glossary
        .terms
        .iter()
        .find(|term| term.id == key)
        .or_else(|| {
            let mut matches = glossary
                .terms
                .iter()
                .filter(|term| term.name.eq_ignore_ascii_case(key));
            let first = matches.next()?;
            matches.next().is_none().then_some(first)
        })
}

/// The UTC date of `at_ms` (milliseconds since the epoch) as YYYY-MM-DD.
pub fn date_utc(at_ms: i64) -> String {
    civil_date(u64::try_from(at_ms.div_euclid(86_400_000)).unwrap_or(0))
}

/// Days since 1970-01-01 to a proleptic Gregorian date (Hinnant's algorithm).
pub fn civil_date(days: u64) -> String {
    let z = days as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02}")
}
