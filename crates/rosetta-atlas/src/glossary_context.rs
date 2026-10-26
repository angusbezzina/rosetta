//! A bounded vocabulary slice tied to the same revision and view as Atlas.

use std::collections::BTreeSet;
use std::path::Path;

use serde::Serialize;

use crate::atlas::{
    self, Bundle,
    focus::{self, FocusedView, Query},
};
use crate::glossary::{Glossary, GlossaryTerm};
use rosetta_core::config::{Config, ConfigError};
use rosetta_core::snapshot::{self, Snapshot};

pub const CONTEXT_SCHEMA: &str = "rosetta.glossary-context/v1";

#[derive(Debug, thiserror::Error)]
pub enum ContextError {
    #[error(transparent)]
    Config(#[from] ConfigError),
    #[error(transparent)]
    Atlas(#[from] atlas::AtlasLoadError),
    #[error(transparent)]
    Focus(#[from] focus::FocusError),
    #[error("--view and --for cannot be combined; the view already has a path selection")]
    ConflictingPaths,
    #[error("--max-chars is too small for the view reference and omission counts")]
    Budget,
}

#[derive(Debug, Clone, Serialize)]
pub struct ContextTerm {
    pub id: String,
    pub name: String,
    pub definition: String,
    pub status: String,
    pub contexts: Vec<String>,
    pub avoid: Vec<String>,
    pub mentioned_in_paths: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct ContextReport {
    pub schema: &'static str,
    pub snapshot: Snapshot,
    pub view_id: Option<String>,
    pub view_reference: Option<String>,
    pub paths: Vec<String>,
    pub contexts: Vec<String>,
    pub parts: Vec<String>,
    pub unmapped_paths: Vec<String>,
    pub terms: Vec<ContextTerm>,
}

fn covers(node_path: &str, path: &str) -> bool {
    path == node_path || path.starts_with(&format!("{node_path}/"))
}

fn view_for(
    bundle: &Bundle,
    paths: &[String],
    reference: Option<&str>,
) -> Result<Option<FocusedView>, ContextError> {
    if let Some(reference) = reference {
        return Ok(Some(focus::reopen(bundle, reference)?));
    }
    if paths.is_empty() {
        return Ok(None);
    }
    let mapped: Vec<String> = paths
        .iter()
        .filter(|path| {
            bundle.nodes.iter().any(|node| {
                node.path
                    .as_deref()
                    .is_some_and(|candidate| covers(candidate, path))
            })
        })
        .cloned()
        .collect();
    if mapped.is_empty() {
        return Ok(None);
    }
    Ok(Some(focus::select(
        bundle,
        Query {
            paths: mapped,
            ..Query::default()
        },
    )?))
}

/// Paths stay repository-relative. A view reference provides the same path
/// selection as `atlas context` and is rejected if the snapshot changed.
pub fn select(
    root: &Path,
    config: &Config,
    glossary: &Glossary,
    requested_paths: &[String],
    reference: Option<&str>,
) -> Result<ContextReport, ContextError> {
    if reference.is_some() && !requested_paths.is_empty() {
        return Err(ContextError::ConflictingPaths);
    }
    let atlas_path = config.source_file(root, "atlas.json");
    let bundle = atlas_path
        .is_file()
        .then(|| atlas::bundle_repo_from(root, &config.source.directory))
        .transpose()?;
    let reopened = reference
        .map(|value| {
            bundle
                .as_ref()
                .ok_or(focus::FocusError::MissingSnapshot)
                .and_then(|bundle| focus::reopen(bundle, value))
        })
        .transpose()?;
    let mut paths = reopened
        .as_ref()
        .map_or_else(|| requested_paths.to_vec(), |view| view.query.paths.clone());
    paths.sort();
    paths.dedup();
    let mut context_set = BTreeSet::new();
    for path in &paths {
        context_set.extend(config.contexts_for_path(path)?);
    }
    let view = match reopened {
        Some(view) => Some(view),
        None => bundle
            .as_ref()
            .map(|bundle| view_for(bundle, &paths, None))
            .transpose()?
            .flatten(),
    };
    let snapshot = bundle
        .as_ref()
        .and_then(|bundle| bundle.snapshot.clone())
        .unwrap_or_else(|| {
            let repository = atlas::starter_atlas(root)
                .map_or_else(|_| root.display().to_string(), |atlas| atlas.project.repo);
            snapshot::capture(root, &config.source.directory, &repository)
        });
    let parts = view.as_ref().map_or_else(Vec::new, |view| {
        view.nodes
            .iter()
            .filter(|node| node.kind.is_part())
            .map(|node| node.id.clone())
            .collect()
    });
    let unmapped_paths = if let Some(bundle) = &bundle {
        paths
            .iter()
            .filter(|path| {
                !bundle.nodes.iter().any(|node| {
                    node.path
                        .as_deref()
                        .is_some_and(|candidate| covers(candidate, path))
                })
            })
            .cloned()
            .collect()
    } else {
        paths.clone()
    };
    let file_text: String = paths
        .iter()
        .filter_map(|path| {
            let candidate = root.join(path);
            let canonical = candidate.canonicalize().ok()?;
            let canonical_root = root.canonicalize().ok()?;
            if !canonical.starts_with(&canonical_root)
                || std::fs::metadata(&canonical).ok()?.len() > 1024 * 1024
            {
                return None;
            }
            std::fs::read_to_string(canonical).ok()
        })
        .collect::<Vec<_>>()
        .join("\n")
        .to_lowercase();
    let view_terms: Option<BTreeSet<&str>> = view
        .as_ref()
        .map(|view| view.terms.iter().map(|term| term.id.as_str()).collect());
    let mut terms: Vec<ContextTerm> = glossary
        .terms
        .iter()
        .filter(|term| {
            (paths.is_empty()
                || term.contexts.is_empty()
                || term
                    .contexts
                    .iter()
                    .any(|scope| context_set.contains(scope)))
                && (paths.is_empty() && reference.is_none()
                    || view_terms
                        .as_ref()
                        .is_none_or(|ids| ids.contains(term.id.as_str())))
        })
        .map(|term: &GlossaryTerm| ContextTerm {
            id: term.id.clone(),
            name: term.name.clone(),
            definition: term.definition.clone(),
            status: term.status.as_str().to_owned(),
            contexts: term.contexts.clone(),
            avoid: term
                .avoid_words()
                .iter()
                .map(|word| word.word.clone())
                .collect(),
            mentioned_in_paths: !file_text.is_empty()
                && file_text.contains(&term.name.to_lowercase()),
        })
        .collect();
    terms.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(ContextReport {
        schema: CONTEXT_SCHEMA,
        snapshot,
        view_id: view.as_ref().map(|view| view.id.clone()),
        view_reference: view.as_ref().map(|view| view.reference.clone()),
        paths,
        contexts: context_set.into_iter().collect(),
        parts,
        unmapped_paths,
        terms,
    })
}

/// Render whole lines and make every loss visible. Avoid words go first,
/// then definitions absent from the selected files, then remaining detail.
pub fn render_text(report: &ContextReport, max_chars: usize) -> Result<String, ContextError> {
    let header = format!(
        "Snapshot: {}\nView: {}\nReference: {}\nPaths: {}\nContexts: {}\nParts: {}\nUnmapped paths: {}\n",
        report.snapshot.fingerprint,
        report
            .view_id
            .as_deref()
            .unwrap_or("none (no atlas selection)"),
        report.view_reference.as_deref().unwrap_or("none"),
        report.paths.join(", "),
        report.contexts.join(", "),
        report.parts.join(", "),
        report.unmapped_paths.join(", ")
    );
    let len = |text: &str| text.chars().count();
    let mut include = vec![true; report.terms.len()];
    let mut definitions = vec![true; report.terms.len()];
    let mut avoids = vec![true; report.terms.len()];
    let render = |include: &[bool], definitions: &[bool], avoids: &[bool]| {
        let mut output = header.clone();
        for (index, term) in report.terms.iter().enumerate() {
            if !include[index] {
                continue;
            }
            output.push_str(&format!("{}: {} ({})", term.id, term.name, term.status));
            if definitions[index] {
                output.push_str(&format!(" — {}", term.definition));
            }
            if avoids[index] && !term.avoid.is_empty() {
                output.push_str(&format!(" | Avoid: {}", term.avoid.join(", ")));
            }
            output.push('\n');
        }
        output.push_str(&format!(
            "Omitted for text budget: {} terms, {} definitions, {} Avoid lists.\n",
            include.iter().filter(|item| !**item).count(),
            definitions
                .iter()
                .enumerate()
                .filter(|(index, item)| include[*index] && !**item)
                .count(),
            avoids
                .iter()
                .enumerate()
                .filter(|(index, item)| include[*index]
                    && !**item
                    && !report.terms[*index].avoid.is_empty())
                .count()
        ));
        output
    };
    let mut output = render(&include, &definitions, &avoids);
    for index in (0..report.terms.len()).rev() {
        if len(&output) <= max_chars {
            break;
        }
        avoids[index] = false;
        output = render(&include, &definitions, &avoids);
    }
    for mentioned in [false, true] {
        for (index, term) in report.terms.iter().enumerate().rev() {
            if len(&output) <= max_chars {
                break;
            }
            if term.mentioned_in_paths == mentioned {
                definitions[index] = false;
                output = render(&include, &definitions, &avoids);
            }
        }
    }
    for index in (0..report.terms.len()).rev() {
        if len(&output) <= max_chars {
            break;
        }
        include[index] = false;
        output = render(&include, &definitions, &avoids);
    }
    if len(&output) > max_chars {
        return Err(ContextError::Budget);
    }
    Ok(output)
}
