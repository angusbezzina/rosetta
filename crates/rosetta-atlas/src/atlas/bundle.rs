//! `rosetta.atlas-bundle/v1`: everything a viewer needs to show an atlas,
//! built from the model, the glossary, the rulebook and the live walk.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use super::emit;
use super::history::Updated;
use super::model::{
    Atlas, CONNECTIONS_VIEW, DATA_VIEW, Edge, Family, INFRA_VIEW, NodeKind, OVERVIEW_VIEW,
    RECORDS_VIEW, Tier, ViewKind,
};
use super::records::RecordsModel;
use super::walk::LiveNode;
use crate::glossary::{Glossary, GlossaryTerm, TermStatus};
use crate::rulebook::Rulebook;

pub const BUNDLE_SCHEMA: &str = "rosetta.atlas-bundle/v1";
pub const LEGACY_BUNDLE_SCHEMA: &str = "virgil.atlas-bundle/v1";

fn deserialize_schema<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<String, D::Error> {
    let schema = String::deserialize(deserializer)?;
    if schema == BUNDLE_SCHEMA || schema == LEGACY_BUNDLE_SCHEMA {
        Ok(schema)
    } else {
        Err(serde::de::Error::custom(format!(
            "expected {BUNDLE_SCHEMA} or {LEGACY_BUNDLE_SCHEMA}"
        )))
    }
}

#[cfg(test)]
mod schema_tests {
    use super::*;

