//! The atlas's rules. Each finding names where it is (`nodes[door].refs.term`)
//! and why, so a person can go straight to the line.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path};

use super::model::{ATLAS_SCHEMA, Atlas, BUILT_VIEWS, NodeKind, VIRGIL_ATLAS_SCHEMA, ViewKind};
use super::records::OTHER_AREA;
use crate::text::is_kebab;

/// What the validator needs from outside the model.
pub struct AtlasContext<'a> {
    pub term_ids: BTreeSet<String>,
    pub rule_ids: BTreeSet<String>,
    pub principle_ids: BTreeSet<String>,
    pub rule_source: Option<String>,
    pub principle_source: Option<String>,
    pub path_exists: Box<dyn Fn(&str) -> bool + 'a>,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AtlasError {
    #[error("schema: must be \"{ATLAS_SCHEMA}\", got \"{0}\"")]
    Schema(String),
    #[error("nodes[{index}].id: \"{id}\" must be kebab-case")]
    NodeIdNotKebab { index: usize, id: String },
    #[error("nodes[{id}]: the id is used by more than one node")]
    DuplicateNode { id: String },
    #[error("nodes[{id}].parent: \"{parent}\" is not a node")]
    MissingParent { id: String, parent: String },
    #[error("nodes[{id}].parent: the parent chain loops back to {id}")]
    ParentCycle { id: String },
    #[error("nodes[{id}].kind: files are walked live and never committed")]
    CommittedFile { id: String },
    #[error("nodes[{id}].path: \"{path}\" does not exist")]
    MissingPath { id: String, path: String },
    #[error("nodes[{id}].refs.term: \"{term}\" is not in .rosetta/glossary.json")]
    MissingTerm { id: String, term: String },
    #[error("nodes[{id}].refs.rules: \"{rule}\" {reason}")]
    MissingRule {
        id: String,
        rule: String,
        reason: String,
    },
    #[error("nodes[{id}].refs.principles: \"{principle}\" {reason}")]
    MissingPrinciple {
        id: String,
        principle: String,
        reason: String,
    },
    #[error("nodes[{id}].refs.docs: \"{doc}\" does not exist")]
    MissingDoc { id: String, doc: String },
    #[error("edges[{index}].{end}: \"{node}\" is not a node")]
    MissingEdgeEnd {
        index: usize,
        end: &'static str,
        node: String,
    },
    #[error(
        "views[{index}].id: \"{id}\" must be kebab-case, unique and not the id of a built view (overview, infra, records, data or connections)"
    )]
    BadViewId { index: usize, id: String },
    #[error("views[{id}].include: a custom view lists the nodes it shows")]
    MissingInclude { id: String },
    #[error("views[{id}].steps: a flow view needs at least one step")]
    MissingSteps { id: String },
    #[error(
        "views[{id}].kind: the {kind} view is built from the nodes and edges; remove it, and for a picture of your own declare a view of kind \"custom\" with an include list"
    )]
    DeclaredBuilt { id: String, kind: &'static str },
    #[error("records.file: \"{0}\" does not exist")]
    MissingRecordsFile(String),
    #[error(
        "records.areas[{index}].id: \"{id}\" must be kebab-case, unique and not \"{OTHER_AREA}\""
    )]
    BadAreaId { index: usize, id: String },
    #[error("views[{id}].{field}: \"{node}\" is not a node")]
    MissingViewNode {
        id: String,
        field: String,
        node: String,
    },
    #[error("capabilities[{index}].id: \"{id}\" must be unique kebab-case")]
    BadCapabilityId { index: usize, id: String },
    #[error("capabilities[{id}].terms: \"{term}\" is not in the glossary")]
    MissingCapabilityTerm { id: String, term: String },
    #[error("capabilities[{id}].{field}: \"{part}\" is not an atlas node")]
    MissingCapabilityPart {
        id: String,
        field: String,
        part: String,
    },
    #[error(
        "capabilities[{id}].implementation[{index}].path: \"{path}\" must stay inside the repository"
    )]
    UnsafeImplementationPath {
        id: String,
        index: usize,
        path: String,
    },
}

fn ref_reason(source: &Option<String>, kind: &str) -> String {
    source.as_ref().map_or_else(
        || format!("has no configured {kind} provider; set refs.rulebook in .rosetta/rosetta.toml"),
        |path| format!("is not in {path}"),
    )
}

