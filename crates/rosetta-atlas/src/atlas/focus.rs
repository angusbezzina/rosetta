//! One deterministic semantic selection for CLI, local viewer and adapters.
//! Camera position belongs to a renderer, never to this contract.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use base64::Engine;
use serde::{Deserialize, Serialize};

use super::bundle::{Bundle, BundleNode, BundleTerm};
use super::model::{Capability, Edge, EdgeKind};
use rosetta_core::snapshot::Snapshot;

pub const FOCUS_SCHEMA: &str = "rosetta.focus/v1";
const REF_PREFIX: &str = "rv1.";

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, clap::ValueEnum)]
#[serde(rename_all = "snake_case")]
pub enum Level {
    Capability,
    #[default]
    Component,
    Implementation,
}

fn default_depth() -> u8 {
    1
}
fn default_nodes() -> usize {
    24
}
fn default_edges() -> usize {
    48
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Query {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capability: Option<String>,
    #[serde(default)]
    pub roots: Vec<String>,
    #[serde(default)]
    pub paths: Vec<String>,
    #[serde(default)]
    pub level: Level,
    #[serde(default = "default_depth")]
    pub depth: u8,
    #[serde(default)]
    pub kinds: Vec<EdgeKind>,
    #[serde(default = "default_nodes")]
    pub max_nodes: usize,
    #[serde(default = "default_edges")]
    pub max_edges: usize,
}

impl Default for Query {
    fn default() -> Self {
        Self {
            capability: None,
            roots: Vec::new(),
            paths: Vec::new(),
            level: Level::Component,
            depth: default_depth(),
            kinds: Vec::new(),
            max_nodes: default_nodes(),
            max_edges: default_edges(),
        }
    }
}

impl Query {
    fn normalized(mut self) -> Result<Self, FocusError> {
        if self.depth > 5
            || !(1..=200).contains(&self.max_nodes)
            || !(1..=400).contains(&self.max_edges)
        {
            return Err(FocusError::Budget);
        }
        self.roots.sort();
        self.roots.dedup();
        self.paths.sort();
        self.paths.dedup();
        self.kinds.sort();
        self.kinds.dedup();
        Ok(self)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Omissions {
    pub nodes_over_budget: usize,
    pub edges_over_budget: usize,
    pub edges_filtered: usize,
    pub beyond_depth: usize,
    pub implementation_links_not_drawn: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImplementationLink {
    pub part: String,
    pub node: String,
    pub path: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FocusedView {
    pub schema: String,
    pub id: String,
    /// Opaque, portable reference that includes the original snapshot.
    pub reference: String,
    pub snapshot: Snapshot,
    pub query: Query,
    pub capabilities: Vec<Capability>,
    pub nodes: Vec<BundleNode>,
    pub edges: Vec<Edge>,
    pub diagram: String,
    pub terms: Vec<BundleTerm>,
    pub target_exists: BTreeMap<String, bool>,
    /// Dotted links join a declared responsibility to an observed file.
    pub implementation_links: Vec<ImplementationLink>,
    pub omissions: Omissions,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Reference {
    version: u8,
    snapshot: String,
    query: Query,
}

#[derive(Debug, thiserror::Error)]
pub enum FocusError {
    #[error("this bundle has no snapshot; rebuild it with the current Rosetta")]
    MissingSnapshot,
    #[error("no capability with id {0:?}")]
    UnknownCapability(String),
    #[error("no part or implementation node with id {0:?}")]
    UnknownNode(String),
    #[error("no part or implementation node covers path {0:?}")]
    UnknownPath(String),
    #[error("depth must be at most 5, max_nodes 1..200 and max_edges 1..400")]
    Budget,
    #[error("the focused-view reference is malformed or too long")]
    InvalidReference,
    #[error("--max-chars is too small to retain the view reference and omission counts")]
    ContextBudget,
    #[error(
        "the view was made for snapshot {expected}; current snapshot is {current}; regenerate the view explicitly"
    )]
    Stale { expected: String, current: String },
}

fn make_reference(snapshot: &Snapshot, query: &Query) -> Result<String, FocusError> {
    let source = Reference {
        version: 1,
        snapshot: snapshot.fingerprint.clone(),
        query: query.clone(),
    };
    let bytes = serde_json::to_vec(&source).map_err(|_| FocusError::InvalidReference)?;
    Ok(format!(
        "{REF_PREFIX}{}",
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
    ))
}

/// Reopen a reference only on the snapshot that produced it. Call `select`
/// with the decoded query to regenerate deliberately after a change.
pub fn reopen(bundle: &Bundle, reference: &str) -> Result<FocusedView, FocusError> {
    let saved = decode_reference(reference)?;
    let snapshot = bundle
        .snapshot
        .as_ref()
        .ok_or(FocusError::MissingSnapshot)?;
    if saved.snapshot != snapshot.fingerprint {
        return Err(FocusError::Stale {
            expected: saved.snapshot,
            current: snapshot.fingerprint.clone(),
        });
    }
    select(bundle, saved.query)
}

/// Re-select the same semantic query on the current snapshot after the user
/// explicitly chooses to refresh a stale reference.
pub fn refresh(bundle: &Bundle, reference: &str) -> Result<FocusedView, FocusError> {
    select(bundle, decode_reference(reference)?.query)
}

fn decode_reference(reference: &str) -> Result<Reference, FocusError> {
    let encoded = reference
        .strip_prefix(REF_PREFIX)
        .ok_or(FocusError::InvalidReference)?;
    if encoded.len() > 8192 {
        return Err(FocusError::InvalidReference);
    }
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(encoded)
        .map_err(|_| FocusError::InvalidReference)?;
    let saved: Reference =
        serde_json::from_slice(&bytes).map_err(|_| FocusError::InvalidReference)?;
    if saved.version != 1 {
        return Err(FocusError::InvalidReference);
    }
    Ok(saved)
}

/// A bounded, readable projection of the exact semantic slice. The reference
/// and omission counts are mandatory; optional detail lines are whole lines.
pub fn render_context(view: &FocusedView, max_chars: usize) -> Result<String, FocusError> {
    if max_chars < 300 {
        return Err(FocusError::ContextBudget);
    }
    let mut output = format!(
        "View: {}\nReference: {}\nRepository: {}\nSnapshot: {} ({:?}, dirty: {:?})\nLevel: {:?}; depth: {}\n",
        view.id,
        view.reference,
        view.snapshot.repository,
        view.snapshot.fingerprint,
        view.snapshot.coverage,
        view.snapshot.dirty,
        view.query.level,
        view.query.depth
    );
    let mut detail = Vec::new();
    for capability in &view.capabilities {
        detail.push(format!(
            "Capability {}: {} ({:?}) — {}\n",
            capability.id, capability.name, capability.status, capability.purpose
        ));
    }
    let owning_parts: BTreeSet<&str> = view
        .capabilities
        .iter()
        .flat_map(|capability| capability.parts.iter().map(String::as_str))
        .chain(view.query.roots.iter().map(String::as_str))
        .collect();
    let part_line = |node: &BundleNode| {
        format!(
            "Part {}: {} [{}] {}{}\n",
            node.id,
            node.label,
            node.kind.as_str(),
            node.path.as_deref().unwrap_or("no path"),
            node.summary
                .as_deref()
                .map_or(String::new(), |summary| format!(" — {summary}"))
        )
    };
    for node in view
        .nodes
        .iter()
        .filter(|node| owning_parts.contains(node.id.as_str()))
    {
        detail.push(part_line(node));
    }
    for capability in &view.capabilities {
        for target in &capability.implementation {
            let exists = view.target_exists.get(&target.path).copied();
            detail.push(format!(
                "Implementation {} -> {}{} ({})\n",
                target.part,
                target.path,
                target
                    .symbol
                    .as_deref()
                    .map_or(String::new(), |symbol| format!("#{symbol}")),
                match exists {
                    Some(true) => "path exists",
                    Some(false) => "path missing",
                    None => "not checked",
                }
            ));
        }
    }
    let relevant_terms: BTreeSet<&str> = view
        .capabilities
        .iter()
        .flat_map(|capability| capability.terms.iter().map(String::as_str))
        .collect();
    for term in view
        .terms
        .iter()
        .filter(|term| relevant_terms.contains(term.id.as_str()))
    {
        detail.push(format!(
            "Term {}: {} — {}\n",
            term.id, term.name, term.definition
        ));
    }
    for node in view
        .nodes
        .iter()
        .filter(|node| !owning_parts.contains(node.id.as_str()))
    {
        detail.push(part_line(node));
    }
    for term in view
        .terms
        .iter()
        .filter(|term| !relevant_terms.contains(term.id.as_str()))
    {
        detail.push(format!(
            "Term {}: {} — {}\n",
            term.id, term.name, term.definition
        ));
    }
    for edge in &view.edges {
        let evidence = edge
            .evidence
            .as_ref()
            .map_or("source unknown", |evidence| evidence.source.as_str());
        detail.push(format!(
            "Relation {} -> {}: {}{} ({evidence})\n",
            edge.from,
            edge.to,
            edge.kind.as_str(),
            edge.label
                .as_deref()
                .map_or(String::new(), |label| format!(" · {label}"))
        ));
    }
    let mut omitted = 0;
    for line in &detail {
        let footer = format!(
            "Omitted by view: {} nodes, {} edges, {} filtered edges, {} beyond depth, {} implementation links. Omitted for text budget: {} lines.\n",
            view.omissions.nodes_over_budget,
            view.omissions.edges_over_budget,
            view.omissions.edges_filtered,
            view.omissions.beyond_depth,
            view.omissions.implementation_links_not_drawn,
            detail.len()
        );
        if output.chars().count() + line.chars().count() + footer.chars().count() > max_chars {
            omitted += 1;
        } else if omitted == 0 {
            output.push_str(line);
        } else {
            omitted += 1;
        }
    }
    let footer = format!(
        "Omitted by view: {} nodes, {} edges, {} filtered edges, {} beyond depth, {} implementation links. Omitted for text budget: {omitted} lines.\n",
        view.omissions.nodes_over_budget,
        view.omissions.edges_over_budget,
        view.omissions.edges_filtered,
        view.omissions.beyond_depth,
        view.omissions.implementation_links_not_drawn
    );
    if output.chars().count() + footer.chars().count() > max_chars {
        return Err(FocusError::ContextBudget);
    }
    output.push_str(&footer);
    Ok(output)
}

pub fn select(bundle: &Bundle, query: Query) -> Result<FocusedView, FocusError> {
    let query = query.normalized()?;
    let snapshot = bundle
        .snapshot
        .as_ref()
        .ok_or(FocusError::MissingSnapshot)?;
    let by_id: BTreeMap<&str, &BundleNode> = bundle
        .nodes
        .iter()
        .map(|node| (node.id.as_str(), node))
        .collect();
    let selected_capability = query
        .capability
        .as_deref()
        .map(|id| {
            bundle
                .capabilities
                .iter()
                .find(|capability| capability.id == id)
                .ok_or_else(|| FocusError::UnknownCapability(id.to_owned()))
        })
        .transpose()?;

    let mut seeds: BTreeSet<String> = query.roots.iter().cloned().collect();
    for root in &query.roots {
        if !by_id.contains_key(root.as_str()) {
            return Err(FocusError::UnknownNode(root.clone()));
        }
    }
    for path in &query.paths {
        let covering = bundle
            .nodes
            .iter()
            .filter(|node| {
                node.path.as_deref().is_some_and(|candidate| {
                    path == candidate || path.starts_with(&format!("{candidate}/"))
                })
            })
            .max_by_key(|node| node.path.as_ref().map_or(0, String::len))
            .ok_or_else(|| FocusError::UnknownPath(path.clone()))?;
        seeds.insert(covering.id.clone());
        if let Some(part) = bundle
            .nodes
            .iter()
            .filter(|node| {
                node.kind.is_part()
                    && node.path.as_deref().is_some_and(|candidate| {
                        path == candidate || path.starts_with(&format!("{candidate}/"))
                    })
            })
            .max_by_key(|node| node.path.as_ref().map_or(0, String::len))
        {
            seeds.insert(part.id.clone());
        }
    }
    if let Some(capability) = selected_capability {
        seeds.extend(capability.parts.iter().cloned());
        seeds.extend(
            capability
                .implementation
                .iter()
                .map(|target| target.part.clone()),
        );
        if query.level == Level::Implementation {
            for target in &capability.implementation {
                if let Some(node) = bundle
                    .nodes
                    .iter()
                    .find(|node| node.path.as_deref() == Some(&target.path))
                {
                    seeds.insert(node.id.clone());
                }
            }
        }
    }
    if seeds.is_empty() {
        seeds.extend(
            bundle
                .nodes
                .iter()
                .filter(|node| node.kind.is_part())
                .map(|node| node.id.clone()),
        );
    }
    for seed in &seeds {
        if !by_id.contains_key(seed.as_str()) {
            return Err(FocusError::UnknownNode(seed.clone()));
        }
    }

    let mut eligible: Vec<&Edge> = bundle
        .edges
        .iter()
        .filter(|edge| query.kinds.is_empty() || query.kinds.contains(&edge.kind))
        .collect();
    eligible.sort_by(|a, b| {
        (&a.from, &a.to, a.kind, &a.label).cmp(&(&b.from, &b.to, b.kind, &b.label))
    });
    let edges_filtered = bundle.edges.len() - eligible.len();
    let mut visited = seeds.clone();
    let mut discovery: Vec<String> = seeds.iter().cloned().collect();
    let mut queue: VecDeque<(String, u8)> = seeds.iter().cloned().map(|id| (id, 0)).collect();
    let mut beyond_depth = BTreeSet::new();
    while let Some((id, depth)) = queue.pop_front() {
        for edge in &eligible {
            let other = if edge.from == id {
                Some(&edge.to)
            } else if edge.to == id {
                Some(&edge.from)
            } else {
                None
            };
            if let Some(other) = other {
                if depth >= query.depth || query.level == Level::Capability {
                    if !visited.contains(other) {
                        beyond_depth.insert(other.clone());
                    }
                } else if visited.insert(other.clone()) {
                    discovery.push(other.clone());
                    queue.push_back((other.clone(), depth + 1));
                }
            }
        }
    }
    beyond_depth.retain(|node| !visited.contains(node));
    let all_nodes = discovery.len();
    let retained: BTreeSet<String> = discovery.into_iter().take(query.max_nodes).collect();
    let mut selected_edges: Vec<Edge> = eligible
        .into_iter()
        .filter(|edge| retained.contains(&edge.from) && retained.contains(&edge.to))
        .cloned()
        .collect();
    let all_edges = selected_edges.len();
    selected_edges.truncate(query.max_edges);
    let edges_over_budget = all_edges.saturating_sub(query.max_edges);
    let nodes: Vec<BundleNode> = bundle
        .nodes
        .iter()
        .filter(|node| retained.contains(&node.id))
        .cloned()
        .collect();
    let capabilities: Vec<Capability> = bundle
        .capabilities
        .iter()
        .filter(|capability| {
            selected_capability.is_some_and(|selected| selected.id == capability.id)
                || capability.parts.iter().any(|part| retained.contains(part))
        })
        .cloned()
        .collect();
    let scoped_paths: Vec<&str> = query
        .paths
        .iter()
        .map(String::as_str)
        .chain(selected_capability.into_iter().flat_map(|capability| {
            capability
                .implementation
                .iter()
                .map(|target| target.path.as_str())
        }))
        .collect();
    let path_contexts: BTreeSet<&str> = bundle
        .path_scopes
        .iter()
        .filter(|scope| {
            scope.paths.iter().any(|pattern| {
                glob::Pattern::new(pattern)
                    .ok()
                    .is_some_and(|pattern| scoped_paths.iter().any(|path| pattern.matches(path)))
            })
        })
        .map(|scope| scope.context.as_str())
        .collect();
    let term_ids: BTreeSet<&str> = capabilities
        .iter()
        .flat_map(|capability| capability.terms.iter().map(String::as_str))
        .chain(nodes.iter().filter_map(|node| node.refs.term.as_deref()))
        .chain(
            bundle
                .terms
                .iter()
                .filter(|term| {
                    !term.contexts.is_empty()
                        && term
                            .contexts
                            .iter()
                            .any(|context| path_contexts.contains(context.as_str()))
                })
                .map(|term| term.id.as_str()),
        )
        .collect();
    let terms = bundle
        .terms
        .iter()
        .filter(|term| term_ids.contains(term.id.as_str()))
        .cloned()
        .collect();
    let target_exists = capabilities
        .iter()
        .flat_map(|capability| capability.implementation.iter())
        .filter_map(|target| {
            bundle
                .target_exists
                .get(&target.path)
                .map(|exists| (target.path.clone(), *exists))
        })
        .collect();
    let total_implementation_links: usize = capabilities
        .iter()
        .map(|capability| capability.implementation.len())
        .sum();
    let mut implementation_links: Vec<ImplementationLink> = capabilities
        .iter()
        .flat_map(|capability| capability.implementation.iter())
        .filter_map(|target| {
            nodes
                .iter()
                .find(|node| node.path.as_deref() == Some(target.path.as_str()))
                .filter(|_| retained.contains(&target.part))
                .map(|node| ImplementationLink {
                    part: target.part.clone(),
                    node: node.id.clone(),
                    path: target.path.clone(),
                })
        })
        .collect();
    implementation_links
        .sort_by(|a, b| (&a.part, &a.node, &a.path).cmp(&(&b.part, &b.node, &b.path)));
    implementation_links.dedup();
    let implementation_links_not_drawn =
        total_implementation_links.saturating_sub(implementation_links.len());
    let reference = make_reference(snapshot, &query)?;
    let id = blake3::hash(reference.as_bytes()).to_hex()[..16].to_owned();
    let diagram_links: Vec<(&str, &str)> = implementation_links
        .iter()
        .map(|link| (link.part.as_str(), link.node.as_str()))
        .collect();
    let diagram = super::emit::focused(&nodes, &selected_edges, &diagram_links);
    Ok(FocusedView {
        schema: FOCUS_SCHEMA.to_owned(),
        id,
        reference,
        snapshot: snapshot.clone(),
        query,
        capabilities,
        nodes,
        edges: selected_edges,
        diagram,
        terms,
        target_exists,
        implementation_links,
        omissions: Omissions {
            nodes_over_budget: all_nodes.saturating_sub(retained.len()),
            edges_over_budget,
            edges_filtered,
            beyond_depth: beyond_depth.len(),
            implementation_links_not_drawn,
        },
    })
}
