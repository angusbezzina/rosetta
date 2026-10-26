// The atlas bundle as rosetta-atlas serializes
// it: snake_case keys, optional fields absent rather than null.

export const ATLAS_BUNDLE_SCHEMA = "rosetta.atlas-bundle/v1";
export const LEGACY_ATLAS_BUNDLE_SCHEMA = "virgil.atlas-bundle/v1";

export type Family = "surface" | "service" | "store" | "agent" | "external";

export type NodeKind =
  | "surface"
  | "service"
  | "store"
  | "agent"
  | "external"
  | "group"
  | "crate"
  | "package"
  | "app"
  | "directory"
  | "resource"
  | "file"
  | "collection"
  | "area";

/**
 * A view with roots draws one source per root it has; connections has one
 * per node with an edge and none without a root, so it is reached by
 * clicking a node rather than from the view menu. A custom view is one the
 * atlas declares with the parts it includes, drawn as the overview is.
 */
export type ViewKind =
  | "overview"
  | "custom"
  | "flow"
  | "data"
  | "records"
  | "infra"
  | "connections";

export type EdgeKind =
  | "depends"
  | "calls"
  | "reads"
  | "writes"
  | "publishes"
  | "delivers"
  | "deploys"
  | "contains"
  /** A record collection's ref field; the edge's label is the field's name. */
  | "refs";

/** Where a part runs; the emitter draws a tier as a subgraph. */
export type Tier = "device" | "cloud" | "edge" | "providers" | "external";

export interface AtlasProject {
  name: string;
  repo: string;
}

export interface AtlasView {
  id: string;
  kind: ViewKind;
  title: string;
  description: string;
  /** A node id, or "" for a view without a root. */
  default_root: string;
  /** Mermaid text keyed by root id ("" for a view without a root). */
  sources: Record<string, string>;
}

export interface NodeRefs {
  term?: string;
  rules: string[];
  principles: string[];
  docs: string[];
}

export interface AtlasNode {
  id: string;
  label: string;
  family: Family;
  kind: NodeKind;
  parent?: string;
  path?: string;
  summary?: string;
  lines?: number;
  tier?: Tier;
  /** The last commit that touched the node's path. */
  updated?: { at: string /* RFC 3339 */; subject: string };
  has_children: boolean;
  refs: NodeRefs;
}

export interface AtlasEdge {
  from: string;
  to: string;
  kind: EdgeKind;
  label?: string;
  /** Where a derived edge came from; the viewer does not show it. */
  derived?: { source: string };
  evidence?: {
    basis: "declared_intent" | "manifest_dependency" | "schema_fact" | "infrastructure_fact" | "source_fact";
    source: string;
    extractor: string;
    revision?: string;
  };
}

export interface AtlasCapability {
  id: string;
  name: string;
  purpose: string;
  status: "settled" | "proposed";
  terms: string[];
  parts: string[];
  implementation: Array<{ part: string; path: string; symbol?: string }>;
}

export interface AvoidWord {
  word: string;
  except: string[];
}

export type TermStatus = "settled" | "proposed";

export interface AtlasTerm {
  id: string;
  name: string;
  definition: string;
  avoid: AvoidWord[];
  status: TermStatus;
  contexts?: string[];
  /** Whether a message draws this term's name with its card; the daemon decides. */
  marked: boolean;
}

export interface AtlasRule {
  id: string;
  kind: "rule" | "principle";
  title: string;
}

export interface AtlasBundle {
  schema: typeof ATLAS_BUNDLE_SCHEMA | typeof LEGACY_ATLAS_BUNDLE_SCHEMA;
  project: AtlasProject;
  views: AtlasView[];
  nodes: AtlasNode[];
  edges: AtlasEdge[];
  terms: AtlasTerm[];
  rules: AtlasRule[];
  capabilities?: AtlasCapability[];
  focused_view_id?: string;
  path_scopes?: { context: string; paths: string[] }[];
  /** Observed existence of declared implementation paths in this checkout. */
  target_exists?: Record<string, boolean>;
  snapshot?: {
    repository: string;
    head?: string | null;
    dirty?: boolean | null;
    model_revision: string;
    fingerprint: string;
    coverage: "git_worktree" | "model_only";
  };
}

export interface FocusedView {
  schema: "rosetta.focus/v1";
  id: string;
  reference: string;
  snapshot: NonNullable<AtlasBundle["snapshot"]>;
  query: { capability?: string; roots: string[]; paths: string[]; level: string; depth: number };
  capabilities: AtlasCapability[];
  nodes: AtlasNode[];
  edges: AtlasEdge[];
  terms: AtlasTerm[];
  target_exists: Record<string, boolean>;
  implementation_links: Array<{ part: string; node: string; path: string }>;
  diagram: string;
  omissions: { nodes_over_budget: number; edges_over_budget: number; edges_filtered: number; beyond_depth: number; implementation_links_not_drawn: number };
}
