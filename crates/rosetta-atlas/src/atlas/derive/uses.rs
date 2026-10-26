//! Parts read from what a workspace member ships and what it talks to: a
//! member with deploy evidence and no curated part becomes a part, and a
//! member that depends directly on a store's driver or an outside service's
//! SDK gets an edge to that store or service.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use regex::Regex;
use serde::Deserialize;

use super::{DeriveError, derived, path_id};
use crate::atlas::model::{Edge, EdgeKind, Family, Node, NodeKind, Refs, Tier};
use crate::workspace::{Manager, Member, Package};

fn read(root: &Path, path: &str) -> Result<String, DeriveError> {
    std::fs::read_to_string(root.join(path)).map_err(|source| DeriveError::Io {
        path: path.to_owned(),
        source,
    })
}

/// A part's path covers a member when it is the member's path or inside it.
fn covers(part: &str, member: &str) -> bool {
    part == member
        || part
            .strip_prefix(member)
            .is_some_and(|rest| rest.starts_with('/'))
}

/// The id of the part derived for a member.
pub fn part_id(member_path: &str) -> String {
    format!("part-{}", path_id(member_path))
}

/// Only the key that marks a browser extension's manifest.
#[derive(Deserialize)]
struct ExtensionManifest {
    manifest_version: Option<u64>,
}

/// What a member ships, read from the first evidence that says: the part's
/// kind, where it runs, and the file that says so.
fn deploy_evidence(
    root: &Path,
    member: &Member,
    tracked: &BTreeSet<&str>,
) -> Result<Option<(NodeKind, Tier, String)>, DeriveError> {
    let at = |file: &str| format!("{}/{file}", member.path);
    let has = |name: &str| member.packages.iter().any(|p| p.name == name);
    for file in ["wrangler.toml", "wrangler.json", "wrangler.jsonc"] {
        if tracked.contains(at(file).as_str()) {
            return Ok(Some((NodeKind::Service, Tier::Edge, at(file))));
        }
    }
    if member.manager == Manager::Bun && has("electron") {
        return Ok(Some((NodeKind::Surface, Tier::Device, member.manifest())));
    }
    let tauri = at("src-tauri/tauri.conf.json");
    if tracked.contains(tauri.as_str()) {
        return Ok(Some((NodeKind::Surface, Tier::Device, tauri)));
    }
    for file in ["manifest.json", "public/manifest.json", "src/manifest.json"] {
        let path = at(file);
        if tracked.contains(path.as_str())
            && serde_json::from_str::<ExtensionManifest>(&read(root, &path)?)
                .is_ok_and(|manifest| manifest.manifest_version.is_some())
        {
            return Ok(Some((NodeKind::Surface, Tier::Device, path)));
        }
    }
    if member.manager == Manager::Bun && has("next") {
        return Ok(Some((NodeKind::Surface, Tier::Cloud, member.manifest())));
    }
    let dockerfile = at("Dockerfile");
    if tracked.contains(dockerfile.as_str()) {
        return Ok(Some((NodeKind::Service, Tier::Cloud, dockerfile)));
    }
    Ok(None)
}

/// One part per member that ships something and that no curated part covers,
/// in member order.
pub fn deploy_parts(
    root: &Path,
    members: &[Member],
    tracked: &BTreeSet<&str>,
    curated: &[&Node],
) -> Result<Vec<Node>, DeriveError> {
    let mut parts = Vec::new();
    for member in members {
        let covered = curated
            .iter()
            .filter_map(|node| node.path.as_deref())
            .any(|path| covers(path, &member.path));
        if covered {
            continue;
        }
        let Some((kind, tier, source)) = deploy_evidence(root, member, tracked)? else {
            continue;
        };
        let family = match kind {
            NodeKind::Surface => Family::Surface,
            _ => Family::Service,
        };
        parts.push(Node {
            id: part_id(&member.path),
            label: member.name.clone(),
            family,
            kind,
            parent: None,
            path: Some(member.path.clone()),
            summary: None,
            tier: Some(tier),
            refs: Refs::default(),
            derived: derived(&source),
        });
    }
    Ok(parts)
}

/// A store or outside service that driver and SDK packages stand for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct Technology {
    id: &'static str,
    label: &'static str,
    family: Family,
}

const fn store(id: &'static str, label: &'static str) -> Technology {
    Technology {
        id,
        label,
        family: Family::Store,
    }
}