pub fn validate_atlas(atlas: &Atlas, context: &AtlasContext<'_>) -> Vec<AtlasError> {
    let mut errors = Vec::new();
    if atlas.schema != ATLAS_SCHEMA && atlas.schema != VIRGIL_ATLAS_SCHEMA {
        errors.push(AtlasError::Schema(atlas.schema.clone()));
    }

    let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
    for node in &atlas.nodes {
        *counts.entry(node.id.as_str()).or_default() += 1;
    }
    let ids: BTreeSet<&str> = counts.keys().copied().collect();
    let parents: BTreeMap<&str, &str> = atlas
        .nodes
        .iter()
        .filter_map(|node| Some((node.id.as_str(), node.parent.as_deref()?)))
        .collect();
    let mut reported_duplicates = BTreeSet::new();

    for (index, node) in atlas.nodes.iter().enumerate() {
        let id = node.id.clone();
        if !is_kebab(&node.id) {
            errors.push(AtlasError::NodeIdNotKebab {
                index,
                id: id.clone(),
            });
        }
        if counts.get(node.id.as_str()).copied().unwrap_or_default() > 1
            && reported_duplicates.insert(node.id.as_str())
        {
            errors.push(AtlasError::DuplicateNode { id: id.clone() });
        }
        if node.kind == NodeKind::File {
            errors.push(AtlasError::CommittedFile { id: id.clone() });
        }
        if let Some(parent) = &node.parent {
            if !ids.contains(parent.as_str()) {
                errors.push(AtlasError::MissingParent {
                    id: id.clone(),
                    parent: parent.clone(),
                });
            } else if loops(&parents, &node.id) {
                errors.push(AtlasError::ParentCycle { id: id.clone() });
            }
        }
        if let Some(path) = &node.path
            && !(context.path_exists)(path)
        {
            errors.push(AtlasError::MissingPath {
                id: id.clone(),
                path: path.clone(),
            });
        }
        if let Some(term) = &node.refs.term
            && !context.term_ids.contains(term)
        {
            errors.push(AtlasError::MissingTerm {
                id: id.clone(),
                term: term.clone(),
            });
        }
        for rule in &node.refs.rules {
            if !context.rule_ids.contains(rule) {
                errors.push(AtlasError::MissingRule {
                    id: id.clone(),
                    rule: rule.clone(),
                    reason: ref_reason(&context.rule_source, "rules"),
                });
            }
        }
        for principle in &node.refs.principles {
            if !context.principle_ids.contains(principle) {
                errors.push(AtlasError::MissingPrinciple {
                    id: id.clone(),
                    principle: principle.clone(),
                    reason: ref_reason(&context.principle_source, "principles"),
                });
            }
        }
        for doc in &node.refs.docs {
            if !(context.path_exists)(doc) {
                errors.push(AtlasError::MissingDoc {
                    id: id.clone(),
                    doc: doc.clone(),
                });
            }
        }
    }

    for (index, edge) in atlas.edges.iter().enumerate() {
        for (end, node) in [("from", &edge.from), ("to", &edge.to)] {
            if !ids.contains(node.as_str()) {
                errors.push(AtlasError::MissingEdgeEnd {
                    index,
                    end,
                    node: node.clone(),
                });
            }
        }
    }

    let mut capability_ids = BTreeSet::new();
    for (index, capability) in atlas.capabilities.iter().enumerate() {
        if !is_kebab(&capability.id) || !capability_ids.insert(capability.id.as_str()) {
            errors.push(AtlasError::BadCapabilityId {
                index,
                id: capability.id.clone(),
            });
        }
        for term in &capability.terms {
            if !context.term_ids.contains(term) {
                errors.push(AtlasError::MissingCapabilityTerm {
                    id: capability.id.clone(),
                    term: term.clone(),
                });
            }
        }
        for part in &capability.parts {
            if !ids.contains(part.as_str()) {
                errors.push(AtlasError::MissingCapabilityPart {
                    id: capability.id.clone(),
                    field: "parts".to_owned(),
                    part: part.clone(),
                });
            }
        }
        for (position, target) in capability.implementation.iter().enumerate() {
            if !ids.contains(target.part.as_str()) {
                errors.push(AtlasError::MissingCapabilityPart {
                    id: capability.id.clone(),
                    field: format!("implementation[{position}].part"),
                    part: target.part.clone(),
                });
            }
            if target.path.is_empty()
                || !Path::new(&target.path)
                    .components()
                    .all(|component| matches!(component, Component::Normal(_)))
            {
                errors.push(AtlasError::UnsafeImplementationPath {
                    id: capability.id.clone(),
                    index: position,
                    path: target.path.clone(),
                });
            }
        }
    }

    let mut view_ids = BTreeSet::new();
    for (index, view) in atlas.views.iter().enumerate() {
        if !is_kebab(&view.id)
            || BUILT_VIEWS.contains(&view.id.as_str())
            || !view_ids.insert(view.id.as_str())
        {
            errors.push(AtlasError::BadViewId {
                index,
                id: view.id.clone(),
            });
        }
        let missing = |field: String, node: &str| AtlasError::MissingViewNode {
            id: view.id.clone(),
            field,
            node: node.to_owned(),
        };
        match view.kind {
            ViewKind::Custom => match &view.include {
                None => errors.push(AtlasError::MissingInclude {
                    id: view.id.clone(),
                }),
                Some(include) => {
                    for (position, node) in include.iter().enumerate() {
                        if !ids.contains(node.as_str()) {
                            errors.push(missing(format!("include[{position}]"), node));
                        }
                    }
                }
            },
            ViewKind::Flow => match view.steps.as_deref() {
                None | Some([]) => errors.push(AtlasError::MissingSteps {
                    id: view.id.clone(),
                }),
                Some(steps) => {
                    for (position, step) in steps.iter().enumerate() {
                        if !ids.contains(step.node.as_str()) {
                            errors.push(missing(format!("steps[{position}].node"), &step.node));
                        }
                    }
                }
            },
            built => errors.push(AtlasError::DeclaredBuilt {
                id: view.id.clone(),
                kind: built.as_str(),
            }),
        }
    }
    if let Some(records) = &atlas.records {
        if !(context.path_exists)(&records.file) {
            errors.push(AtlasError::MissingRecordsFile(records.file.clone()));
        }
        let mut area_ids = BTreeSet::new();
        for (index, area) in records.areas.iter().enumerate() {
            if !is_kebab(&area.id) || area.id == OTHER_AREA || !area_ids.insert(area.id.as_str()) {
                errors.push(AtlasError::BadAreaId {
                    index,
                    id: area.id.clone(),
                });
            }
        }
    }
    errors
}

fn loops(parents: &BTreeMap<&str, &str>, start: &str) -> bool {
    let mut seen = BTreeSet::new();
    let mut at = start;
    while let Some(parent) = parents.get(at) {
        if *parent == start {
            return true;
        }
        if !seen.insert(*parent) {
            return false;
        }
        at = parent;
    }
    false
}
