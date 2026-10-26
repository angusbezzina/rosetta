//! Path impact and a repository-grounded before/after review. Manifest and
//! source derivation describe structure, never claimed runtime behaviour.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path};
use std::process::Command;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::bundle::{Bundle, BundleNode};
use super::model::{Atlas, CONNECTIONS_VIEW, Edge, EdgeKind, NodeKind};
use super::{AtlasLoadError, derive};
use crate::glossary::Glossary;
use crate::rulebook::Rulebook;
use rosetta_core::snapshot::{self, Snapshot};

pub const CHANGED_SCHEMA: &str = "rosetta.changed/v1";
pub const IMPACT_SCHEMA: &str = "rosetta.impact/v1";
pub const INTENT_SCHEMA: &str = "rosetta.impact-proposal/v1";

#[derive(Debug, thiserror::Error)]
pub enum ChangeError {
    #[error(transparent)]
    Atlas(#[from] AtlasLoadError),
    #[error(transparent)]
    Derive(#[from] derive::DeriveError),
    #[error("{0}: {1}")]
    Io(String, #[source] std::io::Error),
    #[error("git {command}: {why}")]
    Git { command: String, why: String },
    #[error("{0} is not a repository-relative path")]
    Path(String),
    #[error("baseline {0} has no Atlas source; semantic comparison is unknown")]
    NoBaseline(String),
    #[error("{0}: {1}")]
    Json(String, #[source] serde_json::Error),
    #[error("the proposal was prepared for baseline {expected}; selected baseline is {current}")]
    StaleBaseline { expected: String, current: String },
    #[error("the proposal was prepared for model {expected}; baseline model is {current}")]
    StaleModel { expected: String, current: String },
    #[error("expected proposal schema {INTENT_SCHEMA}")]
    ProposalSchema,
}

#[derive(Debug, Clone, Serialize)]
pub struct ImpactedPart {
    pub id: String,
    pub name: String,
    pub path: Option<String>,
    pub responsibility: Option<String>,
    pub kind: String,
    pub capabilities: Vec<String>,
    pub incoming: Vec<Edge>,
    pub outgoing: Vec<Edge>,
    pub connections_diagram: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ImpactReport {
    pub schema: &'static str,
    pub path: String,
    pub snapshot: Snapshot,
    pub parts: Vec<ImpactedPart>,
    pub coverage: &'static str,
}

fn safe_path(path: &str) -> bool {
    !path.is_empty()
        && Path::new(path)
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
}

fn covers(candidate: &str, path: &str) -> bool {
    candidate == "." || path == candidate || path.starts_with(&format!("{candidate}/"))
}

pub fn impact(bundle: &Bundle, path: &str) -> Result<ImpactReport, ChangeError> {
    if !safe_path(path) {
        return Err(ChangeError::Path(path.to_owned()));
    }
    let snapshot = bundle
        .snapshot
        .clone()
        .ok_or_else(|| ChangeError::Path("bundle has no snapshot".into()))?;
    let matching = |node: &&BundleNode| {
        node.path
            .as_deref()
            .is_some_and(|candidate| covers(candidate, path))
    };
    let mut candidates: Vec<&BundleNode> = bundle
        .nodes
        .iter()
        .filter(|node| node.kind.is_part())
        .filter(matching)
        .collect();
    if candidates.is_empty() {
        candidates = bundle
            .nodes
            .iter()
            .filter(|node| {
                matches!(
                    node.kind,
                    NodeKind::App | NodeKind::Package | NodeKind::Crate | NodeKind::Resource
                )
            })
            .filter(matching)
            .collect();
    }
    if candidates.is_empty() {
        candidates = bundle
            .nodes
            .iter()
            .filter(|node| node.kind == NodeKind::Directory)
            .filter(matching)
            .collect();
    }
    let most_specific = candidates
        .iter()
        .map(|node| node.path.as_ref().map_or(0, String::len))
        .max()
        .unwrap_or(0);
    let mut parts: Vec<ImpactedPart> = candidates
        .into_iter()
        .filter(|node| node.path.as_ref().map_or(0, String::len) == most_specific)
        .map(|node| {
            let mut incoming: Vec<Edge> = bundle
                .edges
                .iter()
                .filter(|edge| edge.to == node.id)
                .cloned()
                .collect();
            let mut outgoing: Vec<Edge> = bundle
                .edges
                .iter()
                .filter(|edge| edge.from == node.id)
                .cloned()
                .collect();
            incoming.sort_by(|a, b| {
                (&a.from, &a.to, a.kind, &a.label).cmp(&(&b.from, &b.to, b.kind, &b.label))
            });
            outgoing.sort_by(|a, b| {
                (&a.from, &a.to, a.kind, &a.label).cmp(&(&b.from, &b.to, b.kind, &b.label))
            });
            ImpactedPart {
                id: node.id.clone(),
                name: node.label.clone(),
                path: node.path.clone(),
                responsibility: node.summary.clone(),
                kind: node.kind.as_str().into(),
                capabilities: bundle
                    .capabilities
                    .iter()
                    .filter(|capability| capability.parts.contains(&node.id))
                    .map(|capability| capability.id.clone())
                    .collect(),
                incoming,
                outgoing,
                connections_diagram: bundle
                    .views
                    .iter()
                    .find(|view| view.id == CONNECTIONS_VIEW)
                    .and_then(|view| view.sources.get(&node.id).cloned()),
            }
        })
        .collect();
    parts.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(ImpactReport {
        schema: IMPACT_SCHEMA,
        path: path.into(),
        snapshot,
        coverage: if parts.is_empty() {
            "no mapped part"
        } else {
            "mapped part and direct declared or derived edges"
        },
        parts,
    })
}

fn git(root: &Path, args: &[&str]) -> Result<Vec<u8>, ChangeError> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .map_err(|error| ChangeError::Io("git".into(), error))?;
    if output.status.success() {
        Ok(output.stdout)
    } else {
        Err(ChangeError::Git {
            command: args.join(" "),
            why: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
        })
    }
}

fn review_bundle(
    root: &Path,
    source_dir: &str,
    fresh: &Atlas,
    glossary: &Glossary,
) -> Result<Bundle, ChangeError> {
    let config = crate::config::Config::load(root, None).map_err(AtlasLoadError::from)?;
    let rulebook = Rulebook::load_optional_for(root, &config).map_err(AtlasLoadError::from)?;
    let files = super::walk::tracked_files(root).map_err(AtlasLoadError::from)?;
    let live = super::walk::walk(root, &super::walk_roots(fresh, root)?, &files);
    let records = fresh
        .records
        .as_ref()
        .map(|section| super::records::read_records(root, section, &config.records))
        .transpose()
        .map_err(AtlasLoadError::from)?;
    let mut bundle =
        super::build_bundle(fresh, glossary, rulebook.as_ref(), &live, records.as_ref());
    super::decorate_bundle(root, source_dir, &mut bundle)?;
    Ok(bundle)
}

pub fn impact_repo(root: &Path, source_dir: &str, path: &str) -> Result<ImpactReport, ChangeError> {
    let current = super::load_atlas_from(root, source_dir)?;
    let glossary = super::load_glossary_terms_from(root, source_dir)?;
    let fresh = derive::sync(&current, &derive::derive(root, &current)?);
    impact(&review_bundle(root, source_dir, &fresh, &glossary)?, path)
}

fn baseline_checkout(root: &Path, commit: &str) -> Result<tempfile::TempDir, ChangeError> {
    let temp =
        tempfile::tempdir().map_err(|error| ChangeError::Io("baseline scratch".into(), error))?;
    let output = Command::new("git")
        .args(["clone", "--quiet", "--shared", "--no-checkout", "--"])
        .arg(root)
        .arg(temp.path())
        .output()
        .map_err(|error| ChangeError::Io("git clone".into(), error))?;
    if !output.status.success() {
        return Err(ChangeError::Git {
            command: "clone --shared --no-checkout".into(),
            why: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
        });
    }
    git(
        temp.path(),
        &[
            "-c",
            "filter.lfs.smudge=",
            "checkout",
            "--quiet",
            "--detach",
            commit,
        ],
    )?;
    Ok(temp)
}

#[derive(Debug, Clone, Serialize)]
pub struct SemanticDelta {
    pub category: String,
    pub id: String,
    pub field: String,
    pub origin: String,
    pub before: Value,
    pub after: Value,
    pub evidence: Option<String>,
}

#[expect(
    clippy::too_many_arguments,
    reason = "a semantic delta records its identity, before/after values and evidence together"
)]
fn delta(
    out: &mut Vec<SemanticDelta>,
    category: &str,
    id: &str,
    field: &str,
    origin: &str,
    before: Value,
    after: Value,
    evidence: Option<String>,
) {
    if before != after {
        out.push(SemanticDelta {
            category: category.into(),
            id: id.into(),
            field: field.into(),
            origin: origin.into(),
            before,
            after,
            evidence,
        });
    }
}

fn edge_source(edge: &Edge, observed: &derive::DerivedModel) -> Option<String> {
    if let Some(evidence) = &edge.evidence {
        return Some(evidence.source.clone());
    }
    let source = &edge.derived.as_ref()?.source;
    if edge.kind != EdgeKind::Depends || source.contains('/') {
        return Some(source.clone());
    }
    observed
        .nodes
        .iter()
        .find(|node| node.id == edge.from)
        .and_then(|node| node.path.as_ref())
        .map(|path| format!("{path}/{source}"))
        .or_else(|| Some(source.clone()))
}

fn model_deltas(
    before: &Atlas,
    baseline_observed: &derive::DerivedModel,
    current: &Atlas,
    current_observed: &derive::DerivedModel,
    before_glossary: &Glossary,
    glossary: &Glossary,
) -> Vec<SemanticDelta> {
    let mut out = Vec::new();
    let before_nodes: BTreeMap<&str, _> = before
        .nodes
        .iter()
        .map(|node| (node.id.as_str(), node))
        .collect();
    let current_nodes: BTreeMap<&str, _> = current
        .nodes
        .iter()
        .map(|node| (node.id.as_str(), node))
        .collect();
    let fresh_nodes: BTreeMap<&str, _> = current_observed
        .nodes
        .iter()
        .map(|node| (node.id.as_str(), node))
        .collect();
    for id in before_nodes
        .keys()
        .chain(current_nodes.keys())
        .copied()
        .collect::<BTreeSet<_>>()
    {
        match (before_nodes.get(id), current_nodes.get(id)) {
            (Some(old), Some(new)) => {
                delta(
                    &mut out,
                    "part",
                    id,
                    "label",
                    "model_source",
                    json!(old.label),
                    json!(new.label),
                    None,
                );
                delta(
                    &mut out,
                    "part",
                    id,
                    "responsibility",
                    "model_source",
                    json!(old.summary),
                    json!(new.summary),
                    None,
                );
                delta(
                    &mut out,
                    "part",
                    id,
                    "term",
                    "model_source",
                    json!(old.refs.term),
                    json!(new.refs.term),
                    None,
                );
            }
            (Some(old), None) if old.derived.is_none() => delta(
                &mut out,
                "part",
                id,
                "whole",
                "model_source",
                json!(old),
                Value::Null,
                None,
            ),
            (None, Some(new)) if new.derived.is_none() => delta(
                &mut out,
                "part",
                id,
                "whole",
                "model_source",
                Value::Null,
                json!(new),
                None,
            ),
            _ => {}
        }
    }
    let baseline_fresh_nodes: BTreeMap<&str, _> = baseline_observed
        .nodes
        .iter()
        .map(|node| (node.id.as_str(), node))
        .collect();
    for id in baseline_fresh_nodes
        .keys()
        .chain(fresh_nodes.keys())
        .copied()
        .collect::<BTreeSet<_>>()
    {
        let old = baseline_fresh_nodes
            .get(id)
            .filter(|node| node.derived.is_some());
        let new = fresh_nodes.get(id).filter(|node| node.derived.is_some());
        if old.is_none() && new.is_none() {
            continue;
        }
        if old.is_none() != new.is_none() {
            delta(
                &mut out,
                "part",
                id,
                "observed_structure",
                "derived",
                json!(old),
                json!(new),
                new.and_then(|node| node.derived.as_ref().map(|source| source.source.clone()))
                    .or_else(|| {
                        old.and_then(|node| {
                            node.derived.as_ref().map(|source| source.source.clone())
                        })
                    }),
            );
        }
    }
    let before_caps: BTreeMap<&str, _> = before
        .capabilities
        .iter()
        .map(|capability| (capability.id.as_str(), capability))
        .collect();
    let current_caps: BTreeMap<&str, _> = current
        .capabilities
        .iter()
        .map(|capability| (capability.id.as_str(), capability))
        .collect();
    for id in before_caps
        .keys()
        .chain(current_caps.keys())
        .copied()
        .collect::<BTreeSet<_>>()
    {
        match (before_caps.get(id), current_caps.get(id)) {
            (Some(old), Some(new)) => {
                for (field, before, after) in [
                    ("name", json!(old.name), json!(new.name)),
                    ("purpose", json!(old.purpose), json!(new.purpose)),
                    ("status", json!(old.status), json!(new.status)),
                    ("terms", json!(old.terms), json!(new.terms)),
                    ("parts", json!(old.parts), json!(new.parts)),
                    (
                        "implementation",
                        json!(old.implementation),
                        json!(new.implementation),
                    ),
                ] {
                    delta(
                        &mut out,
                        "capability",
                        id,
                        field,
                        "model_source",
                        before,
                        after,
                        None,
                    );
                }
            }
            (Some(old), None) => delta(
                &mut out,
                "capability",
                id,
                "whole",
                "model_source",
                json!(old),
                Value::Null,
                None,
            ),
            (None, Some(new)) => delta(
                &mut out,
                "capability",
                id,
                "whole",
                "model_source",
                Value::Null,
                json!(new),
                None,
            ),
            _ => {}
        }
    }
    let old_terms: BTreeMap<&str, _> = before_glossary
        .terms
        .iter()
        .map(|term| (term.id.as_str(), term))
        .collect();
    let new_terms: BTreeMap<&str, _> = glossary
        .terms
        .iter()
        .map(|term| (term.id.as_str(), term))
        .collect();
    for id in old_terms
        .keys()
        .chain(new_terms.keys())
        .copied()
        .collect::<BTreeSet<_>>()
    {
        match (old_terms.get(id), new_terms.get(id)) {
            (Some(old), Some(new)) => {
                for (field, before, after) in [
                    ("name", json!(old.name), json!(new.name)),
                    ("definition", json!(old.definition), json!(new.definition)),
                    ("status", json!(old.status), json!(new.status)),
                    ("contexts", json!(old.contexts), json!(new.contexts)),
                    ("avoid", json!(old.avoid), json!(new.avoid)),
                ] {
                    delta(
                        &mut out,
                        "term",
                        id,
                        field,
                        "model_source",
                        before,
                        after,
                        None,
                    );
                }
            }
            (Some(old), None) => delta(
                &mut out,
                "term",
                id,
                "whole",
                "model_source",
                json!(old),
                Value::Null,
                None,
            ),
            (None, Some(new)) => delta(
                &mut out,
                "term",
                id,
                "whole",
                "model_source",
                Value::Null,
                json!(new),
                None,
            ),
            _ => {}
        }
    }
    let edge_key = |edge: &Edge| {
        (
            edge.from.clone(),
            edge.to.clone(),
            edge.kind,
            edge.label.clone(),
        )
    };
    for (origin, old_edges, new_edges) in [
        (
            "model_source",
            before
                .edges
                .iter()
                .filter(|edge| edge.derived.is_none())
                .collect::<Vec<_>>(),
            current
                .edges
                .iter()
                .filter(|edge| edge.derived.is_none())
                .collect::<Vec<_>>(),
        ),
        (
            "derived",
            baseline_observed
                .edges
                .iter()
                .filter(|edge| edge.derived.is_some())
                .collect::<Vec<_>>(),
            current_observed
                .edges
                .iter()
                .filter(|edge| edge.derived.is_some())
                .collect::<Vec<_>>(),
        ),
    ] {
        let old: BTreeMap<_, _> = old_edges
            .into_iter()
            .map(|edge| (edge_key(edge), edge))
            .collect();
        let new: BTreeMap<_, _> = new_edges
            .into_iter()
            .map(|edge| (edge_key(edge), edge))
            .collect();
        for key in old
            .keys()
            .chain(new.keys())
            .cloned()
            .collect::<BTreeSet<_>>()
        {
            let previous = old.get(&key);
            let next = new.get(&key);
            if previous.is_some() != next.is_some() {
                let evidence = next
                    .and_then(|edge| edge_source(edge, current_observed))
                    .or_else(|| previous.and_then(|edge| edge_source(edge, baseline_observed)));
                delta(
                    &mut out,
                    "relationship",
                    &format!("{} -> {} ({})", key.0, key.1, key.2.as_str()),
                    "whole",
                    origin,
                    json!(previous),
                    json!(next),
                    evidence,
                );
            }
        }
    }
    out.sort_by(|a, b| {
        (&a.category, &a.id, &a.field, &a.origin).cmp(&(&b.category, &b.id, &b.field, &b.origin))
    });
    out
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IntentProposal {
    pub schema: String,
    pub base_commit: String,
    pub base_model_revision: String,
    pub intents: Vec<Intent>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Intent {
    Relationship {
        from: String,
        to: String,
        relation: EdgeKind,
        present: bool,
        rationale: String,
    },
    CapabilityPart {
        capability: String,
        part: String,
        present: bool,
        rationale: String,
    },
    Responsibility {
        part: String,
        value: String,
        rationale: String,
    },
    TermDefinition {
        term: String,
        value: String,
        rationale: String,
    },
    Claim {
        text: String,
        rationale: String,
    },
}

#[derive(Debug, Clone, Serialize)]
pub struct IntentAssessment {
    pub intent: Intent,
    pub status: String,
    pub why: String,
    pub evidence: Option<String>,
}

fn describe_intent(intent: &Intent) -> String {
    match intent {
        Intent::Relationship {
            from,
            to,
            relation,
            present,
            rationale,
        } => format!(
            "{} {} {} ({}) · proposed reason: {:?}",
            from,
            relation.as_str(),
            to,
            if *present { "add" } else { "remove" },
            rationale
        ),
        Intent::CapabilityPart {
            capability,
            part,
            present,
            rationale,
        } => format!(
            "{} {} capability {} · proposed reason: {:?}",
            if *present { "link" } else { "unlink" },
            part,
            capability,
            rationale
        ),
        Intent::Responsibility {
            part,
            value,
            rationale,
        } => format!(
            "responsibility of {} = {:?} · proposed reason: {:?}",
            part, value, rationale
        ),
        Intent::TermDefinition {
            term,
            value,
            rationale,
        } => format!(
            "definition of {} = {:?} · proposed reason: {:?}",
            term, value, rationale
        ),
        Intent::Claim { text, rationale } => {
            format!("claim {:?} · proposed reason: {:?}", text, rationale)
        }
    }
}

fn assess(
    intents: &[Intent],
    baseline_observed: &derive::DerivedModel,
    current: &Atlas,
    current_observed: &derive::DerivedModel,
    glossary: &Glossary,
) -> Vec<IntentAssessment> {
    intents.iter().map(|intent| {
        let (status, why, evidence) = match intent {
            Intent::Relationship { from,to,relation,present,.. } => {
                let old = baseline_observed.edges.iter().any(|edge| edge.from == *from && edge.to == *to && edge.kind == *relation && edge.derived.is_some());
                let derived = current_observed.edges.iter().find(|edge| edge.from == *from && edge.to == *to && edge.kind == *relation && edge.derived.is_some());
                let curated = current.edges.iter().any(|edge| edge.from == *from && edge.to == *to && edge.kind == *relation && edge.derived.is_none());
                let covered = *relation == EdgeKind::Depends
                    && current_observed.members.contains(from)
                    && current_observed.members.contains(to);
                if covered && *present == derived.is_some() && old != *present {
                    ("implemented", "A supported structural extractor observed the requested relationship change.", derived.and_then(|edge| edge_source(edge, current_observed)))
                } else if curated {
                    ("unknown", "The relationship is declared in the model, but no supported extractor proves runtime behavior.", None)
                } else if covered {
                    ("still_proposed", "The supported structural relationship change was not observed.", None)
                } else {
                    ("unknown", "This relationship or endpoint has no complete structural extractor; its absence cannot be verified.", None)
                }
            }
            Intent::CapabilityPart { capability,part,present,.. } => {
                let found = current.capabilities.iter().find(|entry| entry.id == *capability).is_some_and(|entry| entry.parts.contains(part));
                if found == *present { ("declared_in_model", "The current source model maps this capability to the part; behavior is not inferred.", Some(format!(".rosetta/atlas.json#capabilities[{capability}].parts"))) }
                else { ("still_proposed", "The current source model does not contain this mapping.", None) }
            }
            Intent::Responsibility { part,value,.. } => {
                let found = current.nodes.iter().find(|node| node.id == *part).and_then(|node| node.summary.as_deref()) == Some(value.as_str());
                if found { ("declared_in_model", "The current source model records this responsibility; code behavior remains unverified.", Some(format!(".rosetta/atlas.json#nodes[{part}].summary"))) }
                else { ("still_proposed", "The current source model does not contain this responsibility.", None) }
            }
            Intent::TermDefinition { term,value,.. } => {
                let found = glossary.terms.iter().find(|entry| entry.id == *term).is_some_and(|entry| entry.definition == *value);
                if found { ("declared_in_model", "The current glossary source records this definition.", Some(format!(".rosetta/glossary.json#terms[{term}].definition"))) }
                else { ("still_proposed", "The current glossary source does not contain this definition.", None) }
            }
            Intent::Claim { .. } => ("unknown", "This prose claim has no supported structural assertion or source extractor.", None),
        };
        IntentAssessment { intent: intent.clone(), status: status.into(), why: why.into(), evidence }
    }).collect()
}

#[derive(Debug, Clone, Serialize)]
pub struct ChangedReport {
    pub schema: &'static str,
    pub base_commit: String,
    pub base_model_revision: String,
    pub snapshot: Snapshot,
    pub changed_paths: Vec<String>,
    pub impacts: Vec<ImpactReport>,
    pub semantic: Vec<SemanticDelta>,
    pub proposed: Vec<IntentAssessment>,
    pub unexpected: Vec<SemanticDelta>,
    pub coverage: Vec<String>,
}

pub fn changed(
    root: &Path,
    source_dir: &str,
    base: &str,
    proposal: Option<&IntentProposal>,
) -> Result<ChangedReport, ChangeError> {
    let commit = String::from_utf8_lossy(&git(
        root,
        &[
            "rev-parse",
            "--verify",
            "--end-of-options",
            &format!("{base}^{{commit}}"),
        ],
    )?)
    .trim()
    .to_owned();
    let baseline = baseline_checkout(root, &commit)?;
    let baseline_root = baseline.path();
    let baseline_source = format!("{source_dir}/atlas.json");
    if !baseline_root.join(&baseline_source).is_file() {
        return Err(ChangeError::NoBaseline(baseline_source));
    }
    let before = super::load_atlas_from(baseline_root, source_dir)?;
    let baseline_observed = derive::derive(baseline_root, &before)?;
    let before_glossary_path = format!("{source_dir}/glossary.json");
    let before_glossary = baseline_root
        .join(&before_glossary_path)
        .is_file()
        .then(|| super::load_glossary_terms_from(baseline_root, source_dir))
        .transpose()?;
    let revision = snapshot::model_revision(baseline_root, source_dir);
    if let Some(proposal) = proposal {
        if proposal.schema != INTENT_SCHEMA {
            return Err(ChangeError::ProposalSchema);
        }
        if proposal.base_commit != commit {
            return Err(ChangeError::StaleBaseline {
                expected: proposal.base_commit.clone(),
                current: commit,
            });
        }
        if proposal.base_model_revision != revision {
            return Err(ChangeError::StaleModel {
                expected: proposal.base_model_revision.clone(),
                current: revision,
            });
        }
    }
    let current = super::load_atlas_from(root, source_dir)?;
    let glossary = super::load_glossary_terms_from(root, source_dir)?;
    let current_observed = derive::derive(root, &current)?;
    let fresh = derive::sync(&current, &current_observed);
    let bundle = review_bundle(root, source_dir, &fresh, &glossary)?;
    let snapshot = bundle
        .snapshot
        .clone()
        .ok_or_else(|| ChangeError::Path("bundle has no snapshot".into()))?;
    let mut paths: BTreeSet<String> = git(root, &["diff", "--name-only", "-z", &commit, "--"])?
        .split(|byte| *byte == 0)
        .filter(|item| !item.is_empty())
        .map(|item| String::from_utf8_lossy(item).into_owned())
        .collect();
    paths.extend(
        git(root, &["ls-files", "--others", "--exclude-standard", "-z"])?
            .split(|byte| *byte == 0)
            .filter(|item| !item.is_empty())
            .map(|item| String::from_utf8_lossy(item).into_owned()),
    );
    let changed_paths: Vec<String> = paths.into_iter().collect();
    let impacts: Vec<ImpactReport> = changed_paths
        .iter()
        .filter_map(|path| impact(&bundle, path).ok())
        .collect();
    let semantic = model_deltas(
        &before,
        &baseline_observed,
        &current,
        &current_observed,
        &before_glossary.clone().unwrap_or_else(|| Glossary {
            schema: glossary.schema.clone(),
            context: glossary.context.clone(),
            purpose: String::new(),
            sections: Vec::new(),
            terms: Vec::new(),
        }),
        &glossary,
    );
    let proposed = proposal.map_or_else(Vec::new, |proposal| {
        assess(
            &proposal.intents,
            &baseline_observed,
            &current,
            &current_observed,
            &glossary,
        )
    });
    let matched_relationships: BTreeSet<String> = proposed
        .iter()
        .filter(|entry| entry.status == "implemented")
        .filter_map(|entry| match &entry.intent {
            Intent::Relationship {
                from, to, relation, ..
            } => Some(format!("{from} -> {to} ({})", relation.as_str())),
            _ => None,
        })
        .collect();
    let unexpected: Vec<SemanticDelta> = semantic
        .iter()
        .filter(|delta| delta.origin == "derived" && !matched_relationships.contains(&delta.id))
        .cloned()
        .collect();
    let mut coverage = vec!["Baseline and current structure are each re-derived from their respective repository trees; source model edits are compared separately.".into(),"Manifest dependencies and containment are structural evidence; calls, data access, and capability behavior require other evidence.".into()];
    if before_glossary.is_none() {
        coverage.push(
            "The baseline has no glossary source; all current terms appear newly curated.".into(),
        );
    }
    Ok(ChangedReport {
        schema: CHANGED_SCHEMA,
        base_commit: commit,
        base_model_revision: revision,
        snapshot,
        changed_paths,
        impacts,
        semantic,
        proposed,
        unexpected,
        coverage,
    })
}

pub fn render_impact(report: &ImpactReport, markdown: bool) -> String {
    let mut text = if markdown {
        format!("## Impact: `{}`\n\n", report.path)
    } else {
        format!("Impact: {}\n", report.path)
    };
    if report.parts.is_empty() {
        text.push_str("No mapped part; coverage unknown for this path.\n");
        return text;
    }
    for part in &report.parts {
        text.push_str(&format!(
            "{}{} ({}) — {}\n",
            if markdown { "### " } else { "" },
            part.name,
            part.id,
            part.responsibility
                .as_deref()
                .unwrap_or("responsibility not curated")
        ));
        if !part.capabilities.is_empty() {
            text.push_str(&format!("Capabilities: {}\n", part.capabilities.join(", ")));
        }
        for edge in &part.incoming {
            text.push_str(&format!(
                "  {} -> {}: {} [{}]\n",
                edge.from,
                edge.to,
                edge.kind.as_str(),
                edge.evidence
                    .as_ref()
                    .map_or("source unknown", |evidence| evidence.source.as_str())
            ));
        }
        for edge in &part.outgoing {
            text.push_str(&format!(
                "  {} -> {}: {} [{}]\n",
                edge.from,
                edge.to,
                edge.kind.as_str(),
                edge.evidence
                    .as_ref()
                    .map_or("source unknown", |evidence| evidence.source.as_str())
            ));
        }
        if markdown && let Some(diagram) = &part.connections_diagram {
            text.push_str("\n```mermaid\n");
            text.push_str(&super::page::without_clicks(diagram));
            text.push_str("```\n\n");
        }
    }
    text
}

pub fn render_changed(report: &ChangedReport, markdown: bool) -> String {
    let mut text = if markdown {
        format!(
            "# Atlas change review\n\nBaseline: `{}`\n\n",
            report.base_commit
        )
    } else {
        format!("Atlas change review\nBaseline: {}\n", report.base_commit)
    };
    text.push_str(&format!(
        "Current snapshot: {}\nChanged paths: {}\n",
        report.snapshot.fingerprint,
        report.changed_paths.len()
    ));
    for impact in &report.impacts {
        text.push_str(&render_impact(impact, markdown));
    }
    text.push_str(if markdown {
        "\n## Semantic changes\n\n"
    } else {
        "Semantic changes:\n"
    });
    for delta in &report.semantic {
        text.push_str(&format!(
            "- {} {} {} [{}]: {} -> {}{}\n",
            delta.category,
            delta.id,
            delta.field,
            delta.origin,
            delta.before,
            delta.after,
            delta
                .evidence
                .as_ref()
                .map_or(String::new(), |source| format!(" · {source}"))
        ));
    }
    text.push_str(if markdown {
        "\n## Proposed versus observed\n\n"
    } else {
        "Proposed versus observed:\n"
    });
    for item in &report.proposed {
        text.push_str(&format!(
            "- {}: {} — {}{}\n",
            item.status,
            describe_intent(&item.intent),
            item.why,
            item.evidence
                .as_ref()
                .map_or(String::new(), |source| format!(" · {source}"))
        ));
    }
    text.push_str(&format!(
        "Unexpected structural changes: {}\n",
        report.unexpected.len()
    ));
    for delta in &report.unexpected {
        text.push_str(&format!(
            "- {} {} {}{}\n",
            delta.category,
            delta.id,
            delta.field,
            delta
                .evidence
                .as_ref()
                .map_or(String::new(), |source| format!(" · {source}"))
        ));
    }
    for note in &report.coverage {
        text.push_str(&format!("Coverage: {note}\n"));
    }
    text
}
