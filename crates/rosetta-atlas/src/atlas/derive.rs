//! Nodes and edges read from the repo itself: the Cargo and bun workspace
//! members, the directories between the repo root and them, and their
//! dependencies on each other; the cloud resources its Pulumi programs and
//! Terraform files declare; a part for each member that ships something no
//! curated part covers; and the stores and outside services the members'
//! drivers and SDKs reach. `sync` merges them into the committed model
//! without touching what a person curated; `compare` says how a model
//! differs from a sync.

mod iac;
mod uses;

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

pub use uses::part_id;

use super::model::{Atlas, Derived, Edge, EdgeKind, Family, Node, NodeKind, Refs};
use super::walk::{self, WalkError};
use crate::text::kebab_of;
use crate::workspace::{self, Manager, WorkspaceError};
use rosetta_core::config::{Config, ConfigError};

/// The repo root's node id.
pub const ROOT_ID: &str = "repo";

#[derive(Debug, thiserror::Error)]
pub enum DeriveError {
    #[error(transparent)]
    Config(#[from] ConfigError),
    #[error(transparent)]
    Workspace(#[from] WorkspaceError),
    #[error(transparent)]
    Walk(#[from] WalkError),
    #[error("{path}: {source}")]
    Io {
        path: String,
        source: std::io::Error,
    },
}

/// What a derivation found, in a fixed order.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DerivedModel {
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,
    /// Ids of the workspace member nodes, which the live walk descends into.
    pub members: BTreeSet<String>,
}

/// The family a path defaults to by its top directory; curation overrides it.
pub fn default_family(path: &str) -> Family {
    match path.split('/').next().unwrap_or_default() {
        "apps" => Family::Surface,
        "tools" => Family::Agent,
        "infra" => Family::Store,
        _ => Family::Service,
    }
}

/// A node id for a repo path.
pub fn path_id(path: &str) -> String {
    kebab_of(path)
}

fn derived(source: &str) -> Option<Derived> {
    Some(Derived {
        source: source.to_owned(),
    })
}

fn last_segment(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

/// Derives from the repo at `root`. The model's project name labels the
/// repo root's node, and its curated nodes decide which members already
/// have a part and which stores and outside services already have a node.
pub fn derive(root: &Path, atlas: &Atlas) -> Result<DerivedModel, DeriveError> {
    let config = Config::load(root, None)?;
    derive_with_config(root, atlas, &config)
}

pub fn derive_with_config(
    root: &Path,
    atlas: &Atlas,
    config: &Config,
) -> Result<DerivedModel, DeriveError> {
    let project = atlas.project.name.as_str();
    let mut members = workspace::cargo_members(root)?;
    let mut seen: BTreeSet<String> = members.iter().map(|m| m.path.clone()).collect();
    for member in workspace::bun_members(root)? {
        // A directory that is in both workspaces is one node, named by Cargo.
        if seen.insert(member.path.clone()) {
            members.push(member);
        }
    }
    // A package the root workspaces exclude is still a part of the repo.
    for member in workspace::excluded_members(root)? {
        if seen.insert(member.path.clone()) {
            members.push(member);
        }
    }
    members.sort_by(|a, b| a.path.cmp(&b.path));
    let member_paths: BTreeSet<&str> = members.iter().map(|m| m.path.as_str()).collect();

    let root_source = if root.join("Cargo.toml").is_file() {
        "Cargo.toml"
    } else {
        "package.json"
    };
    let mut by_path: BTreeMap<String, Node> = BTreeMap::new();
    let repo_node = Node {
        id: ROOT_ID.to_owned(),
        label: project.to_owned(),
        family: Family::Service,
        kind: NodeKind::Directory,
        parent: None,
        path: Some(".".to_owned()),
        summary: None,
        tier: None,
        refs: Refs::default(),
        derived: derived(root_source),
    };

    let parent_of = |path: &str| -> String {
        match path.rsplit_once('/') {
            Some((parent, _)) => path_id(parent),
            None => ROOT_ID.to_owned(),
        }
    };

    for member in &members {
        let source = member.manager.manifest();
        let mut prefix = String::new();
        for segment in member.path.split('/') {
            if !prefix.is_empty() {
                prefix.push('/');
            }
            prefix.push_str(segment);
            if prefix == member.path || member_paths.contains(prefix.as_str()) {
                continue;
            }
            by_path.entry(prefix.clone()).or_insert_with(|| Node {
                id: path_id(&prefix),
                label: last_segment(&prefix).to_owned(),
                family: default_family(&prefix),
                kind: NodeKind::Directory,
                parent: Some(parent_of(&prefix)),
                path: Some(prefix.clone()),
                summary: None,
                tier: None,
                refs: Refs::default(),
                derived: derived(source),
            });
        }
        let kind = match member.manager {
            Manager::Cargo => NodeKind::Crate,
            Manager::Bun if member.path.starts_with("apps/") => NodeKind::App,
            Manager::Bun => NodeKind::Package,
        };
        by_path.insert(
            member.path.clone(),
            Node {
                id: path_id(&member.path),
                label: member.name.clone(),
                family: default_family(&member.path),
                kind,
                parent: Some(parent_of(&member.path)),
                path: Some(member.path.clone()),
                summary: None,
                tier: None,
                refs: Refs::default(),
                derived: derived(source),
            },
        );
    }

    // The working tree, as sync reads it: a tracked file deleted but not yet
    // staged is gone.
    let tracked: Vec<String> = walk::tracked_files(root)?
        .into_iter()
        .filter(|file| root.join(file).is_file())
        .collect();
    let programs = if config.derivation.iac {
        iac::programs(root, &tracked)?
    } else {
        Vec::new()
    };
    for program in &programs {
        let mut prefix = String::new();
        for segment in program.dir.split('/').filter(|s| !s.is_empty()) {
            if !prefix.is_empty() {
                prefix.push('/');
            }
            prefix.push_str(segment);
            by_path.entry(prefix.clone()).or_insert_with(|| Node {
                id: path_id(&prefix),
                label: last_segment(&prefix).to_owned(),
                family: default_family(&prefix),
                kind: NodeKind::Directory,
                parent: Some(parent_of(&prefix)),
                path: Some(prefix.clone()),
                summary: None,
                tier: None,
                refs: Refs::default(),
                derived: derived(&program.source),
            });
        }
    }

    let curated: Vec<&Node> = atlas
        .nodes
        .iter()
        .filter(|node| node.derived.is_none() && node.kind.is_part())
        .collect();
    let tracked_set: BTreeSet<&str> = tracked.iter().map(String::as_str).collect();
    let parts = uses::deploy_parts(root, &members, &tracked_set, &curated)?;
    // The nodes a person said where they keep state: a curated read or
    // write to any store.
    let stores: BTreeSet<&str> = atlas
        .nodes
        .iter()
        .filter(|node| node.family == Family::Store)
        .map(|node| node.id.as_str())
        .collect();
    let keeps_state: BTreeSet<&str> = atlas
        .edges
        .iter()
        .filter(|edge| edge.derived.is_none())
        .filter(|edge| matches!(edge.kind, EdgeKind::Reads | EdgeKind::Writes))
        .filter(|edge| stores.contains(edge.to.as_str()))
        .map(|edge| edge.from.as_str())
        .collect();
    let (technologies, used) = if config.derivation.drivers {
        uses::uses(root, &members, &tracked, &curated, &keeps_state, &parts)?
    } else {
        (Vec::new(), Vec::new())
    };

    let mut edges: Vec<Edge> = members
        .iter()
        .flat_map(|member| {
            let member_paths = &member_paths;
            member
                .depends_on
                .iter()
                .filter(move |target| member_paths.contains(target.as_str()))
                .map(move |target| Edge {
                    from: path_id(&member.path),
                    to: path_id(target),
                    kind: EdgeKind::Depends,
                    label: None,
                    derived: derived(member.manager.manifest()),
                    evidence: None,
                })
        })
        .chain(used)
        .filter(|edge| edge.from != edge.to)
        .collect();
    edges.sort_by(|a, b| (&a.from, &a.to, a.kind).cmp(&(&b.from, &b.to, b.kind)));
    edges.dedup_by(|a, b| a.from == b.from && a.to == b.to && a.kind == b.kind);

    let mut nodes = vec![repo_node];
    nodes.extend(by_path.into_values());
    nodes.extend(programs.into_iter().flat_map(|program| program.resources));
    nodes.extend(parts);
    nodes.extend(technologies);
    Ok(DerivedModel {
        members: members.iter().map(|m| path_id(&m.path)).collect(),
        nodes,
        edges,
    })
}

fn has_curation(node: &Node) -> bool {
    node.summary.is_some() || !node.refs.is_empty()
}

/// Ids a curated part of the model points at, which keep a stale node.
fn curated_references(atlas: &Atlas) -> BTreeSet<String> {
    let mut ids = BTreeSet::new();
    for edge in atlas.edges.iter().filter(|edge| edge.derived.is_none()) {
        ids.insert(edge.from.clone());
        ids.insert(edge.to.clone());
    }
    for view in &atlas.views {
        ids.extend(view.include.iter().flatten().cloned());
        ids.extend(view.steps.iter().flatten().map(|step| step.node.clone()));
    }
    for node in atlas.nodes.iter().filter(|node| node.derived.is_none()) {
        ids.extend(node.parent.iter().cloned());
    }
    ids
}

/// Merges a derivation into the model. Curated nodes come first in their
/// written order, then derived nodes in derivation order. A derived node
/// keeps its curated label, summary, family, tier and refs; a derived node whose
/// source is gone is dropped unless curation or a curated reference keeps
/// it, and then `compare` reports it.
pub fn sync(atlas: &Atlas, derived_model: &DerivedModel) -> Atlas {
    let derived_ids: BTreeSet<&str> = derived_model.nodes.iter().map(|n| n.id.as_str()).collect();
    let existing: BTreeMap<&str, &Node> = atlas.nodes.iter().map(|n| (n.id.as_str(), n)).collect();
    let keep = curated_references(atlas);

    let mut nodes: Vec<Node> = atlas
        .nodes
        .iter()
        .filter(|node| node.derived.is_none() && !derived_ids.contains(node.id.as_str()))
        .cloned()
        .collect();
    for fresh in &derived_model.nodes {
        nodes.push(match existing.get(fresh.id.as_str()) {
            Some(current) => Node {
                label: current.label.clone(),
                summary: current.summary.clone(),
                family: current.family,
                tier: current.tier.or(fresh.tier),
                refs: current.refs.clone(),
                ..fresh.clone()
            },
            None => fresh.clone(),
        });
    }
    nodes.extend(
        atlas
            .nodes
            .iter()
            .filter(|node| node.derived.is_some() && !derived_ids.contains(node.id.as_str()))
            .filter(|node| has_curation(node) || keep.contains(&node.id))
            .cloned(),
    );

    let existing_edges: BTreeMap<(&str, &str, EdgeKind), &Edge> = atlas
        .edges
        .iter()
        .map(|edge| ((edge.from.as_str(), edge.to.as_str(), edge.kind), edge))
        .collect();
    let mut edges: Vec<Edge> = atlas
        .edges
        .iter()
        .filter(|edge| edge.derived.is_none())
        .cloned()
        .collect();
    let curated_edges: BTreeSet<(String, String, EdgeKind)> = edges
        .iter()
        .map(|edge| (edge.from.clone(), edge.to.clone(), edge.kind))
        .collect();
    for fresh in &derived_model.edges {
        if curated_edges.contains(&(fresh.from.clone(), fresh.to.clone(), fresh.kind)) {
            continue;
        }
        // What derived the edge says it carries; a person may label an
        // edge the derivation leaves unlabelled.
        let label = fresh.label.clone().or_else(|| {
            existing_edges
                .get(&(fresh.from.as_str(), fresh.to.as_str(), fresh.kind))
                .and_then(|edge| edge.label.clone())
        });
        edges.push(Edge {
            label,
            ..fresh.clone()
        });
    }

    Atlas {
        schema: atlas.schema.clone(),
        project: atlas.project.clone(),
        nodes,
        edges,
        views: atlas.views.clone(),
        capabilities: atlas.capabilities.clone(),
        records: atlas.records.clone(),
    }
}

/// How the committed model differs from a sync, one finding per difference.
pub fn compare(atlas: &Atlas, derived_model: &DerivedModel) -> Vec<String> {
    let mut findings = Vec::new();
    let existing: BTreeMap<&str, &Node> = atlas.nodes.iter().map(|n| (n.id.as_str(), n)).collect();
    let fresh: BTreeMap<&str, &Node> = derived_model
        .nodes
        .iter()
        .map(|n| (n.id.as_str(), n))
        .collect();

    for node in &derived_model.nodes {
        let path = node.path.as_deref().unwrap_or_default();
        let source = node
            .derived
            .as_ref()
            .map(|d| d.source.as_str())
            .unwrap_or_default();
        let what = if derived_model.members.contains(&node.id) {
            format!("is a workspace member in {source}")
        } else {
            format!("is derived from {source}")
        };
        match existing.get(node.id.as_str()) {
            None => findings.push(format!(
                "nodes: {path} ({}) {what} with no node \"{}\"; run task atlas:sync",
                node.label, node.id
            )),
            Some(current) if current.derived != node.derived => findings.push(format!(
                "nodes[{}].derived: must name source \"{}\"; run task atlas:sync",
                node.id,
                node.derived
                    .as_ref()
                    .map(|d| d.source.as_str())
                    .unwrap_or_default()
            )),
            Some(current)
                if current.kind != node.kind
                    || current.path != node.path
                    || current.parent != node.parent =>
            {
                findings.push(format!(
                    "nodes[{}]: kind, path or parent differs from what {} declares; run task atlas:sync",
                    node.id,
                    node.derived
                        .as_ref()
                        .map(|d| d.source.as_str())
                        .unwrap_or_default()
                ));
            }
            Some(_) => {}
        }
    }
    for node in &atlas.nodes {
        if let Some(source) = &node.derived
            && !fresh.contains_key(node.id.as_str())
        {
            findings.push(format!(
                "nodes[{}].derived: {} no longer declares it, so it is stale; run task atlas:sync or remove it",
                node.id, source.source
            ));
        }
    }

    let fresh_edges: BTreeSet<(&str, &str, EdgeKind)> = derived_model
        .edges
        .iter()
        .map(|e| (e.from.as_str(), e.to.as_str(), e.kind))
        .collect();
    let committed_edges: BTreeSet<(&str, &str, EdgeKind)> = atlas
        .edges
        .iter()
        .map(|e| (e.from.as_str(), e.to.as_str(), e.kind))
        .collect();
    for edge in &derived_model.edges {
        if !committed_edges.contains(&(edge.from.as_str(), edge.to.as_str(), edge.kind)) {
            findings.push(format!(
                "edges: {} {} {} is missing; run task atlas:sync",
                edge.from,
                edge.kind.as_str(),
                edge.to
            ));
        }
    }
    for (index, edge) in atlas.edges.iter().enumerate() {
        if edge.derived.is_some()
            && !fresh_edges.contains(&(edge.from.as_str(), edge.to.as_str(), edge.kind))
        {
            findings.push(format!(
                "edges[{index}].derived: {} {} {} is no longer declared; run task atlas:sync",
                edge.from,
                edge.kind.as_str(),
                edge.to
            ));
        }
    }

    if findings.is_empty() && sync(atlas, derived_model) != *atlas {
        findings.push(
            "nodes: the order or a derived tier differs from what task atlas:sync writes; run task atlas:sync"
                .to_owned(),
        );
    }
    findings
}
