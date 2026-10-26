//! The `rosetta.atlas/v1` model committed as `.rosetta/atlas.json`: curated
//! nodes and custom views, plus nodes derived from the repo's own files.
//! Files below a member are never committed; they are walked live.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub const ATLAS_SCHEMA: &str = "rosetta.atlas/v1";
/// Existing Virgil repositories remain valid while they move to Rosetta.
pub const VIRGIL_ATLAS_SCHEMA: &str = "virgil.atlas/v1";
pub const ATLAS_SOURCE: &str = ".rosetta/atlas.json";
/// The id of the view every bundle carries, one source per connected part.
pub const CONNECTIONS_VIEW: &str = "connections";
/// The id of the view an atlas with a `records` section carries.
pub const RECORDS_VIEW: &str = "records";
/// The id of the built picture of every part.
pub const OVERVIEW_VIEW: &str = "overview";
/// The id of the built picture of the cloud resources that hold state or run.
pub const INFRA_VIEW: &str = "infra";
/// The id of the built picture of every edge that moves state.
pub const DATA_VIEW: &str = "data";
/// Views the bundle builds from rules; the model never declares one.
pub const BUILT_VIEWS: [&str; 5] = [
    OVERVIEW_VIEW,
    INFRA_VIEW,
    RECORDS_VIEW,
    DATA_VIEW,
    CONNECTIONS_VIEW,
];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Atlas {
    /// `rosetta.atlas/v1`; the Virgil schema id remains an accepted alias.
    pub schema: String,
    pub project: Project,
    pub nodes: Vec<Node>,
    #[serde(default)]
    pub edges: Vec<Edge>,
    /// Custom views only: `flow` and `custom`. Overview, Infra, Records, Data
    /// and Connections are built from the nodes and edges.
    pub views: Vec<View>,
    /// Human-curated purpose-to-code links. They survive re-sync unchanged.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub capabilities: Vec<Capability>,
    /// Where the record collections are declared and the areas that group
    /// them; an atlas without it has no records view.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub records: Option<Records>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum CapabilityStatus {
    #[default]
    Settled,
    Proposed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Capability {
    /// Stable identity, independent of display name or implementation path.
    pub id: String,
    pub name: String,
    /// What this capability enables, as a reviewed human statement.
    pub purpose: String,
    #[serde(default)]
    pub status: CapabilityStatus,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub terms: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub parts: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub implementation: Vec<ImplementationTarget>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ImplementationTarget {
    /// A responsible part. The path may lie below a different package.
    pub part: String,
    /// A repository-relative source file or directory, checked separately.
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub symbol: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Records {
    /// Repo-relative path of a JSON Schema bundle. The collection-map key
    /// comes from rosetta.toml, with Rosetta and Virgil extensions accepted.
    pub file: String,
    /// Each collection belongs to exactly one area.
    pub areas: Vec<RecordArea>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RecordArea {
    /// Kebab-case and unique; `other` is where an unplaced collection goes.
    pub id: String,
    pub title: String,
    /// Collection names, or a prefix ending in `*` (`bot_*`).
    pub collections: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Project {
    pub name: String,
    /// `owner/name` of the repository.
    pub repo: String,
}

/// What a node is to a reader; the viewer colours by family.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "lowercase")]
pub enum Family {
    Surface,
    Service,
    Store,
    Agent,
    External,
}

impl Family {
    pub fn as_str(self) -> &'static str {
        match self {
            Family::Surface => "surface",
            Family::Service => "service",
            Family::Store => "store",
            Family::Agent => "agent",
            Family::External => "external",
        }
    }

    /// The heading over a group of parts of this family.
    pub fn plural(self) -> &'static str {
        match self {
            Family::Surface => "Surfaces",
            Family::Service => "Services",
            Family::Store => "Stores",
            Family::Agent => "Agents",
            Family::External => "External",
        }
    }
}

/// Where a part runs, which the pictures group it by. A part with no tier
/// (a crate, a directory) sits outside every group.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "lowercase")]
pub enum Tier {
    /// On the person's own machine: desktop and mobile apps, browser
    /// extensions, local daemons and their stores.
    Device,
    /// The repository's own services and resources in a cloud.
    Cloud,
    /// At the network edge: edge workers and realtime relays.
    Edge,
    /// The vendors the repository's services call: push, media and storage
    /// providers.
    Providers,
    /// Everything else outside the repo's own services.
    External,
}

impl Tier {
    pub fn as_str(self) -> &'static str {
        match self {
            Tier::Device => "device",
            Tier::Cloud => "cloud",
            Tier::Edge => "edge",
            Tier::Providers => "providers",
            Tier::External => "external",
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            Tier::Device => "Device",
            Tier::Cloud => "Cloud",
            Tier::Edge => "Edge",
            Tier::Providers => "Providers",
            Tier::External => "External",
        }
    }
}

/// What a node is in the repo. `file` appears only in a bundle, from the live
/// walk; the committed model never lists a file.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "lowercase")]
pub enum NodeKind {
    Surface,
    Service,
    Store,
    Agent,
    External,
    Group,
    Crate,
    Package,
    App,
    Directory,
    Resource,
    File,
    /// A record collection, read from the schema bundle.
    Collection,
    /// A curated group of record collections.
    Area,
}

impl NodeKind {
    pub fn as_str(self) -> &'static str {
        match self {
            NodeKind::Surface => "surface",
            NodeKind::Service => "service",
            NodeKind::Store => "store",
            NodeKind::Agent => "agent",
            NodeKind::External => "external",
            NodeKind::Group => "group",
            NodeKind::Crate => "crate",
            NodeKind::Package => "package",
            NodeKind::App => "app",
            NodeKind::Directory => "directory",
            NodeKind::Resource => "resource",
            NodeKind::File => "file",
            NodeKind::Collection => "collection",
            NodeKind::Area => "area",
        }
    }

    /// Whether a node of this kind is a part a reader meets: what runs,
    /// holds state, acts or sits outside, as opposed to a crate, a directory,
    /// a resource or a file.
    pub fn is_part(self) -> bool {
        matches!(
            self,
            NodeKind::Surface
                | NodeKind::Service
                | NodeKind::Store
                | NodeKind::Agent
                | NodeKind::External
        )
    }

    /// A capability can be owned by a runtime part or by a concrete code
    /// unit. Walked files and directories are implementation, not owners.
    pub fn is_capability_owner(self) -> bool {
        self.is_part() || matches!(self, NodeKind::Crate | NodeKind::Package | NodeKind::App)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Refs {
    /// A term id in `.rosetta/glossary.json`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub term: Option<String>,
    /// Rule ids in the configured local rulebook.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rules: Vec<String>,
    /// Principle ids in the configured local rulebook.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub principles: Vec<String>,
    /// Repo-relative paths.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub docs: Vec<String>,
}

impl Refs {
    pub fn is_empty(&self) -> bool {
        self.term.is_none()
            && self.rules.is_empty()
            && self.principles.is_empty()
            && self.docs.is_empty()
    }
}

/// Marks what `atlas sync` owns, naming the file it was read from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Derived {
    pub source: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Node {
    /// Kebab-case and unique; a derived node's id is the kebab of its path.
    pub id: String,
    pub label: String,
    pub family: Family,
    pub kind: NodeKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,
    /// Repo-relative path that must exist.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    /// One sentence saying what it is.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tier: Option<Tier>,
    #[serde(default, skip_serializing_if = "Refs::is_empty")]
    pub refs: Refs,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub derived: Option<Derived>,
}

#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    Serialize,
    Deserialize,
    JsonSchema,
    clap::ValueEnum,
)]
#[serde(rename_all = "lowercase")]
pub enum EdgeKind {
    /// A manifest dependency proves availability, not a runtime call or data access.
    Depends,
    Calls,
    Reads,
    Writes,
    Publishes,
    Delivers,
    Deploys,
    Contains,
    /// A record collection's ref field; its label is the field.
    Refs,
}

impl EdgeKind {
    /// Whether the edge moves state: what a data view draws.
    pub fn moves_data(self) -> bool {
        matches!(
            self,
            EdgeKind::Reads | EdgeKind::Writes | EdgeKind::Publishes | EdgeKind::Delivers
        )
    }

    pub fn as_str(self) -> &'static str {
        match self {
            EdgeKind::Depends => "depends on",
            EdgeKind::Calls => "calls",
            EdgeKind::Reads => "reads",
            EdgeKind::Writes => "writes",
            EdgeKind::Publishes => "publishes",
            EdgeKind::Delivers => "delivers",
            EdgeKind::Deploys => "deploys",
            EdgeKind::Contains => "contains",
            EdgeKind::Refs => "refs",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Edge {
    pub from: String,
    pub to: String,
    pub kind: EdgeKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub derived: Option<Derived>,
    /// Populated in a viewer bundle or explicitly supplied by a curator.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evidence: Option<EdgeEvidence>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceBasis {
    DeclaredIntent,
    ManifestDependency,
    SchemaFact,
    InfrastructureFact,
    SourceFact,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EdgeEvidence {
    pub basis: EvidenceBasis,
    pub source: String,
    pub extractor: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revision: Option<String>,
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "lowercase")]
pub enum ViewKind {
    /// Every part, grouped by tier, and the edges between them. Built.
    Overview,
    /// Numbered steps across parts. Declared.
    Flow,
    /// The cloud resources that hold state or run, grouped by the file that
    /// declares them, and what uses them. Built.
    Infra,
    /// Where state lives and how it moves: every read, write, publish and
    /// delivery and the parts at their ends, stores drawn as cylinders. Built.
    Data,
    /// The record collections: their areas, and per area its collections
    /// and their refs. Built when the model has a `records` section; the
    /// model never declares it.
    Records,
    /// One picture per part: what it uses and what uses it. Every bundle has
    /// exactly one, built from the edges; the model never declares it.
    Connections,
    /// The parts a person lists, grouped by tier, with the edges between
    /// them. Declared.
    Custom,
}

impl ViewKind {
    pub fn as_str(self) -> &'static str {
        match self {
            ViewKind::Overview => "overview",
            ViewKind::Flow => "flow",
            ViewKind::Infra => "infra",
            ViewKind::Data => "data",
            ViewKind::Records => "records",
            ViewKind::Connections => "connections",
            ViewKind::Custom => "custom",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Step {
    pub node: String,
    /// What happens at this step.
    pub label: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct View {
    pub id: String,
    pub kind: ViewKind,
    pub title: String,
    /// One line for the view menu.
    pub description: String,
    /// The nodes a custom view shows.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub include: Option<Vec<String>>,
    /// A flow's steps, numbered in order.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub steps: Option<Vec<Step>>,
}

impl Atlas {
    pub fn node(&self, id: &str) -> Option<&Node> {
        self.nodes.iter().find(|node| node.id == id)
    }
}
