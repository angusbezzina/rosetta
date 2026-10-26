//! A local, read-only MCP adapter over the glossary and Atlas contracts.

use std::collections::{BTreeSet, VecDeque};
use std::path::PathBuf;

use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{
    CallToolResult, ListResourcesResult, PaginatedRequestParams, ReadResourceRequestParams,
    ReadResourceResponse, ReadResourceResult, Resource, ResourceContents, ServerCapabilities,
    ServerConfig,
};
use rmcp::service::{RequestContext, RoleServer};
use rmcp::{ErrorData as McpError, ServerHandler, ServiceExt, tool, tool_handler, tool_router};
use rosetta_atlas::atlas::{self, Bundle, NodeKind, change, focus};
use rosetta_atlas::glossary_context;
use rosetta_core::config::Config;
use rosetta_glossary::{self as glossary, Glossary};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Value, json};

const DEFAULT_MAX_CHARS: usize = 8_000;
const MIN_MAX_CHARS: usize = 256;
const MAX_MAX_CHARS: usize = 100_000;
const GLOSSARY_URI: &str = "rosetta://GLOSSARY.md";
const ATLAS_URI: &str = "rosetta://ATLAS.md";

#[derive(Clone)]
pub struct RosettaServer {
    root: PathBuf,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct KeyArgs {
    /// Stable term or part ID, or exact term name.
    key: String,
    /// Maximum characters in the tool's JSON object (default 8000).
    #[schemars(with = "usize")]
    max_chars: Option<usize>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct SearchArgs {
    /// Case-insensitive word or phrase to search for.
    query: String,
    #[schemars(with = "usize")]
    max_chars: Option<usize>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct LintArgs {
    /// Repository-relative path, used to select syntax and vocabulary context.
    path: String,
    text: String,
    #[schemars(with = "usize")]
    max_chars: Option<usize>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct PathsArgs {
    #[schemars(with = "Vec<String>")]
    paths: Option<Vec<String>>,
    /// Existing focused view reference; cannot be combined with paths.
    #[schemars(with = "String")]
    reference: Option<String>,
    #[schemars(with = "usize")]
    max_chars: Option<usize>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct ListArgs {
    /// Optional case-insensitive filter over part ID, label, path, or summary.
    #[schemars(with = "String")]
    query: Option<String>,
    #[schemars(with = "usize")]
    max_chars: Option<usize>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct PairArgs {
    from: String,
    to: String,
    /// Maximum number of links in a directed path, 1 to 5.
    #[schemars(with = "usize")]
    max_depth: Option<usize>,
    #[schemars(with = "usize")]
    max_chars: Option<usize>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct PathArgs {
    path: String,
    #[schemars(with = "usize")]
    max_chars: Option<usize>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct ViewArgs {
    /// Stable view ID, such as overview or connections.
    view: String,
    /// Required for a view keyed by a part, such as connections.
    #[schemars(with = "String")]
    root: Option<String>,
    #[schemars(with = "usize")]
    max_chars: Option<usize>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct OpenArgs {
    #[schemars(with = "String")]
    capability: Option<String>,
    /// Stable part or implementation node IDs.
    #[schemars(with = "Vec<String>")]
    roots: Option<Vec<String>>,
    /// Repository-relative paths.
    #[schemars(with = "Vec<String>")]
    paths: Option<Vec<String>>,
    /// capability, component (default), or implementation.
    #[schemars(with = "String")]
    level: Option<String>,
    #[schemars(with = "u8")]
    depth: Option<u8>,
    #[schemars(with = "usize")]
    max_nodes: Option<usize>,
    #[schemars(with = "usize")]
    max_edges: Option<usize>,
    #[schemars(with = "usize")]
    max_chars: Option<usize>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct ReferenceArgs {
    reference: String,
    #[schemars(with = "usize")]
    max_chars: Option<usize>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct ProposalArgs {
    /// Focused view reference that grounds this proposed overlay.
    reference: String,
    /// Declarative rosetta.impact-proposal/v1 object. It is never applied.
    #[schemars(with = "std::collections::BTreeMap<String, Value>")]
    proposal: Value,
    #[schemars(with = "usize")]
    max_chars: Option<usize>,
}

fn max_chars(value: Option<usize>) -> Result<usize, String> {
    let value = value.unwrap_or(DEFAULT_MAX_CHARS);
    if !(MIN_MAX_CHARS..=MAX_MAX_CHARS).contains(&value) {
        return Err(format!(
            "max_chars must be {MIN_MAX_CHARS}..{MAX_MAX_CHARS}"
        ));
    }
    Ok(value)
}

fn error(message: impl std::fmt::Display) -> CallToolResult {
    CallToolResult::structured_error(json!({"error": message.to_string()}))
}

/// Remove whole optional fields and items until the serialized JSON fits.
/// The identity fields remain, so an agent can reopen a fuller result.
fn bounded(mut value: Value, requested: Option<usize>) -> CallToolResult {
    let limit = match max_chars(requested) {
        Ok(limit) => limit,
        Err(message) => return error(message),
    };
    let Some(object) = value.as_object_mut() else {
        return error("internal: tool result must be a JSON object");
    };
    object.insert("omitted_for_budget".into(), json!(0));
    let mut omitted = 0usize;
    loop {
        if value.to_string().chars().count() <= limit {
            return CallToolResult::structured(value);
        }
        let Some(object) = value.as_object_mut() else {
            break;
        };
        let mut shortened = false;
        for key in [
            "diagram",
            "connections_diagram",
            "text",
            "summary",
            "definition",
        ] {
            if object.remove(key).is_some() {
                shortened = true;
                omitted += 1;
                break;
            }
        }
        if !shortened {
            for key in [
                "nodes",
                "edges",
                "terms",
                "parts",
                "paths",
                "findings",
                "results",
                "incoming",
                "outgoing",
                "matches",
                "intents",
                "capabilities",
                "implementation_links",
                "target_exists",
            ] {
                if let Some(items) = object.get_mut(key).and_then(Value::as_array_mut)
                    && items.pop().is_some()
                {
                    shortened = true;
                    omitted += 1;
                    break;
                }
            }
        }
        if !shortened {
            break;
        }
        object.insert("omitted_for_budget".into(), json!(omitted));
    }
    error("max_chars is too small to retain identity and snapshot; increase it")
}

impl RosettaServer {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    fn config(&self) -> Result<Config, String> {
        Config::load(&self.root, None).map_err(|e| e.to_string())
    }

    fn bundle(&self) -> Result<Bundle, String> {
        let config = self.config()?;
        atlas::bundle_repo_from(&self.root, &config.source.directory).map_err(|e| e.to_string())
    }

    fn glossary(&self) -> Result<Glossary, String> {
        let config = self.config()?;
        let context = glossary::repo_context(&self.root).map_err(|e| e.to_string())?;
        let source = config.source_file(&self.root, "glossary.json");
        let relative = source.strip_prefix(&self.root).map_err(|e| e.to_string())?;
        let source = relative.to_string_lossy();
        let result = glossary::load_glossary_from(&self.root, &source, &context);
        result.glossary.ok_or_else(|| {
            result
                .problems
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join("; ")
        })
    }

    fn snapshot(bundle: &Bundle) -> Value {
        json!(bundle.snapshot)
    }

    fn focus_value(view: &focus::FocusedView) -> Value {
        json!({
            "schema": view.schema, "id": view.id, "reference": view.reference,
            "snapshot": view.snapshot, "level": view.query.level, "query": view.query,
            "capabilities": view.capabilities, "nodes": view.nodes,
            "edges": view.edges, "terms": view.terms,
            "diagram": view.diagram, "implementation_links": view.implementation_links,
            "target_exists": view.target_exists, "omissions": view.omissions
        })
    }

    fn select(&self, query: focus::Query) -> Result<focus::FocusedView, String> {
        focus::select(&self.bundle()?, query).map_err(|e| e.to_string())
    }

    fn read_reference(&self, reference: &str) -> Result<focus::FocusedView, String> {
        focus::reopen(&self.bundle()?, reference).map_err(|e| e.to_string())
    }

    fn edge_result(&self, key: String, incoming: bool, limit: Option<usize>) -> CallToolResult {
        let bundle = match self.bundle() {
            Ok(bundle) => bundle,
            Err(e) => return error(e),
        };
        let Some(part) = bundle.nodes.iter().find(|node| node.id == key) else {
            return error(format!("unknown part {key}"));
        };
        let edges: Vec<_> = bundle
            .edges
            .iter()
            .filter(|edge| {
                if incoming {
                    edge.to == key
                } else {
                    edge.from == key
                }
            })
            .cloned()
            .collect();
        bounded(
            json!({"schema":"rosetta.connections/v1", "part": part.id, "snapshot": Self::snapshot(&bundle), "direction": if incoming { "incoming" } else { "outgoing" }, "edges": edges, "total": edges.len()}),
            limit,
        )
    }
}

#[tool_router]
impl RosettaServer {
    #[tool(
        name = "define-term",
        description = "Define a term by stable ID or exact name, including status and avoided words"
    )]
    fn define_term(&self, Parameters(args): Parameters<KeyArgs>) -> CallToolResult {
        let glossary = match self.glossary() {
            Ok(g) => g,
            Err(e) => return error(e),
        };
        let Some(term) = glossary::find_term(&glossary, &args.key) else {
            return error(format!("unknown term {}", args.key));
        };
        bounded(
            json!({"schema":"rosetta.term/v1", "term": term}),
            args.max_chars,
        )
    }

    #[tool(
        name = "search-terms",
        description = "Search glossary IDs, names and definitions"
    )]
    fn search_terms(&self, Parameters(args): Parameters<SearchArgs>) -> CallToolResult {
        let glossary = match self.glossary() {
            Ok(g) => g,
            Err(e) => return error(e),
        };
        let query = args.query.to_lowercase();
        let terms: Vec<_> = glossary
            .terms
            .iter()
            .filter(|t| {
                [t.id.as_str(), &t.name, &t.definition]
                    .iter()
                    .any(|v| v.to_lowercase().contains(&query))
            })
            .collect();
        bounded(
            json!({"schema":"rosetta.term-search/v1", "query": args.query, "total": terms.len(), "terms": terms}),
            args.max_chars,
        )
    }

    #[tool(
        name = "lint-text",
        description = "Lint supplied copy using the repository glossary and path context"
    )]
    fn lint_text(&self, Parameters(args): Parameters<LintArgs>) -> CallToolResult {
        let glossary = match self.glossary() {
            Ok(g) => g,
            Err(e) => return error(e),
        };
        let config = match self.config() {
            Ok(c) => c,
            Err(e) => return error(e),
        };
        let contexts = match config.contexts_for_path(&args.path) {
            Ok(c) => c,
            Err(e) => return error(e),
        };
        let linter = match glossary::lint::Linter::new(&glossary) {
            Ok(l) => l,
            Err(e) => return error(e),
        };
        let findings = match linter.lint_text(&args.path, &args.text, &contexts) {
            Ok(f) => f,
            Err(e) => return error(e),
        };
        bounded(
            json!({"schema":"rosetta.lint/v1", "path": args.path, "contexts": contexts, "total": findings.len(), "findings": findings}),
            args.max_chars,
        )
    }

    #[tool(
        name = "terms-for-paths",
        description = "Select relevant terms for repository paths or an existing view reference"
    )]
    fn terms_for_paths(&self, Parameters(args): Parameters<PathsArgs>) -> CallToolResult {
        let glossary = match self.glossary() {
            Ok(g) => g,
            Err(e) => return error(e),
        };
        let config = match self.config() {
            Ok(c) => c,
            Err(e) => return error(e),
        };
        match glossary_context::select(
            &self.root,
            &config,
            &glossary,
            &args.paths.unwrap_or_default(),
            args.reference.as_deref(),
        ) {
            Ok(report) => bounded(json!(report), args.max_chars),
            Err(e) => error(e),
        }
    }

    #[tool(
        name = "list-parts",
        description = "List stable Atlas part IDs and labels; filter by a word or phrase"
    )]
    fn list_parts(&self, Parameters(args): Parameters<ListArgs>) -> CallToolResult {
        let bundle = match self.bundle() {
            Ok(b) => b,
            Err(e) => return error(e),
        };
        let query = args.query.unwrap_or_default().to_lowercase();
        let parts: Vec<_> = bundle
            .nodes
            .iter()
            .filter(|n| !matches!(n.kind, NodeKind::File | NodeKind::Directory | NodeKind::Group))
            .filter(|n| {
                [&n.id, &n.label, n.path.as_ref().unwrap_or(&n.id), n.summary.as_ref().unwrap_or(&n.label)]
                    .iter()
                    .any(|v| v.to_lowercase().contains(&query))
            })
            .map(|n| json!({"id":n.id,"label":n.label,"kind":n.kind,"path":n.path,"summary":n.summary}))
            .collect();
        bounded(
            json!({"schema":"rosetta.parts/v1", "snapshot":Self::snapshot(&bundle), "total":parts.len(), "parts":parts}),
            args.max_chars,
        )
    }

    #[tool(
        name = "read-part",
        description = "Read one Atlas part with its grounded connections and a reopenable view reference"
    )]
    fn read_part(&self, Parameters(args): Parameters<KeyArgs>) -> CallToolResult {
        let bundle = match self.bundle() {
            Ok(b) => b,
            Err(e) => return error(e),
        };
        let Some(part) = bundle.nodes.iter().find(|n| n.id == args.key) else {
            return error(format!("unknown part {}", args.key));
        };
        let view = match focus::select(
            &bundle,
            focus::Query {
                roots: vec![args.key],
                ..Default::default()
            },
        ) {
            Ok(v) => v,
            Err(e) => return error(e),
        };
        bounded(
            json!({"schema":"rosetta.part/v1", "part":part, "reference":view.reference, "snapshot":view.snapshot, "incoming":bundle.edges.iter().filter(|e| e.to==part.id).collect::<Vec<_>>(), "outgoing":bundle.edges.iter().filter(|e| e.from==part.id).collect::<Vec<_>>(), "omissions":view.omissions}),
            args.max_chars,
        )
    }

    #[tool(
        name = "incomers",
        description = "Show evidence-backed connections ending at a stable part ID; e.g. readers of a store"
    )]
    fn incomers(&self, Parameters(args): Parameters<KeyArgs>) -> CallToolResult {
        self.edge_result(args.key, true, args.max_chars)
    }

    #[tool(
        name = "outgoers",
        description = "Show evidence-backed connections starting at a stable part ID"
    )]
    fn outgoers(&self, Parameters(args): Parameters<KeyArgs>) -> CallToolResult {
        self.edge_result(args.key, false, args.max_chars)
    }

    #[tool(
        name = "paths-between",
        description = "Find directed connection paths between two stable part IDs"
    )]
    fn paths_between(&self, Parameters(args): Parameters<PairArgs>) -> CallToolResult {
        let bundle = match self.bundle() {
            Ok(b) => b,
            Err(e) => return error(e),
        };
        if !bundle.nodes.iter().any(|n| n.id == args.from)
            || !bundle.nodes.iter().any(|n| n.id == args.to)
        {
            return error("from and to must be existing node IDs");
        }
        let depth = args.max_depth.unwrap_or(3);
        if !(1..=5).contains(&depth) {
            return error("max_depth must be 1..5");
        }
        let mut queue = VecDeque::from([(vec![args.from.clone()], Vec::<atlas::Edge>::new())]);
        let mut paths = Vec::new();
        let mut expanded = 0usize;
        while let Some((path, evidence)) = queue.pop_front() {
            expanded += 1;
            if expanded > 10_000 {
                break;
            }
            let Some(last) = path.last() else { continue };
            if *last == args.to {
                paths.push(json!({"nodes":path,"edges":evidence}));
                if paths.len() >= 50 {
                    break;
                }
                continue;
            }
            if path.len() > depth {
                continue;
            }
            for edge in bundle.edges.iter().filter(|e| &e.from == last) {
                if !path.contains(&edge.to) {
                    let mut next = path.clone();
                    next.push(edge.to.clone());
                    let mut next_evidence = evidence.clone();
                    next_evidence.push(edge.clone());
                    queue.push_back((next, next_evidence));
                }
            }
        }
        bounded(
            json!({"schema":"rosetta.paths/v1", "from":args.from, "to":args.to, "snapshot":Self::snapshot(&bundle), "paths":paths, "total_found":paths.len(), "max_depth":depth, "search_truncated": expanded > 10_000 || !queue.is_empty()}),
            args.max_chars,
        )
    }

    #[tool(
        name = "impact",
        description = "Show Atlas parts and connections affected by a repository-relative path"
    )]
    fn impact(&self, Parameters(args): Parameters<PathArgs>) -> CallToolResult {
        let bundle = match self.bundle() {
            Ok(b) => b,
            Err(e) => return error(e),
        };
        match change::impact(&bundle, &args.path) {
            Ok(report) => bounded(json!(report), args.max_chars),
            Err(e) => error(e),
        }
    }

    #[tool(
        name = "render-view",
        description = "Return the current Mermaid source for a named Atlas view and optional part root"
    )]
    fn render_view(&self, Parameters(args): Parameters<ViewArgs>) -> CallToolResult {
        let bundle = match self.bundle() {
            Ok(b) => b,
            Err(e) => return error(e),
        };
        match atlas::render_view(&bundle, &args.view, args.root.as_deref()) {
            Ok(diagram) => bounded(
                json!({"schema":"rosetta.mermaid/v1", "view":args.view, "root":args.root, "snapshot":Self::snapshot(&bundle), "diagram":diagram}),
                args.max_chars,
            ),
            Err(e) => error(e),
        }
    }

    #[tool(
        name = "open-view",
        description = "Open the shared focused view by capability, path or stable part ID; returns reference, snapshot, evidence and omissions"
    )]
    fn open_view(&self, Parameters(args): Parameters<OpenArgs>) -> CallToolResult {
        let level = match args.level.as_deref().unwrap_or("component") {
            "capability" => focus::Level::Capability,
            "component" => focus::Level::Component,
            "implementation" => focus::Level::Implementation,
            other => return error(format!("unknown level {other}")),
        };
        let query = focus::Query {
            capability: args.capability,
            roots: args.roots.unwrap_or_default(),
            paths: args.paths.unwrap_or_default(),
            level,
            depth: args.depth.unwrap_or(1),
            max_nodes: args.max_nodes.unwrap_or(24),
            max_edges: args.max_edges.unwrap_or(48),
            ..Default::default()
        };
        match self.select(query) {
            Ok(view) => bounded(Self::focus_value(&view), args.max_chars),
            Err(e) => error(e),
        }
    }

    #[tool(
        name = "read-view",
        description = "Reopen a focused view reference on its original snapshot; stale references are rejected"
    )]
    fn read_view(&self, Parameters(args): Parameters<ReferenceArgs>) -> CallToolResult {
        match self.read_reference(&args.reference) {
            Ok(view) => bounded(Self::focus_value(&view), args.max_chars),
            Err(e) => error(e),
        }
    }

    #[tool(
        name = "propose-change",
        description = "Validate and echo a declarative proposed overlay for review; never accept or apply it"
    )]
    fn propose_change(&self, Parameters(args): Parameters<ProposalArgs>) -> CallToolResult {
        let view = match self.read_reference(&args.reference) {
            Ok(v) => v,
            Err(e) => return error(e),
        };
        let proposal: change::IntentProposal = match serde_json::from_value(args.proposal) {
            Ok(p) => p,
            Err(e) => return error(e),
        };
        if proposal.schema != change::INTENT_SCHEMA {
            return error(format!("expected schema {}", change::INTENT_SCHEMA));
        }
        if view.snapshot.head.as_deref().unwrap_or("") != proposal.base_commit {
            return error("proposal base_commit differs from the view's HEAD");
        }
        if view.snapshot.model_revision != proposal.base_model_revision {
            return error("proposal base_model_revision differs from the view's model");
        }
        if proposal.intents.is_empty() {
            return error("proposal needs at least one intent");
        }
        let bundle = match self.bundle() {
            Ok(b) => b,
            Err(e) => return error(e),
        };
        let node_ids: BTreeSet<_> = bundle.nodes.iter().map(|n| n.id.as_str()).collect();
        let term_ids: BTreeSet<_> = bundle.terms.iter().map(|t| t.id.as_str()).collect();
        let capability_ids: BTreeSet<_> =
            bundle.capabilities.iter().map(|c| c.id.as_str()).collect();
        let invalid: Vec<_> = proposal
            .intents
            .iter()
            .filter_map(|intent| {
                use change::Intent;
                let bad = match intent {
                    Intent::Relationship { from, to, .. } => {
                        !node_ids.contains(from.as_str()) || !node_ids.contains(to.as_str())
                    }
                    Intent::CapabilityPart {
                        capability, part, ..
                    } => {
                        !capability_ids.contains(capability.as_str())
                            || !node_ids.contains(part.as_str())
                    }
                    Intent::Responsibility { part, .. } => !node_ids.contains(part.as_str()),
                    Intent::TermDefinition { term, .. } => !term_ids.contains(term.as_str()),
                    Intent::Claim { .. } => false,
                };
                bad.then(|| format!("unknown ID in {intent:?}"))
            })
            .collect();
        if !invalid.is_empty() {
            return error(invalid.join("; "));
        }
        bounded(
            json!({"schema":"rosetta.proposal-review/v1", "status":"proposed_unverified", "reference":view.reference, "snapshot":view.snapshot, "proposal":proposal, "note":"No model or source file was changed; observed edges remain separate."}),
            args.max_chars,
        )
    }
}

