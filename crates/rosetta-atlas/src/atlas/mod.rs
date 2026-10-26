//! The atlas: `.rosetta/atlas.json` is a typed model of what a repo is made
//! of. Curated parts, edges and custom views sit beside what sync derives
//! from the repo's files; files below a member are walked live into a bundle
//! whose built views the viewer renders.

pub mod assertions;
pub mod blurb;
pub mod bundle;
pub mod change;
pub mod derive;
pub mod emit;
pub mod export;
pub mod focus;
pub mod history;
pub mod model;
pub mod page;
pub mod records;
pub mod validate;
pub mod walk;

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path};

pub use bundle::{Bundle, BundleNode, build_bundle};
pub use model::{
    ATLAS_SCHEMA, ATLAS_SOURCE, Atlas, CONNECTIONS_VIEW, Edge, Family, Node, NodeKind, Project,
    RECORDS_VIEW, Tier, View, ViewKind,
};
pub use validate::{AtlasContext, AtlasError, validate_atlas};

use crate::diff::unified_diff;
use crate::glossary::Glossary;
use crate::json_text;
use crate::rulebook::{Rulebook, RulebookError};
use rosetta_core::config::{self, Config, ConfigError, RefKind};

#[derive(Debug, thiserror::Error)]
pub enum AtlasLoadError {
    #[error(transparent)]
    Config(#[from] ConfigError),
    #[error("{path}: {source}")]
    Io {
        path: String,
        source: std::io::Error,
    },
    #[error("{path}: {source}")]
    Json {
        path: String,
        source: serde_json::Error,
    },
    #[error(transparent)]
    Rulebook(#[from] RulebookError),
    #[error(transparent)]
    Derive(#[from] derive::DeriveError),
    #[error(transparent)]
    Walk(#[from] walk::WalkError),
    #[error(transparent)]
    Records(#[from] records::RecordsError),
    #[error(transparent)]
    History(#[from] history::HistoryError),
    #[error(transparent)]
    Format(#[from] json_text::FormatError),
    #[error("{ATLAS_SOURCE}: {} problem(s); run task atlas:check", .0.len())]
    Invalid(Vec<AtlasError>),
    #[error(
        "atlas derived structure differs from the current repository ({0} changes); run rosetta atlas sync, then rosetta atlas check"
    )]
    StaleDerived(usize),
    #[error("views: no view \"{0}\"")]
    UnknownView(String),
    #[error("nodes: no node \"{0}\"")]
    UnknownNode(String),
    #[error("views[{view}]: no source for root \"{root}\"; a view with roots takes --root <part>")]
    NoSource { view: String, root: String },
}

fn read(root: &Path, path: &str) -> Result<String, AtlasLoadError> {
    std::fs::read_to_string(root.join(path)).map_err(|source| AtlasLoadError::Io {
        path: path.to_owned(),
        source,
    })
}

pub fn load_atlas(root: &Path) -> Result<Atlas, AtlasLoadError> {
    load_atlas_from(root, crate::repo::SOURCE_DIR)
}

pub fn load_atlas_from(root: &Path, source_dir: &str) -> Result<Atlas, AtlasLoadError> {
    let path = format!("{source_dir}/atlas.json");
    parse_atlas(&read(root, &path)?, &path)
}

fn parse_atlas(text: &str, path: &str) -> Result<Atlas, AtlasLoadError> {
    serde_json::from_str(text).map_err(|source| AtlasLoadError::Json {
        path: path.to_owned(),
        source,
    })
}

/// The model a repo without `.rosetta/atlas.json` starts from: nothing
/// curated and no custom view, which the first sync fills with what it
/// derives. The project is named by the root package.json, else the root
/// directory; the repository by the `origin` remote, else the name.
pub fn starter_atlas(root: &Path) -> Result<Atlas, AtlasLoadError> {
    let name = match crate::workspace::root_package_name(root).map_err(derive::DeriveError::from)? {
        Some(name) => name,
        None => crate::workspace::root_cargo_name(root)
            .map_err(derive::DeriveError::from)?
            .or_else(|| {
                std::fs::canonicalize(root)
                    .ok()
                    .and_then(|path| path.file_name().map(|n| n.to_string_lossy().into_owned()))
            })
            .unwrap_or_else(|| derive::ROOT_ID.to_owned()),
    };
    let repo = crate::repo::origin_repository(root).unwrap_or_else(|| name.clone());
    Ok(Atlas {
        schema: ATLAS_SCHEMA.to_owned(),
        project: Project { name, repo },
        nodes: Vec::new(),
        edges: Vec::new(),
        views: Vec::new(),
        capabilities: Vec::new(),
        records: None,
    })
}

/// The glossary as the atlas reads it: typed, without the glossary's own
/// checks, which `glossary check` runs.
pub fn load_glossary_terms(root: &Path) -> Result<Glossary, AtlasLoadError> {
    load_glossary_terms_from(root, crate::repo::SOURCE_DIR)
}

pub fn load_glossary_terms_from(root: &Path, source_dir: &str) -> Result<Glossary, AtlasLoadError> {
    let path = format!("{source_dir}/glossary.json");
    serde_json::from_str(&read(root, &path)?)
        .map_err(|source| AtlasLoadError::Json { path, source })
}

pub fn repo_context(root: &Path) -> Result<AtlasContext<'_>, AtlasLoadError> {
    repo_context_from(root, crate::repo::SOURCE_DIR)
}

pub fn repo_context_from<'a>(
    root: &'a Path,
    source_dir: &str,
) -> Result<AtlasContext<'a>, AtlasLoadError> {
    let glossary_source = format!("{source_dir}/glossary.json");
    let term_ids: BTreeSet<String> = if root.join(&glossary_source).is_file() {
        load_glossary_terms_from(root, source_dir)?
            .terms
            .into_iter()
            .map(|term| term.id)
            .collect()
    } else {
        BTreeSet::new()
    };
    let config = Config::load(root, None)?;
    let rulebook = Rulebook::load_optional_for(root, &config)?;
    let mut rule_ids = rulebook
        .as_ref()
        .map(Rulebook::rule_ids)
        .unwrap_or_default();
    rule_ids.extend(config.ref_values(root, RefKind::Rule)?);
    let mut principle_ids = rulebook
        .as_ref()
        .map(Rulebook::principle_ids)
        .unwrap_or_default();
    principle_ids.extend(config.ref_values(root, RefKind::Principle)?);
    let document_ids = config.ref_values(root, RefKind::Document)?;
    Ok(AtlasContext {
        term_ids,
        rule_ids,
        principle_ids,
        rule_source: config.ref_source(RefKind::Rule),
        principle_source: config.ref_source(RefKind::Principle),
        path_exists: Box::new(move |path| config::document_ref_exists(root, &document_ids, path)),
    })
}

/// Every finding `atlas check` reports, as `path: why` lines.
pub fn check_atlas(root: &Path) -> Result<Vec<String>, AtlasLoadError> {
    check_atlas_from(root, crate::repo::SOURCE_DIR)
}

pub fn check_atlas_from(root: &Path, source_dir: &str) -> Result<Vec<String>, AtlasLoadError> {
    check_atlas_with_mode_from(root, source_dir, false)
}

/// Normal consistency checks, with optional completeness requirements for
/// teams that have curated the human-facing parts of their map.
pub fn check_atlas_with_mode_from(
    root: &Path,
    source_dir: &str,
    strict: bool,
) -> Result<Vec<String>, AtlasLoadError> {
    let atlas = match load_atlas_from(root, source_dir) {
        Ok(atlas) => atlas,
        Err(AtlasLoadError::Json { path, source }) => {
            return Ok(vec![format!("{path}: {source}")]);
        }
        Err(error) => return Err(error),
    };
    let context = repo_context_from(root, source_dir)?;
    let mut findings: Vec<String> = validate_atlas(&atlas, &context)
        .iter()
        .map(ToString::to_string)
        .collect();
    for capability in &atlas.capabilities {
        for (index, target) in capability.implementation.iter().enumerate() {
            if !target_exists(root, &target.path) {
                findings.push(format!(
                    "capabilities[{}].implementation[{index}].path: \"{}\" is not an existing path inside this repository; repair the link or remove it",
                    capability.id, target.path
                ));
            }
        }
    }
    findings.extend(derive::compare(&atlas, &derive::derive(root, &atlas)?));
    if let Some(section) = &atlas.records
        && root.join(&section.file).is_file()
    {
        let config = Config::load(root, None)?;
        match records::read_records(root, section, &config.records) {
            Ok(model) => findings.extend(model.findings),
            Err(error) => findings.push(error.to_string()),
        }
    }
    if findings.is_empty() && root.join(export::ATLAS_OUTPUT).is_file() {
        let expected = export::expected_from(root, source_dir)?;
        let committed = read(root, export::ATLAS_OUTPUT)?;
        if committed != expected {
            findings.push(format!(
                "{}: differs from the generated atlas; run rosetta atlas export",
                export::ATLAS_OUTPUT
            ));
            findings.push(unified_diff(
                &committed,
                &expected,
                "ATLAS.md (committed)",
                "ATLAS.md (generated)",
            ));
        }
    }
    if strict {
        let glossary_exists = root.join(source_dir).join("glossary.json").is_file();
        for node in &atlas.nodes {
            if !node.kind.is_part() {
                continue;
            }
            if node
                .summary
                .as_deref()
                .is_none_or(|value| value.trim().is_empty())
            {
                findings.push(format!(
                    "nodes[{}].summary: a part needs a summary",
                    node.id
                ));
            }
            if node.tier.is_none() {
                findings.push(format!("nodes[{}].tier: a part needs a tier", node.id));
            }
            if glossary_exists
                && matches!(
                    node.kind,
                    NodeKind::Surface | NodeKind::Service | NodeKind::Store
                )
                && node.refs.term.is_none()
            {
                findings.push(format!(
                    "nodes[{}].refs.term: a named part needs a glossary term",
                    node.id
                ));
            }
        }
    }
    Ok(findings)
}

/// Merges the derivation into the committed model and writes it back,
/// starting from `starter_atlas` when the repo has none. Returns whether the
/// file changed.
pub fn sync_atlas(root: &Path) -> Result<bool, AtlasLoadError> {
    sync_atlas_from(root, crate::repo::SOURCE_DIR)
}

pub fn sync_atlas_from(root: &Path, source_dir: &str) -> Result<bool, AtlasLoadError> {
    let path = format!("{source_dir}/atlas.json");
    let (before, atlas) = if root.join(&path).is_file() {
        let before = read(root, &path)?;
        let atlas = parse_atlas(&before, &path)?;
        (Some(before), atlas)
    } else {
        (None, starter_atlas(root)?)
    };
    let synced = derive::sync(&atlas, &derive::derive(root, &atlas)?);
    let text = json_text::to_pretty_string(&synced)?;
    if before.as_deref() == Some(text.as_str()) {
        return Ok(false);
    }
    let io = |source| AtlasLoadError::Io {
        path: path.clone(),
        source,
    };
    std::fs::create_dir_all(root.join(source_dir)).map_err(io)?;
    std::fs::write(root.join(&path), text).map_err(io)?;
    Ok(true)
}

/// The members the live walk descends into, from the committed model.
fn walk_roots(atlas: &Atlas, root: &Path) -> Result<Vec<walk::WalkRoot>, AtlasLoadError> {
    let members = derive::derive(root, atlas)?.members;
    let mut roots: Vec<_> = atlas
        .nodes
        .iter()
        .filter(|node| members.contains(&node.id))
        .filter_map(|node| {
            Some(walk::WalkRoot {
                id: node.id.clone(),
                path: node.path.clone()?,
                family: node.family,
            })
        })
        .collect();
    if roots.is_empty()
        && let Some(node) = atlas.nodes.iter().find(|node| node.id == derive::ROOT_ID)
    {
        roots.push(walk::WalkRoot {
            id: node.id.clone(),
            path: String::new(),
            family: node.family,
        });
    }
    Ok(roots)
}

/// The model checked, the live walk done, every view emitted and each part
/// dated by its last commit.
pub fn bundle_repo(root: &Path) -> Result<Bundle, AtlasLoadError> {
    bundle_repo_from(root, crate::repo::SOURCE_DIR)
}

pub fn bundle_repo_from(root: &Path, source_dir: &str) -> Result<Bundle, AtlasLoadError> {
    let atlas = load_atlas_from(root, source_dir)?;
    let context = repo_context_from(root, source_dir)?;
    let errors = validate_atlas(&atlas, &context);
    if !errors.is_empty() {
        return Err(AtlasLoadError::Invalid(errors));
    }
    // A saved derived edge must not acquire a fresh snapshot revision when
    // its manifest or schema source has changed since the last sync.
    let derived_changes = derive::compare(&atlas, &derive::derive(root, &atlas)?);
    if !derived_changes.is_empty() {
        return Err(AtlasLoadError::StaleDerived(derived_changes.len()));
    }
    let glossary_source = format!("{source_dir}/glossary.json");
    let glossary = if root.join(&glossary_source).is_file() {
        load_glossary_terms_from(root, source_dir)?
    } else {
        Glossary {
            schema: crate::glossary::model::GLOSSARY_SCHEMA.to_owned(),
            context: atlas.project.name.clone(),
            purpose: String::new(),
            sections: Vec::new(),
            terms: Vec::new(),
        }
    };
    let config = Config::load(root, None)?;
    let rulebook = Rulebook::load_optional_for(root, &config)?;
    let files = walk::tracked_files(root)?;
    let live = walk::walk(root, &walk_roots(&atlas, root)?, &files);
    let records = atlas
        .records
        .as_ref()
        .map(|section| records::read_records(root, section, &config.records))
        .transpose()?;
    let mut bundle = build_bundle(
        &atlas,
        &glossary,
        rulebook.as_ref(),
        &live,
        records.as_ref(),
    );
    decorate_bundle(root, source_dir, &mut bundle)?;
    let paths: BTreeSet<String> = bundle
        .nodes
        .iter()
        .filter_map(|node| node.path.clone())
        .collect();
    let covers: BTreeMap<&str, &str> = live
        .iter()
        .filter_map(|node| Some((node.module_dir.as_deref()?, node.path.as_str())))
        .collect();
    let dated =
        history::last_changed_covering(root, &paths.iter().map(String::as_str).collect(), &covers)?;
    for node in &mut bundle.nodes {
        node.updated = node.path.as_ref().and_then(|path| dated.get(path).cloned());
        // A walked file's blurb came from the walk's own read of it.
        if node.summary.is_none() && node.kind != model::NodeKind::File {
            node.summary = node
                .path
                .as_deref()
                .and_then(|path| blurb::of_path(root, path));
        }
    }
    Ok(bundle)
}

pub(crate) fn decorate_bundle(
    root: &Path,
    source_dir: &str,
    bundle: &mut Bundle,
) -> Result<(), AtlasLoadError> {
    bundle.path_scopes = Config::load(root, Some(source_dir))?.path_scopes;
    let snapshot = rosetta_core::snapshot::capture(root, source_dir, &bundle.project.repo);
    let revision = snapshot.fingerprint.clone();
    bundle.snapshot = Some(snapshot);
    let node_paths: BTreeMap<&str, &str> = bundle
        .nodes
        .iter()
        .filter_map(|node| Some((node.id.as_str(), node.path.as_deref()?)))
        .collect();
    for edge in &mut bundle.edges {
        if let Some(evidence) = &mut edge.evidence {
            if evidence.revision.is_none() {
                evidence.revision = Some(revision.clone());
            }
            continue;
        }
        let (basis, source, extractor) = match &edge.derived {
            None => (
                model::EvidenceBasis::DeclaredIntent,
                format!("{source_dir}/atlas.json"),
                "curation",
            ),
            Some(derived) if edge.kind == model::EdgeKind::Refs => (
                model::EvidenceBasis::SchemaFact,
                derived.source.clone(),
                "record schema",
            ),
            Some(derived) if edge.kind == model::EdgeKind::Depends => (
                model::EvidenceBasis::ManifestDependency,
                if derived.source.contains('/') {
                    derived.source.clone()
                } else {
                    node_paths.get(edge.from.as_str()).map_or_else(
                        || derived.source.clone(),
                        |path| format!("{path}/{}", derived.source),
                    )
                },
                "workspace manifest",
            ),
            Some(derived) if edge.kind == model::EdgeKind::Deploys => (
                model::EvidenceBasis::InfrastructureFact,
                derived.source.clone(),
                "infrastructure parser",
            ),
            Some(derived) => (
                model::EvidenceBasis::SourceFact,
                derived.source.clone(),
                "source parser",
            ),
        };
        edge.evidence = Some(model::EdgeEvidence {
            basis,
            source,
            extractor: extractor.to_owned(),
            revision: Some(revision.clone()),
        });
    }
    bundle.target_exists = bundle
        .capabilities
        .iter()
        .flat_map(|capability| capability.implementation.iter())
        .map(|target| (target.path.clone(), target_exists(root, &target.path)))
        .collect();
    Ok(())
}

/// True only for a source path that resolves within this checkout. It is
/// deliberately separate from the human claim that the path implements a
/// capability: existence is checkable, behavior is not.
pub fn target_exists(root: &Path, path: &str) -> bool {
    if path.is_empty()
        || !Path::new(path)
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
    {
        return false;
    }
    let (Ok(repo), Ok(target)) = (
        std::fs::canonicalize(root),
        std::fs::canonicalize(root.join(path)),
    ) else {
        return false;
    };
    target.starts_with(repo)
}

/// One view's Mermaid source; `root` picks the part or area a view with
/// roots shows, else the view's default root.
pub fn render_view(
    bundle: &Bundle,
    view: &str,
    root: Option<&str>,
) -> Result<String, AtlasLoadError> {
    let found = bundle
        .views
        .iter()
        .find(|candidate| candidate.id == view)
        .ok_or_else(|| AtlasLoadError::UnknownView(view.to_owned()))?;
    let key = root.unwrap_or(&found.default_root);
    found
        .sources
        .get(key)
        .cloned()
        .ok_or_else(|| AtlasLoadError::NoSource {
            view: view.to_owned(),
            root: key.to_owned(),
        })
}