    #[test]
    fn reads_current_and_legacy_bundle_ids() -> Result<(), Box<dyn std::error::Error>> {
        let current = include_str!("../../tests/fixtures/out/bundle.json");
        let bundle: Bundle = serde_json::from_str(current)?;
        assert_eq!(bundle.schema, BUNDLE_SCHEMA);
        let legacy = current.replace(BUNDLE_SCHEMA, LEGACY_BUNDLE_SCHEMA);
        let bundle: Bundle = serde_json::from_str(&legacy)?;
        assert_eq!(bundle.schema, LEGACY_BUNDLE_SCHEMA);
        let unknown = current.replace(BUNDLE_SCHEMA, "other.atlas-bundle/v1");
        assert!(serde_json::from_str::<Bundle>(&unknown).is_err());
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Bundle {
    #[serde(deserialize_with = "deserialize_schema")]
    pub schema: String,
    pub project: BundleProject,
    pub views: Vec<BundleView>,
    pub nodes: Vec<BundleNode>,
    pub edges: Vec<Edge>,
    pub terms: Vec<BundleTerm>,
    pub rules: Vec<BundleRule>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub capabilities: Vec<super::model::Capability>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub path_scopes: Vec<rosetta_core::config::PathScope>,
    /// Filesystem checks are observed facts; capability links remain curation.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub target_exists: BTreeMap<String, bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snapshot: Option<rosetta_core::snapshot::Snapshot>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BundleProject {
    pub name: String,
    pub repo: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BundleView {
    pub id: String,
    pub kind: ViewKind,
    pub title: String,
    pub description: String,
    /// The root a view opens on; empty for a view whose "" source is its
    /// picture, and for connections, which opens only on a part.
    pub default_root: String,
    /// Mermaid text keyed by root id: "" for the view without a root, a node
    /// id for each part a view with roots draws.
    pub sources: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct BundleRefs {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub term: Option<String>,
    pub rules: Vec<String>,
    pub principles: Vec<String>,
    pub docs: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BundleNode {
    pub id: String,
    pub label: String,
    pub family: Family,
    pub kind: NodeKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lines: Option<u64>,
    pub has_children: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tier: Option<Tier>,
    /// The newest commit that touched the part's path; absent for a part
    /// with no path or none in the history read.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated: Option<Updated>,
    pub refs: BundleRefs,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BundleAvoid {
    pub word: String,
    pub except: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BundleTerm {
    pub id: String,
    pub name: String,
    pub definition: String,
    pub avoid: Vec<BundleAvoid>,
    pub status: TermStatus,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub contexts: Vec<String>,
    /// Whether a message draws this term's name with its card: settled, not
    /// a word any term lists under Avoid, and the first term of its name.
    pub marked: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RuleKind {
    Rule,
    Principle,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BundleRule {
    pub id: String,
    pub kind: RuleKind,
    pub title: String,
}

/// The model's nodes, the live ones, then the record areas and collections,
/// with `has_children` set.
pub fn bundle_nodes(
    atlas: &Atlas,
    live: &[LiveNode],
    records: Option<&RecordsModel>,
) -> Vec<BundleNode> {
    let mut nodes: Vec<BundleNode> = atlas
        .nodes
        .iter()
        .map(|node| BundleNode {
            id: node.id.clone(),
            label: node.label.clone(),
            family: node.family,
            kind: node.kind,
            parent: node.parent.clone(),
            path: node.path.clone(),
            summary: node.summary.clone(),
            lines: None,
            has_children: false,
            tier: node.tier,
            updated: None,
            refs: BundleRefs {
                term: node.refs.term.clone(),
                rules: node.refs.rules.clone(),
                principles: node.refs.principles.clone(),
                docs: node.refs.docs.clone(),
            },
        })
        .collect();
    nodes.extend(live.iter().map(|node| BundleNode {
        id: node.id.clone(),
        label: node.label.clone(),
        family: node.family,
        kind: node.kind,
        parent: Some(node.parent.clone()),
        path: Some(node.path.clone()),
        summary: node.summary.clone(),
        lines: node.lines,
        has_children: false,
        tier: None,
        updated: None,
        refs: BundleRefs::default(),
    }));
    if let Some(records) = records {
        nodes.extend(records.areas.iter().cloned());
        nodes.extend(records.collections.iter().cloned());
    }
    let parents: BTreeSet<String> = nodes.iter().filter_map(|n| n.parent.clone()).collect();
    for node in &mut nodes {
        node.has_children = parents.contains(&node.id);
    }
    nodes
}

pub fn build_bundle(
    atlas: &Atlas,
    glossary: &Glossary,
    rulebook: Option<&Rulebook>,
    live: &[LiveNode],
    records: Option<&RecordsModel>,
) -> Bundle {
    let nodes = bundle_nodes(atlas, live, records);
    let mut edges = atlas.edges.clone();
    if let Some(records) = records {
        edges.extend(records.edges.iter().cloned());
    }
    let built =
        |id: &str, kind: ViewKind, title: &str, description: String, source: String| BundleView {
            id: id.to_owned(),
            kind,
            title: title.to_owned(),
            description,
            default_root: String::new(),
            sources: BTreeMap::from([(String::new(), source)]),
        };
    // Built views come first in a fixed order, and only when they have
    // something to draw; the model's custom views follow in its order.
    let mut views: Vec<BundleView> = Vec::new();
    if let Some(source) = emit::overview(&nodes, &atlas.edges) {
        views.push(built(
            OVERVIEW_VIEW,
            ViewKind::Overview,
            "Overview",
            format!(
                "What {} is made of and how the parts talk.",
                atlas.project.name
            ),
            source,
        ));
    }
    if let Some(source) = emit::infra(&nodes, &atlas.edges) {
        views.push(built(
            INFRA_VIEW,
            ViewKind::Infra,
            "Infra",
            "The cloud resources the repository declares and what uses them.".to_owned(),
            source,
        ));
    }
    if let Some(records) = records {
        views.push(BundleView {
            id: RECORDS_VIEW.to_owned(),
            kind: ViewKind::Records,
            title: "Records".to_owned(),
            description: "The record collections by area, and the refs between them.".to_owned(),
            default_root: String::new(),
            sources: emit::records_sources(records),
        });
    }
    if let Some(source) = emit::data(&nodes, &atlas.edges) {
        views.push(built(
            DATA_VIEW,
            ViewKind::Data,
            "Data",
            "Where state lives and how it moves: reads, writes, publishes and deliveries."
                .to_owned(),
            source,
        ));
    }
    views.extend(atlas.views.iter().map(|view| {
        built(
            &view.id,
            view.kind,
            &view.title,
            view.description.clone(),
            emit::declared_source(view, &nodes, &atlas.edges),
        )
    }));
    views.push(BundleView {
        id: CONNECTIONS_VIEW.to_owned(),
        kind: ViewKind::Connections,
        title: "Connections".to_owned(),
        description: "What a part uses and what uses it.".to_owned(),
        default_root: String::new(),
        sources: emit::connections_sources(&nodes, &edges),
    });

    let terms = bundle_terms(&glossary.terms);

    let referenced: BTreeSet<&str> = atlas
        .nodes
        .iter()
        .flat_map(|node| node.refs.rules.iter().chain(&node.refs.principles))
        .map(String::as_str)
        .collect();
    let mut rules = Vec::new();
    if let Some(rulebook) = rulebook {
        rules.extend(
            rulebook
                .principles
                .iter()
                .filter(|entry| referenced.contains(entry.id.as_str()))
                .map(|entry| BundleRule {
                    id: entry.id.clone(),
                    kind: RuleKind::Principle,
                    title: entry.statement.clone(),
                }),
        );
        rules.extend(
            rulebook
                .rules
                .iter()
                .filter(|entry| referenced.contains(entry.id.as_str()))
                .map(|entry| BundleRule {
                    id: entry.id.clone(),
                    kind: RuleKind::Rule,
                    title: entry.statement.clone(),
                }),
        );
    }

    Bundle {
        schema: BUNDLE_SCHEMA.to_owned(),
        project: BundleProject {
            name: atlas.project.name.clone(),
            repo: atlas.project.repo.clone(),
        },
        views,
        nodes,
        edges,
        terms,
        rules,
        capabilities: atlas.capabilities.clone(),
        path_scopes: Vec::new(),
        target_exists: BTreeMap::new(),
        snapshot: None,
    }
}

/// The glossary as a bundle carries it, each term saying whether a message
/// marks it.
fn bundle_terms(terms: &[GlossaryTerm]) -> Vec<BundleTerm> {
    let avoided: BTreeSet<String> = terms
        .iter()
        .flat_map(|term| term.avoid_words())
        .map(|entry| entry.word.to_lowercase())
        .collect();
    let mut names = BTreeSet::new();
    terms
        .iter()
        .map(|term| BundleTerm {
            marked: {
                let name = term.name.trim().to_lowercase();
                term.status == TermStatus::Settled
                    && !name.is_empty()
                    && !avoided.contains(&name)
                    && names.insert(name)
            },
            id: term.id.clone(),
            name: term.name.clone(),
            definition: term.definition.clone(),
            avoid: term
                .avoid_words()
                .iter()
                .map(|entry| BundleAvoid {
                    word: entry.word.clone(),
                    except: entry.except.clone().unwrap_or_default(),
                })
                .collect(),
            status: term.status,
            contexts: term.contexts.clone(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::glossary::model::AvoidWord;

    fn term(name: &str, status: TermStatus, avoid: &[&str]) -> GlossaryTerm {
        GlossaryTerm {
            id: format!("term.{}", name.to_lowercase().replace(' ', "-")),
            name: name.to_owned(),
            section: "Product".to_owned(),
            definition: format!("What {name} means."),
            contexts: Vec::new(),
            avoid: Some(
                avoid
                    .iter()
                    .map(|word| AvoidWord {
                        word: (*word).to_owned(),
                        except: None,
                    })
                    .collect(),
            ),
            code: None,
            refs: None,
            status,
            decided: None,
            epic: None,
        }
    }

    fn marked(terms: &[GlossaryTerm]) -> Vec<&str> {
        let bundled = bundle_terms(terms);
        terms
            .iter()
            .zip(&bundled)
            .filter(|(_, bundled)| bundled.marked)
            .map(|(term, _)| term.name.as_str())
            .collect()
    }

    #[test]
    fn a_message_marks_settled_terms_and_never_an_avoided_word() {
        let terms = [
            term("Door", TermStatus::Settled, &[]),
            term("Group", TermStatus::Settled, &["workspace"]),
            term("Lantern", TermStatus::Proposed, &[]),
            term("Workspace", TermStatus::Settled, &[]),
        ];
        assert_eq!(marked(&terms), ["Door", "Group"]);
    }

    #[test]
    fn the_first_term_of_a_name_is_the_one_marked() {
        let terms = [
            term("Door", TermStatus::Settled, &[]),
            term("door", TermStatus::Settled, &[]),
            term(" ", TermStatus::Settled, &[]),
        ];
        assert_eq!(marked(&terms), ["Door"]);
    }
}