#[tool_handler]
impl ServerHandler for RosettaServer {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().enable_resources().build())
            .with_instructions("Read-only local repository model. Open a focused view and cite its reference and stable part or term IDs. Proposed overlays are unverified and never applied.")
    }

    fn list_resources(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> impl Future<Output = Result<ListResourcesResult, McpError>> + Send + '_ {
        std::future::ready(Ok(ListResourcesResult::with_all_items(vec![
            Resource::new(GLOSSARY_URI, "GLOSSARY.md").with_mime_type("text/markdown"),
            Resource::new(ATLAS_URI, "ATLAS.md").with_mime_type("text/markdown"),
        ])))
    }

    fn read_resource(
        &self,
        request: ReadResourceRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> impl Future<Output = Result<ReadResourceResponse, McpError>> + Send + '_ {
        let result = match request.uri.as_str() {
            GLOSSARY_URI => self.glossary().map(|g| {
                glossary::render_glossary_for(
                    &g,
                    &format!(
                        "{}/glossary.json",
                        self.config()
                            .map(|c| c.source.directory)
                            .unwrap_or_else(|_| ".rosetta".into())
                    ),
                )
            }),
            ATLAS_URI => self.config().and_then(|config| {
                atlas::export::expected_from(&self.root, &config.source.directory)
                    .map_err(|e| e.to_string())
            }),
            _ => Err(format!("unknown resource {}", request.uri)),
        };
        std::future::ready(
            result
                .map(|text| {
                    ReadResourceResult::new(vec![ResourceContents::text(text, request.uri)]).into()
                })
                .map_err(|e| McpError::invalid_params(e, None)),
        )
    }
}

pub async fn serve(root: PathBuf) -> Result<(), Box<dyn std::error::Error>> {
    let root = root.canonicalize()?;
    let config = Config::load(&root, None)?;
    for name in ["glossary.json", "atlas.json"] {
        let source = config.source_file(&root, name);
        if !source.is_file() {
            return Err(format!(
                "{} is missing; initialize this local repository first",
                source.display()
            )
            .into());
        }
    }
    let server = RosettaServer::new(root)
        .serve(rmcp::transport::stdio())
        .await?;
    server.waiting().await?;
    Ok(())
}