const fn outside(id: &'static str, label: &'static str) -> Technology {
    Technology {
        id,
        label,
        family: Family::External,
    }
}

const POSTGRES: Technology = store("postgres", "Postgres");
const SQLITE: Technology = store("sqlite", "SQLite");
const MYSQL: Technology = store("mysql", "MySQL");
const SQL_SERVER: Technology = store("sql-server", "SQL Server");
const REDIS: Technology = store("redis", "Redis");
const OBJECT_STORAGE: Technology = store("object-storage", "Object storage");
const MONGODB: Technology = store("mongodb", "MongoDB");

/// What a Prisma schema's datasource provider names.
fn prisma_provider(provider: &str) -> Option<Technology> {
    match provider {
        "postgresql" | "postgres" | "cockroachdb" => Some(POSTGRES),
        "mysql" => Some(MYSQL),
        "sqlite" => Some(SQLITE),
        "sqlserver" => Some(SQL_SERVER),
        "mongodb" => Some(MONGODB),
        _ => None,
    }
}

/// The technologies one direct dependency stands for. `prisma` is what the
/// member's Prisma schemas name, for `@prisma/client`.
fn technologies(manager: Manager, package: &Package, prisma: &[Technology]) -> Vec<Technology> {
    let name = package.name.as_str();
    let feature = |f: &str| package.features.iter().any(|on| on == f);
    match manager {
        Manager::Cargo => match name {
            "sqlx" => [("postgres", POSTGRES), ("sqlite", SQLITE), ("mysql", MYSQL)]
                .into_iter()
                .filter(|(f, _)| feature(f))
                .map(|(_, tech)| tech)
                .collect(),
            "tokio-postgres" | "postgres" => vec![POSTGRES],
            "rusqlite" | "libsql" => vec![SQLITE],
            "mysql_async" | "mysql" => vec![MYSQL],
            "redis" => vec![REDIS],
            "aws-sdk-s3" | "object_store" => vec![OBJECT_STORAGE],
            "mongodb" => vec![MONGODB],
            "async-stripe" => vec![outside("stripe", "Stripe")],
            "async-openai" => vec![outside("openai", "OpenAI")],
            "resend-rs" => vec![outside("resend", "Resend")],
            _ => Vec::new(),
        },
        Manager::Bun => match name {
            "pg" | "postgres" => vec![POSTGRES],
            "@prisma/client" => prisma.to_vec(),
            "better-sqlite3" | "libsql" | "@libsql/client" => vec![SQLITE],
            "mysql2" => vec![MYSQL],
            "redis" | "ioredis" => vec![REDIS],
            "@aws-sdk/client-s3" => vec![OBJECT_STORAGE],
            "mongodb" => vec![MONGODB],
            "stripe" => vec![outside("stripe", "Stripe")],
            "openai" => vec![outside("openai", "OpenAI")],
            "@anthropic-ai/sdk" => vec![outside("anthropic", "Anthropic")],
            "resend" => vec![outside("resend", "Resend")],
            "twilio" => vec![outside("twilio", "Twilio")],
            name if name.starts_with("@clerk/") => vec![outside("clerk", "Clerk")],
            name if name.starts_with("@slack/") => vec![outside("slack", "Slack")],
            _ => Vec::new(),
        },
    }
}

/// The datasource providers of the Prisma schemas below a member.
fn prisma_schemas(
    root: &Path,
    member: &Member,
    tracked: &[String],
    datasource: &Regex,
) -> Result<Vec<Technology>, DeriveError> {
    let mut found = BTreeSet::new();
    for file in tracked
        .iter()
        .filter(|file| file.ends_with(".prisma") && covers(file, &member.path))
    {
        for capture in datasource.captures_iter(&read(root, file)?) {
            found.extend(prisma_provider(&capture[1]));
        }
    }
    Ok(found.into_iter().collect())
}

