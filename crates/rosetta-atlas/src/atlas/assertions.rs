//! Small, evidence-bounded structural assertions for a local repository.
//! A declared edge is never substituted for one observed by an extractor.

use std::collections::BTreeSet;
use std::path::{Component, Path};

use serde::{Deserialize, Serialize};

use super::AtlasLoadError;
use super::derive::{self, DerivedModel};
use super::model::{Atlas, EdgeKind};
use crate::glossary::Glossary;
use rosetta_core::snapshot::{self, Snapshot};

pub const ASSERTIONS_SCHEMA: &str = "rosetta.assertions/v1";
pub const REPORT_SCHEMA: &str = "rosetta.assertion-report/v1";

#[derive(Debug, thiserror::Error)]
pub enum AssertionError {
    #[error(transparent)]
    Atlas(#[from] AtlasLoadError),
    #[error(transparent)]
    Derive(#[from] derive::DeriveError),
    #[error("{0}: {1}")]
    Io(String, #[source] std::io::Error),
    #[error("{0}: {1}")]
    Json(String, #[source] serde_json::Error),
    #[error("expected assertion schema {ASSERTIONS_SCHEMA}")]
    Schema,
    #[error("assertion id {0:?} is empty or repeated")]
    RuleId(String),
    #[error("{0}: assertion source exceeds 64 KiB")]
    TooLarge(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuleSet {
    pub schema: String,
    pub rules: Vec<Rule>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Rule {
    ImplementationExists {
        id: String,
        capability: String,
        #[serde(default)]
        part: Option<String>,
        #[serde(default)]
        term: Option<String>,
    },
    Dependencies {
        id: String,
        part: String,
        #[serde(default)]
        allowed: Option<Vec<String>>,
        #[serde(default)]
        forbidden: Vec<String>,
        #[serde(default)]
        term: Option<String>,
    },
    RequiredRelationship {
        id: String,
        from: String,
        to: String,
        relation: EdgeKind,
        #[serde(default)]
        term: Option<String>,
    },
}

impl Rule {
    fn id(&self) -> &str {
        match self {
            Rule::ImplementationExists { id, .. }
            | Rule::Dependencies { id, .. }
            | Rule::RequiredRelationship { id, .. } => id,
        }
    }

    fn term(&self) -> Option<&str> {
        match self {
            Rule::ImplementationExists { term, .. }
            | Rule::Dependencies { term, .. }
            | Rule::RequiredRelationship { term, .. } => term.as_deref(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Pass,
    Violation,
    Unknown,
}

#[derive(Debug, Clone, Serialize)]
pub struct Finding {
    pub id: String,
    pub status: Status,
    pub part: Option<String>,
    pub term: Option<String>,
    pub message: String,
    pub evidence: Vec<String>,
    pub coverage: String,
    pub repair: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Report {
    pub schema: &'static str,
    pub snapshot: Snapshot,
    pub findings: Vec<Finding>,
    pub coverage: Vec<String>,
}

fn finding(
    rule: &Rule,
    part: Option<&str>,
    status: Status,
    message: impl Into<String>,
    evidence: Vec<String>,
    coverage: impl Into<String>,
    repair: Option<String>,
) -> Finding {
    Finding {
        id: rule.id().to_owned(),
        status,
        part: part.map(str::to_owned),
        term: rule.term().map(str::to_owned),
        message: message.into(),
        evidence,
        coverage: coverage.into(),
        repair,
    }
}

fn safe_path(path: &str) -> bool {
    !path.is_empty()
        && Path::new(path)
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
}

fn member_id<'a>(atlas: &Atlas, observed: &'a DerivedModel, id: &str) -> Option<&'a str> {
    if observed.members.contains(id) {
        return observed.members.get(id).map(String::as_str);
    }
    let path = atlas
        .nodes
        .iter()
        .find(|node| node.id == id)?
        .path
        .as_deref()?;
    observed
        .nodes
        .iter()
        .filter(|node| observed.members.contains(&node.id))
        .filter(|node| {
            node.path.as_deref().is_some_and(|member_path| {
                path == member_path || path.starts_with(&format!("{member_path}/"))
            })
        })
        .max_by_key(|node| node.path.as_ref().map_or(0, String::len))
        .map(|node| node.id.as_str())
}

fn dependency_source(observed: &DerivedModel, member: &str) -> Option<String> {
    observed
        .nodes
        .iter()
        .find(|node| node.id == member)
        .and_then(|node| {
            let source = &node.derived.as_ref()?.source;
            if source.contains('/') {
                Some(source.clone())
            } else {
                Some(format!("{}/{}", node.path.as_deref()?, source))
            }
        })
}

fn evaluate(
    root: &Path,
    atlas: &Atlas,
    glossary: &Glossary,
    observed: &DerivedModel,
    rule: &Rule,
) -> Finding {
    if let Some(term) = rule.term()
        && !glossary.terms.iter().any(|entry| entry.id == term)
    {
        return finding(
            rule,
            None,
            Status::Unknown,
            format!("Term {term} is not in the current glossary."),
            vec![],
            "The rule's vocabulary link cannot be resolved.",
            Some(format!(
                "Use a current glossary term id in assertion {}.",
                rule.id()
            )),
        );
    }
    match rule {
        Rule::ImplementationExists {
            capability, part, ..
        } => {
            let Some(capability_model) = atlas
                .capabilities
                .iter()
                .find(|entry| entry.id == *capability)
            else {
                return finding(
                    rule,
                    part.as_deref(),
                    Status::Unknown,
                    format!("Capability {capability} is not in the current Atlas."),
                    vec![],
                    "No implementation target can be checked.",
                    Some(format!(
                        "Use a current capability id in assertion {}.",
                        rule.id()
                    )),
                );
            };
            let targets: Vec<_> = capability_model
                .implementation
                .iter()
                .filter(|target| part.as_ref().is_none_or(|part| target.part == *part))
                .collect();
            if targets.is_empty() {
                return finding(
                    rule,
                    part.as_deref(),
                    Status::Violation,
                    format!("Capability {capability} has no selected implementation target."),
                    vec![],
                    "Checks declared implementation paths, not behavior.",
                    Some(format!(
                        "Add a reviewed implementation path for {capability} in the Atlas source."
                    )),
                );
            }
            let root_canonical = root.canonicalize().ok();
            let broken: Vec<_> = targets
                .iter()
                .filter(|target| {
                    !safe_path(&target.path)
                        || root_canonical.as_ref().is_none_or(|root| {
                            root.join(&target.path)
                                .canonicalize()
                                .ok()
                                .is_none_or(|path| !path.starts_with(root) || !path.exists())
                        })
                })
                .collect();
            if !broken.is_empty() {
                let paths: Vec<String> = broken.iter().map(|target| target.path.clone()).collect();
                return finding(
                    rule,
                    part.as_deref(),
                    Status::Violation,
                    format!(
                        "Implementation target missing or outside the checkout: {}.",
                        paths.join(", ")
                    ),
                    paths.clone(),
                    "Path existence only; symbols and behavior are not checked.",
                    Some(format!(
                        "Update the implementation path for {capability}, or restore the missing path."
                    )),
                );
            }
            finding(
                rule,
                part.as_deref(),
                Status::Pass,
                format!("{} implementation target path(s) exist.", targets.len()),
                targets.iter().map(|target| target.path.clone()).collect(),
                "Path existence only; symbols and behavior are not checked.",
                None,
            )
        }
        Rule::Dependencies {
            part,
            allowed,
            forbidden,
            ..
        } => {
            if allowed.is_none() && forbidden.is_empty() {
                return finding(
                    rule,
                    Some(part),
                    Status::Unknown,
                    "This dependency rule has no allowed or forbidden boundary.",
                    vec![],
                    "No constraint was supplied, so a pass would say nothing.",
                    Some("Set allowed, forbidden, or both on this rule.".into()),
                );
            }
            let Some(source) = member_id(atlas, observed, part) else {
                return finding(
                    rule,
                    Some(part),
                    Status::Unknown,
                    format!("{part} has no supported workspace manifest."),
                    vec![],
                    "Only internal Cargo and Node workspace dependencies are extracted.",
                    Some(format!(
                        "Select a part backed by a supported workspace member, or add an extractor for {part}."
                    )),
                );
            };
            let wanted = allowed.iter().flatten().chain(forbidden.iter());
            let mut unresolved = Vec::new();
            let mapped: Vec<(String, String)> = wanted
                .map(|id| {
                    let member = member_id(atlas, observed, id).map(str::to_owned);
                    if member.is_none() {
                        unresolved.push(id.clone());
                    }
                    (id.clone(), member.unwrap_or_default())
                })
                .collect();
            if !unresolved.is_empty() {
                return finding(
                    rule,
                    Some(part),
                    Status::Unknown,
                    format!(
                        "No supported workspace member for: {}.",
                        unresolved.join(", ")
                    ),
                    vec![],
                    "A missing extractor or target cannot prove a dependency boundary.",
                    Some("Use workspace member ids or add the missing extractor.".into()),
                );
            }
            let actual: Vec<_> = observed
                .edges
                .iter()
                .filter(|edge| edge.from == source && edge.kind == EdgeKind::Depends)
                .collect();
            let allowed_members: BTreeSet<&str> = mapped
                .iter()
                .take(allowed.as_ref().map_or(0, Vec::len))
                .map(|(_, member)| member.as_str())
                .collect();
            let forbidden_members: BTreeSet<&str> = mapped
                .iter()
                .skip(allowed.as_ref().map_or(0, Vec::len))
                .map(|(_, member)| member.as_str())
                .collect();
            let broken: Vec<_> = actual
                .iter()
                .filter(|edge| {
                    forbidden_members.contains(edge.to.as_str())
                        || allowed
                            .as_ref()
                            .is_some_and(|_| !allowed_members.contains(edge.to.as_str()))
                })
                .collect();
            let evidence = dependency_source(observed, source).into_iter().collect();
            if broken.is_empty() {
                finding(
                    rule,
                    Some(part),
                    Status::Pass,
                    format!("No disallowed internal dependency was observed for {part}."),
                    evidence,
                    "Complete for discovered internal Cargo and Node workspace dependencies; external packages and runtime calls are outside this rule.",
                    None,
                )
            } else {
                finding(
                    rule,
                    Some(part),
                    Status::Violation,
                    format!(
                        "{part} has disallowed internal dependencies: {}.",
                        broken
                            .iter()
                            .map(|edge| edge.to.as_str())
                            .collect::<Vec<_>>()
                            .join(", ")
                    ),
                    evidence,
                    "Derived from current workspace manifests; external packages and runtime calls are outside this rule.",
                    Some(format!(
                        "Remove the disallowed dependency from {} or revise assertion {} after review.",
                        dependency_source(observed, source)
                            .unwrap_or_else(|| "the source manifest".into()),
                        rule.id()
                    )),
                )
            }
        }
        Rule::RequiredRelationship {
            from, to, relation, ..
        } => {
            if *relation != EdgeKind::Depends {
                return finding(
                    rule,
                    Some(from),
                    Status::Unknown,
                    format!(
                        "{} relationships have no completeness proof from the current extractor.",
                        relation.as_str()
                    ),
                    vec![],
                    "Only internal workspace manifest dependencies support required-relationship absence checks.",
                    Some(format!(
                        "Use a supported dependency assertion or add an extractor for {}.",
                        relation.as_str()
                    )),
                );
            }
            let (Some(source), Some(target)) = (
                member_id(atlas, observed, from),
                member_id(atlas, observed, to),
            ) else {
                return finding(
                    rule,
                    Some(from),
                    Status::Unknown,
                    format!(
                        "The relationship {from} -> {to} is outside supported workspace coverage."
                    ),
                    vec![],
                    "Absence cannot be certified without both workspace members.",
                    Some("Add the missing workspace extractor or use supported member ids.".into()),
                );
            };
            let found = observed.edges.iter().find(|edge| {
                edge.from == source && edge.to == target && edge.kind == EdgeKind::Depends
            });
            match found {
                Some(_edge) => finding(
                    rule,
                    Some(from),
                    Status::Pass,
                    format!("Required dependency {from} -> {to} is present."),
                    dependency_source(observed, source).into_iter().collect(),
                    "Internal workspace manifest dependency; no runtime call is inferred.",
                    None,
                ),
                None => finding(
                    rule,
                    Some(from),
                    Status::Violation,
                    format!("Required dependency {from} -> {to} is absent."),
                    dependency_source(observed, source).into_iter().collect(),
                    "Both ends are known workspace members; their current manifests were inspected.",
                    Some(format!(
                        "Restore the dependency in {} or revise assertion {} after review.",
                        dependency_source(observed, source)
                            .unwrap_or_else(|| "the source manifest".into()),
                        rule.id()
                    )),
                ),
            }
        }
    }
}

pub fn verify(root: &Path, source_dir: &str) -> Result<Report, AssertionError> {
    let source = format!("{source_dir}/assertions.json");
    let rules = if root.join(&source).is_file() {
        let bytes = std::fs::read(root.join(&source))
            .map_err(|error| AssertionError::Io(source.clone(), error))?;
        if bytes.len() > 64 * 1024 {
            return Err(AssertionError::TooLarge(source));
        }
        serde_json::from_slice::<RuleSet>(&bytes)
            .map_err(|error| AssertionError::Json(source, error))?
    } else {
        RuleSet {
            schema: ASSERTIONS_SCHEMA.into(),
            rules: Vec::new(),
        }
    };
    if rules.schema != ASSERTIONS_SCHEMA {
        return Err(AssertionError::Schema);
    }
    let mut ids = BTreeSet::new();
    for rule in &rules.rules {
        if rule.id().trim().is_empty() || !ids.insert(rule.id()) {
            return Err(AssertionError::RuleId(rule.id().into()));
        }
    }
    let atlas = super::load_atlas_from(root, source_dir)?;
    let glossary = super::load_glossary_terms_from(root, source_dir)?;
    let observed = derive::derive(root, &atlas)?;
    let snapshot = snapshot::capture(root, source_dir, &atlas.project.repo);
    let findings = rules
        .rules
        .iter()
        .map(|rule| evaluate(root, &atlas, &glossary, &observed, rule))
        .collect();
    Ok(Report {
        schema: REPORT_SCHEMA,
        snapshot,
        findings,
        coverage: vec!["Assertions use the current local tree. Only existing implementation paths and internal workspace manifest dependencies are currently checkable; no assertion proves behavior or human meaning.".into()],
    })
}

pub fn render(report: &Report) -> String {
    let mut text = format!("Atlas assertions at {}\n", report.snapshot.fingerprint);
    if report.findings.is_empty() {
        text.push_str("No assertions configured. Add .rosetta/assertions.json.\n");
    }
    for item in &report.findings {
        text.push_str(&format!(
            "{}: {:?} — {}\n",
            item.id, item.status, item.message
        ));
        if let Some(term) = &item.term {
            text.push_str(&format!("  Term: {term}\n"));
        }
        if let Some(part) = &item.part {
            text.push_str(&format!("  Part: {part}\n"));
        }
        for source in &item.evidence {
            text.push_str(&format!("  Evidence: {source}\n"));
        }
        text.push_str(&format!("  Coverage: {}\n", item.coverage));
        if let Some(repair) = &item.repair {
            text.push_str(&format!("  Repair: {repair}\n"));
        }
    }
    text
}
