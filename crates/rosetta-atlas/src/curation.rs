//! Validated, revision-gated edits to the two canonical model files.
//!
//! The browser and an agent submit the same declarative proposal. Preview is
//! pure; only explicit apply writes source and regenerates checked projections.

use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::atlas::{self, Atlas, export};
use crate::glossary::{self, Glossary};
use crate::json_text;
use rosetta_core::snapshot;

pub const PROPOSAL_SCHEMA: &str = "rosetta.curation/v1";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Proposal {
    pub schema: String,
    /// The model revision shown when the person or agent began this edit.
    pub base_model_revision: String,
    #[serde(default)]
    pub rationale: String,
    pub changes: Vec<Change>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Change {
    SetNodeLabel {
        id: String,
        value: String,
    },
    SetNodeSummary {
        id: String,
        value: String,
    },
    SetNodeTerm {
        id: String,
        term: Option<String>,
    },
    SetTermName {
        id: String,
        value: String,
    },
    SetTermDefinition {
        id: String,
        value: String,
    },
    SetCapabilityName {
        id: String,
        value: String,
    },
    SetCapabilityPurpose {
        id: String,
        value: String,
    },
    SetCapabilityPart {
        capability: String,
        part: String,
        present: bool,
    },
}

#[derive(Debug, Clone, Serialize)]
pub struct FieldChange {
    pub source: String,
    pub id: String,
    pub field: String,
    pub before: Value,
    pub after: Value,
}

#[derive(Debug, Clone, Serialize)]
pub struct Preview {
    pub schema: &'static str,
    pub base_model_revision: String,
    pub rationale: String,
    pub changes: Vec<FieldChange>,
    pub affected_files: Vec<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum CurationError {
    #[error(transparent)]
    Config(#[from] rosetta_core::config::ConfigError),
    #[error("{0}: {1}")]
    Io(String, #[source] std::io::Error),
    #[error(transparent)]
    Atlas(#[from] atlas::AtlasLoadError),
    #[error(transparent)]
    Glossary(#[from] glossary::GlossaryError),
    #[error(transparent)]
    Format(#[from] json_text::FormatError),
    #[error("expected proposal schema {PROPOSAL_SCHEMA}")]
    Schema,
    #[error(
        "the model changed since the proposal was made: expected {expected}, current {current}"
    )]
    Conflict { expected: String, current: String },
    #[error("{0}")]
    Invalid(String),
    #[error("the just-applied edit cannot be undone because {0} changed")]
    UndoConflict(String),
    #[error("there is no just-applied edit to undo")]
    NoUndo,
}

fn io(path: &Path, error: std::io::Error) -> CurationError {
    CurationError::Io(path.display().to_string(), error)
}

fn source_paths(root: &Path, source_dir: &str) -> Result<Vec<String>, CurationError> {
    let mut paths = vec![
        format!("{source_dir}/atlas.json"),
        format!("{source_dir}/glossary.json"),
        "ATLAS.md".into(),
        "GLOSSARY.md".into(),
    ];
    paths.extend(
        rosetta_core::config::Config::load(root, None)?
            .refs
            .rulebook,
    );
    Ok(paths)
}

fn inside_repo(root: &Path, relative: &str) -> Result<PathBuf, CurationError> {
    if !Path::new(relative)
        .components()
        .all(|component| matches!(component, std::path::Component::Normal(_)))
    {
        return Err(CurationError::Invalid(format!("unsafe path {relative}")));
    }
    let root = root.canonicalize().map_err(|error| io(root, error))?;
    let path = root.join(relative);
    let mut ancestor = path
        .parent()
        .ok_or_else(|| CurationError::Invalid(format!("unsafe path {relative}")))?;
    while !ancestor.exists() {
        ancestor = ancestor
            .parent()
            .ok_or_else(|| CurationError::Invalid(format!("unsafe path {relative}")))?;
    }
    let ancestor = ancestor
        .canonicalize()
        .map_err(|error| io(ancestor, error))?;
    if !ancestor.starts_with(&root) {
        return Err(CurationError::Invalid(format!(
            "{relative} resolves outside the repository"
        )));
    }
    if path.is_symlink() {
        return Err(CurationError::Invalid(format!("{relative} is a symlink")));
    }
    Ok(path)
}

fn read_optional(root: &Path, relative: &str) -> Result<Option<Vec<u8>>, CurationError> {
    let path = inside_repo(root, relative)?;
    match std::fs::read(&path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(io(&path, error)),
    }
}

fn write_optional(root: &Path, relative: &str, bytes: Option<&[u8]>) -> Result<(), CurationError> {
    let path = inside_repo(root, relative)?;
    if let Some(bytes) = bytes {
        let parent = path
            .parent()
            .ok_or_else(|| CurationError::Invalid(format!("unsafe path {relative}")))?;
        let mut temp = tempfile::NamedTempFile::new_in(parent).map_err(|error| io(&path, error))?;
        temp.write_all(bytes).map_err(|error| io(&path, error))?;
        temp.persist(&path)
            .map_err(|error| io(&path, error.error))?;
    } else if path.exists() {
        std::fs::remove_file(&path).map_err(|error| io(&path, error))?;
    }
    Ok(())
}

fn text(value: &str, field: &str) -> Result<(), CurationError> {
    if value.trim().is_empty() || value.len() > 1000 || value.contains(['\r', '\n']) {
        return Err(CurationError::Invalid(format!(
            "{field} must be one non-empty line of at most 1000 bytes"
        )));
    }
    Ok(())
}

fn field(source: &str, id: &str, field: &str, before: Value, after: Value) -> FieldChange {
    FieldChange {
        source: source.into(),
        id: id.into(),
        field: field.into(),
        before,
        after,
    }
}

fn apply_change(
    atlas: &mut Atlas,
    glossary: &mut Glossary,
    change: &Change,
) -> Result<FieldChange, CurationError> {
    match change {
        Change::SetNodeLabel { id, value } => {
            text(value, "node label")?;
            let node = atlas
                .nodes
                .iter_mut()
                .find(|node| node.id == *id)
                .ok_or_else(|| CurationError::Invalid(format!("unknown committed node {id}")))?;
            let before = json!(node.label);
            node.label = value.clone();
            Ok(field("atlas", id, "label", before, json!(value)))
        }
        Change::SetNodeSummary { id, value } => {
            text(value, "node responsibility")?;
            let node = atlas
                .nodes
                .iter_mut()
                .find(|node| node.id == *id)
                .ok_or_else(|| CurationError::Invalid(format!("unknown committed node {id}")))?;
            let before = json!(node.summary);
            node.summary = Some(value.clone());
            Ok(field("atlas", id, "summary", before, json!(value)))
        }
        Change::SetNodeTerm { id, term } => {
            if let Some(term) = term
                && !glossary.terms.iter().any(|entry| entry.id == *term)
            {
                return Err(CurationError::Invalid(format!(
                    "unknown glossary term {term}"
                )));
            }
            let node = atlas
                .nodes
                .iter_mut()
                .find(|node| node.id == *id)
                .ok_or_else(|| CurationError::Invalid(format!("unknown committed node {id}")))?;
            let before = json!(node.refs.term);
            node.refs.term = term.clone();
            Ok(field("atlas", id, "refs.term", before, json!(term)))
        }
        Change::SetTermName { id, value } => {
            text(value, "term name")?;
            let term = glossary
                .terms
                .iter_mut()
                .find(|term| term.id == *id)
                .ok_or_else(|| CurationError::Invalid(format!("unknown term {id}")))?;
            let before = json!(term.name);
            term.name = value.clone();
            Ok(field("glossary", id, "name", before, json!(value)))
        }
        Change::SetTermDefinition { id, value } => {
            text(value, "term definition")?;
            let term = glossary
                .terms
                .iter_mut()
                .find(|term| term.id == *id)
                .ok_or_else(|| CurationError::Invalid(format!("unknown term {id}")))?;
            let before = json!(term.definition);
            term.definition = value.clone();
            Ok(field("glossary", id, "definition", before, json!(value)))
        }
        Change::SetCapabilityName { id, value } => {
            text(value, "capability name")?;
            let capability = atlas
                .capabilities
                .iter_mut()
                .find(|capability| capability.id == *id)
                .ok_or_else(|| CurationError::Invalid(format!("unknown capability {id}")))?;
            let before = json!(capability.name);
            capability.name = value.clone();
            Ok(field("atlas", id, "name", before, json!(value)))
        }
        Change::SetCapabilityPurpose { id, value } => {
            text(value, "capability purpose")?;
            let capability = atlas
                .capabilities
                .iter_mut()
                .find(|capability| capability.id == *id)
                .ok_or_else(|| CurationError::Invalid(format!("unknown capability {id}")))?;
            let before = json!(capability.purpose);
            capability.purpose = value.clone();
            Ok(field("atlas", id, "purpose", before, json!(value)))
        }
        Change::SetCapabilityPart {
            capability,
            part,
            present,
        } => {
            if !atlas
                .nodes
                .iter()
                .any(|node| node.id == *part && node.kind.is_capability_owner())
            {
                return Err(CurationError::Invalid(format!("unknown part {part}")));
            }
            let entry = atlas
                .capabilities
                .iter_mut()
                .find(|entry| entry.id == *capability)
                .ok_or_else(|| {
                    CurationError::Invalid(format!("unknown capability {capability}"))
                })?;
            let before = json!(entry.parts);
            entry.parts.retain(|known| known != part);
            if *present {
                entry.parts.push(part.clone());
                entry.parts.sort();
            }
            Ok(field(
                "atlas",
                capability,
                "parts",
                before,
                json!(entry.parts),
            ))
        }
    }
}

struct Prepared {
    preview: Preview,
    atlas: Option<String>,
    glossary: Option<String>,
}

fn prepare(root: &Path, source_dir: &str, proposal: &Proposal) -> Result<Prepared, CurationError> {
    if proposal.schema != PROPOSAL_SCHEMA {
        return Err(CurationError::Schema);
    }
    if proposal.changes.is_empty() || proposal.changes.len() > 100 {
        return Err(CurationError::Invalid(
            "proposal needs 1..100 changes".into(),
        ));
    }
    if proposal.rationale.len() > 2000 {
        return Err(CurationError::Invalid(
            "rationale exceeds 2000 bytes".into(),
        ));
    }
    let old_atlas = atlas::load_atlas_from(root, source_dir)?;
    let old_glossary = atlas::load_glossary_terms_from(root, source_dir)?;
    let current = snapshot::capture(root, source_dir, &old_atlas.project.repo).model_revision;
    if current != proposal.base_model_revision {
        return Err(CurationError::Conflict {
            expected: proposal.base_model_revision.clone(),
            current,
        });
    }
    let mut atlas = old_atlas.clone();
    let mut glossary = old_glossary.clone();
    let mut changes = Vec::new();
    for change in &proposal.changes {
        let diff = apply_change(&mut atlas, &mut glossary, change)?;
        if diff.before != diff.after {
            changes.push(diff);
        }
    }
    if changes.is_empty() {
        return Err(CurationError::Invalid("proposal makes no change".into()));
    }
    let glossary_source = format!("{source_dir}/glossary.json");
    let glossary_text = json_text::to_pretty_string(&glossary)?;
    let context = glossary::repo_context(root)?;
    let loaded = glossary::parse_glossary_from(&glossary_text, &glossary_source, &context);
    if !loaded.problems.is_empty() {
        return Err(CurationError::Invalid(
            loaded
                .problems
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join("\n"),
        ));
    }
    let mut atlas_context = atlas::repo_context_from(root, source_dir)?;
    atlas_context.term_ids = glossary
        .terms
        .iter()
        .map(|term| term.id.clone())
        .collect::<BTreeSet<_>>();
    let findings = atlas::validate_atlas(&atlas, &atlas_context);
    if !findings.is_empty() {
        return Err(CurationError::Invalid(
            findings
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join("\n"),
        ));
    }
    let atlas_text = json_text::to_pretty_string(&atlas)?;
    let atlas_changed = atlas != old_atlas;
    let glossary_changed = glossary != old_glossary;
    let mut affected_files = Vec::new();
    if atlas_changed {
        affected_files.extend([format!("{source_dir}/atlas.json"), "ATLAS.md".to_owned()]);
    }
    if glossary_changed {
        affected_files.extend([glossary_source, "GLOSSARY.md".to_owned()]);
    }
    Ok(Prepared {
        preview: Preview {
            schema: PROPOSAL_SCHEMA,
            base_model_revision: proposal.base_model_revision.clone(),
            rationale: proposal.rationale.clone(),
            changes,
            affected_files,
        },
        atlas: atlas_changed.then_some(atlas_text),
        glossary: glossary_changed.then_some(glossary_text),
    })
}

pub fn preview(
    root: &Path,
    source_dir: &str,
    proposal: &Proposal,
) -> Result<Preview, CurationError> {
    Ok(prepare(root, source_dir, proposal)?.preview)
}

#[derive(Debug)]
pub struct UndoRecord {
    before: BTreeMap<String, Option<Vec<u8>>>,
    after: BTreeMap<String, Option<Vec<u8>>>,
}

/// Apply a proposal only after preview. The caller keeps the returned record
/// for one undo and must not accept another apply without replacing it.
pub fn apply(
    root: &Path,
    source_dir: &str,
    proposal: &Proposal,
) -> Result<(Preview, UndoRecord), CurationError> {
    let prepared = prepare(root, source_dir, proposal)?;
    let mut before = BTreeMap::new();
    for relative in source_paths(root, source_dir)? {
        before.insert(relative.clone(), read_optional(root, &relative)?);
    }
    let outcome = (|| {
        if let Some(atlas) = &prepared.atlas {
            write_optional(
                root,
                &format!("{source_dir}/atlas.json"),
                Some(atlas.as_bytes()),
            )?;
        }
        if let Some(glossary) = &prepared.glossary {
            write_optional(
                root,
                &format!("{source_dir}/glossary.json"),
                Some(glossary.as_bytes()),
            )?;
        }
        if prepared.glossary.is_some() {
            let context = glossary::repo_context(root)?;
            let loaded = glossary::load_glossary_from(
                root,
                &format!("{source_dir}/glossary.json"),
                &context,
            );
            let glossary = loaded.glossary.ok_or_else(|| {
                CurationError::Invalid(format!("new glossary invalid: {:?}", loaded.problems))
            })?;
            glossary::render_files_from(root, &format!("{source_dir}/glossary.json"), &glossary)?;
        }
        if prepared.atlas.is_some() || prepared.glossary.is_some() {
            export::export_repo_from(root, source_dir)?;
        }
        Ok::<(), CurationError>(())
    })();
    if let Err(error) = outcome {
        for (relative, bytes) in &before {
            let _ = write_optional(root, relative, bytes.as_deref());
        }
        return Err(error);
    }
    let mut after = BTreeMap::new();
    for relative in before.keys() {
        after.insert(relative.clone(), read_optional(root, relative)?);
    }
    Ok((prepared.preview, UndoRecord { before, after }))
}

/// Undo only while every file this apply changed still has its exact applied
/// bytes. An intervening editor change is kept and reported as a conflict.
pub fn undo(root: &Path, record: UndoRecord) -> Result<(), (CurationError, Box<UndoRecord>)> {
    for (relative, previous) in &record.before {
        let applied = record.after.get(relative);
        if applied == Some(previous) {
            continue;
        }
        match read_optional(root, relative) {
            Ok(current) if Some(&current) == applied => {}
            Ok(_) => {
                return Err((
                    CurationError::UndoConflict(relative.clone()),
                    Box::new(record),
                ));
            }
            Err(error) => return Err((error, Box::new(record))),
        }
    }
    for (relative, previous) in &record.before {
        if record.after.get(relative) != Some(previous)
            && let Err(error) = write_optional(root, relative, previous.as_deref())
        {
            return Err((error, Box::new(record)));
        }
    }
    Ok(())
}