/// What the members use: one edge per member and technology (`reads` and
/// `writes` to a store, `calls` to an outside service) labelled with the
/// packages that say so, and a node for each technology no curated node
/// stands for.
///
/// A driver names a technology, never an instance: two members that both use
/// SQLite or Postgres may keep entirely separate databases. So a curated
/// store stands only for the member it sits on (that member is the store
/// and gets no edge), every other member reaches the technology's node, and
/// a part whose reads or writes a person curated keeps only those. An
/// outside service is one vendor, so a curated node carrying its label
/// stands for it.
pub fn uses(
    root: &Path,
    members: &[Member],
    tracked: &[String],
    curated: &[&Node],
    keeps_curated_state: &BTreeSet<&str>,
    parts: &[Node],
) -> Result<(Vec<Node>, Vec<Edge>), DeriveError> {
    let datasource =
        Regex::new(r#"datasource\s+\w+\s*\{[^}]*?provider\s*=\s*"([^"]+)""#).map_err(|error| {
            DeriveError::Io {
                path: ".prisma".to_owned(),
                source: std::io::Error::other(error),
            }
        })?;
    // Each technology with the members that use it directly, and the
    // packages that say so.
    let mut users: BTreeMap<Technology, BTreeMap<&str, (&Member, BTreeSet<&str>)>> =
        BTreeMap::new();
    for member in members {
        let prisma = if member.packages.iter().any(|p| p.name == "@prisma/client") {
            prisma_schemas(root, member, tracked, &datasource)?
        } else {
            Vec::new()
        };
        for package in member.packages.iter().filter(|p| !p.dev) {
            for tech in technologies(member.manager, package, &prisma) {
                users
                    .entry(tech)
                    .or_default()
                    .entry(member.path.as_str())
                    .or_insert_with(|| (member, BTreeSet::new()))
                    .1
                    .insert(package.name.as_str());
            }
        }
    }

    let curated_parts: Vec<&Node> = curated
        .iter()
        .copied()
        .filter(|node| node.kind.is_part())
        .collect();
    let store_paths: BTreeSet<&str> = curated_parts
        .iter()
        .filter(|node| node.family == Family::Store)
        .filter_map(|node| node.path.as_deref())
        .collect();
    let from_of = |path: &str| -> String {
        curated_parts
            .iter()
            .copied()
            .chain(parts.iter())
            .find(|node| node.path.as_deref() == Some(path))
            .map_or_else(|| path_id(path), |node| node.id.clone())
    };
    let mut nodes = Vec::new();
    let mut edges = Vec::new();
    for (tech, by_member) in &users {
        let by_member: BTreeMap<&str, (&Member, &BTreeSet<&str>, String)> = by_member
            .iter()
            .filter(|(path, _)| tech.family != Family::Store || !store_paths.contains(**path))
            .map(|(path, (member, packages))| (*path, (*member, packages, from_of(path))))
            .filter(|(_, (_, _, from))| {
                tech.family != Family::Store || !keeps_curated_state.contains(from.as_str())
            })
            .collect();
        if by_member.is_empty() {
            continue;
        }
        let standing: Vec<&str> = match tech.family {
            Family::Store => Vec::new(),
            _ => curated_parts
                .iter()
                .filter(|node| node.family == tech.family)
                .filter(|node| node.label.eq_ignore_ascii_case(tech.label))
                .map(|node| node.id.as_str())
                .collect(),
        };
        let target = match standing.as_slice() {
            [id] => (*id).to_owned(),
            _ => {
                let id = format!("{}-{}", tech.family.as_str(), tech.id);
                let first = by_member.values().next().map(|(m, _, _)| m.manifest());
                nodes.push(Node {
                    id: id.clone(),
                    label: tech.label.to_owned(),
                    family: tech.family,
                    kind: match tech.family {
                        Family::Store => NodeKind::Store,
                        _ => NodeKind::External,
                    },
                    parent: None,
                    path: None,
                    summary: None,
                    tier: (tech.family == Family::External).then_some(Tier::External),
                    refs: Refs::default(),
                    derived: derived(&first.unwrap_or_default()),
                });
                id
            }
        };
        for (member, packages, from) in by_member.into_values() {
            let label = Some(packages.iter().copied().collect::<Vec<_>>().join(", "));
            // A package dependency establishes a possible integration only.
            // It cannot prove a call, a read or a write at runtime.
            let kinds: &[EdgeKind] = &[EdgeKind::Depends];
            for kind in kinds {
                edges.push(Edge {
                    from: from.clone(),
                    to: target.clone(),
                    kind: *kind,
                    label: label.clone(),
                    derived: derived(&member.manifest()),
                    evidence: None,
                });
            }
        }
    }
    nodes.sort_by(|a, b| a.id.cmp(&b.id));
    Ok((nodes, edges))
}
