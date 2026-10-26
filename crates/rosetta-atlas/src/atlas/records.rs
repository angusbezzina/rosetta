//! The record collections, read from the schema bundle a repo names in its
//! atlas (`records.file`): one node per collection inside the curated area
//! it belongs to, and one `refs` edge per ref field. They are read whenever
//! a bundle is built, never committed: the schema bundle is their source.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use rosetta_core::config::RecordsConfig;
use serde::Deserialize;
use serde_json::{Map, Value};

use super::bundle::{BundleNode, BundleRefs};
use super::model::{Derived, Edge, EdgeKind, Family, NodeKind, Records};

/// A collection's node id is this followed by its name.
pub const COLLECTION_PREFIX: &str = "records:";
/// An area's node id is this followed by its curated id.
pub const AREA_PREFIX: &str = "records-area:";
/// Where a collection no curated area claims is drawn, so it still shows
/// while `atlas check` asks for it to be placed.
pub const OTHER_AREA: &str = "other";

#[derive(Debug, thiserror::Error)]
pub enum RecordsError {
    #[error("{path}: {source}")]
    Read {
        path: String,
        source: std::io::Error,
    },
    #[error("{path}: {source}")]
    Parse {
        path: String,
        source: serde_json::Error,
    },
    #[error("{path}: resolves outside the repository")]
    UnsafePath { path: String },
    #[error("{path} at {location}: {why}")]
    Shape {
        path: String,
        location: String,
        why: String,
    },
}

/// The parts of the schema bundle the atlas reads.
#[derive(Debug)]
struct SchemaBundle {
    collections: BTreeMap<String, CollectionSchema>,
}

#[derive(Debug)]
struct CollectionSchema {
    description: String,
    scope: Scope,
    properties: BTreeMap<String, FieldSchema>,
}

/// Who may read a collection's records, which a records picture shows as a
/// `scope-` class on the collection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Scope {
    Container,
    Private,
    Recipient,
}

impl Scope {
    pub fn as_str(self) -> &'static str {
        match self {
            Scope::Container => "container",
            Scope::Private => "private",
            Scope::Recipient => "recipient",
        }
    }
}

/// A field, or a list item or variant inside one: refs may sit at any depth.
#[derive(Debug)]
struct FieldSchema {
    ref_to: Option<RefTo>,
    items: Option<Box<FieldSchema>>,
    properties: BTreeMap<String, FieldSchema>,
    one_of: Vec<FieldSchema>,
}

/// The collections a ref may point at, or any record at all, which names no
/// collection to draw an edge to.
#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum RefTo {
    Collections(Vec<String>),
    Any(AnyCollection),
}

#[derive(Debug, Deserialize)]
enum AnyCollection {
    #[serde(rename = "any")]
    Any,
}

/// The collections as the bundle draws them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RecordsModel {
    /// The areas in curated order, then `other` when anything fell into it.
    pub areas: Vec<BundleNode>,
    /// Every collection, by name.
    pub collections: Vec<BundleNode>,
    pub scopes: BTreeMap<String, Scope>,
    pub edges: Vec<Edge>,
    /// What `atlas check` reports: placements and refs the curation or the
    /// schema leaves wrong.
    pub findings: Vec<String>,
}

pub fn collection_id(name: &str) -> String {
    format!("{COLLECTION_PREFIX}{name}")
}

pub fn area_id(id: &str) -> String {
    format!("{AREA_PREFIX}{id}")
}

/// Whether a curated entry names a collection: its exact name, or a prefix
/// ending in `*`.
fn claims(pattern: &str, name: &str) -> bool {
    match pattern.strip_suffix('*') {
        Some(prefix) => name.starts_with(prefix),
        None => pattern == name,
    }
}

/// The description's first sentence.
fn first_sentence(text: &str) -> Option<String> {
    let text = text.trim();
    if text.is_empty() {
        return None;
    }
    let end = text.find(". ").map_or(text.len(), |at| at + 1);
    Some(text[..end].to_owned())
}

/// Every ref below a field, as the dotted path of property names and the
/// collections it names.
fn refs_in<'a>(path: &str, field: &'a FieldSchema, out: &mut Vec<(String, &'a [String])>) {
    if let Some(RefTo::Collections(targets)) = &field.ref_to {
        out.push((path.to_owned(), targets));
    }
    if let Some(items) = &field.items {
        refs_in(path, items, out);
    }
    for variant in &field.one_of {
        refs_in(path, variant, out);
    }
    for (name, inner) in &field.properties {
        refs_in(&format!("{path}.{name}"), inner, out);
    }
}

fn node(id: String, label: String, kind: NodeKind, parent: Option<String>) -> BundleNode {
    BundleNode {
        id,
        label,
        family: Family::Store,
        kind,
        parent,
        path: None,
        summary: None,
        lines: None,
        has_children: false,
        tier: None,
        updated: None,
        refs: BundleRefs::default(),
    }
}

fn shape(path: &str, location: &str, why: impl Into<String>) -> RecordsError {
    RecordsError::Shape {
        path: path.to_owned(),
        location: location.to_owned(),
        why: why.into(),
    }
}

fn object<'a>(
    value: &'a Value,
    path: &str,
    location: &str,
) -> Result<&'a Map<String, Value>, RecordsError> {
    value
        .as_object()
        .ok_or_else(|| shape(path, location, "must be a JSON object"))
}

fn extension<'a>(
    object: &'a Map<String, Value>,
    configured: &str,
    generic: &str,
    legacy: &str,
) -> Option<&'a Value> {
    object
        .get(configured)
        .or_else(|| object.get(generic))
        .or_else(|| object.get(legacy))
}

fn field_schema(
    value: &Value,
    keys: &RecordsConfig,
    path: &str,
    location: &str,
) -> Result<FieldSchema, RecordsError> {
    let fields = object(value, path, location)?;
    let ref_to = extension(
        fields,
        &keys.ref_to_key,
        "x-rosetta-ref-to",
        "x-virgil-ref-to",
    )
    .filter(|value| !value.is_null())
    .map(|value| {
        serde_json::from_value(value.clone()).map_err(|error| {
            shape(
                path,
                &format!("{location}.{}", keys.ref_to_key),
                error.to_string(),
            )
        })
    })
    .transpose()?;
    let items = fields
        .get("items")
        .filter(|value| !value.is_null())
        .map(|value| field_schema(value, keys, path, &format!("{location}.items")).map(Box::new))
        .transpose()?;
    let properties = fields
        .get("properties")
        .map(|value| properties(value, keys, path, &format!("{location}.properties")))
        .transpose()?
        .unwrap_or_default();
    let one_of = fields
        .get("oneOf")
        .map(|value| {
            value
                .as_array()
                .ok_or_else(|| shape(path, &format!("{location}.oneOf"), "must be an array"))?
                .iter()
                .enumerate()
                .map(|(index, value)| {
                    field_schema(value, keys, path, &format!("{location}.oneOf[{index}]"))
                })
                .collect::<Result<Vec<_>, _>>()
        })
        .transpose()?
        .unwrap_or_default();
    Ok(FieldSchema {
        ref_to,
        items,
        properties,
        one_of,
    })
}

fn properties(
    value: &Value,
    keys: &RecordsConfig,
    path: &str,
    location: &str,
) -> Result<BTreeMap<String, FieldSchema>, RecordsError> {
    object(value, path, location)?
        .iter()
        .map(|(name, value)| {
            field_schema(value, keys, path, &format!("{location}.{name}"))
                .map(|field| (name.clone(), field))
        })
        .collect()
}

fn schema_bundle(
    value: &Value,
    keys: &RecordsConfig,
    path: &str,
) -> Result<SchemaBundle, RecordsError> {
    let root = object(value, path, "$")?;
    let collections = extension(root, &keys.collections_key, "x-rosetta-collections", "x-virgil-collections")
        .ok_or_else(|| shape(path, "$", format!("missing collection map; set records.collections_key in .rosetta/rosetta.toml (expected {})", keys.collections_key)))?;
    let mut parsed = BTreeMap::new();
    for (name, value) in object(collections, path, &keys.collections_key)? {
        let location = format!("{}.{}", keys.collections_key, name);
        let collection = object(value, path, &location)?;
        let description = collection
            .get("description")
            .map(|value| {
                value.as_str().map(str::to_owned).ok_or_else(|| {
                    shape(path, &format!("{location}.description"), "must be a string")
                })
            })
            .transpose()?
            .unwrap_or_default();
        let scope_value = extension(
            collection,
            &keys.scope_key,
            "x-rosetta-scope",
            "x-virgil-scope",
        )
        .ok_or_else(|| {
            shape(
                path,
                &location,
                format!(
                    "missing scope; set records.scope_key in .rosetta/rosetta.toml (expected {})",
                    keys.scope_key
                ),
            )
        })?;
        let scope = serde_json::from_value(scope_value.clone()).map_err(|error| {
            shape(
                path,
                &format!("{location}.{}", keys.scope_key),
                error.to_string(),
            )
        })?;
        let properties = collection
            .get("properties")
            .map(|value| properties(value, keys, path, &format!("{location}.properties")))
            .transpose()?
            .unwrap_or_default();
        parsed.insert(
            name.clone(),
            CollectionSchema {
                description,
                scope,
                properties,
            },
        );
    }
    Ok(SchemaBundle {
        collections: parsed,
    })
}

/// Reads `records.file` under `root` and places each collection in its area.
pub fn read_records(
    root: &Path,
    records: &Records,
    keys: &RecordsConfig,
) -> Result<RecordsModel, RecordsError> {
    let canonical_root = std::fs::canonicalize(root).map_err(|source| RecordsError::Read {
        path: root.display().to_string(),
        source,
    })?;
    let canonical_path =
        std::fs::canonicalize(root.join(&records.file)).map_err(|source| RecordsError::Read {
            path: records.file.clone(),
            source,
        })?;
    if !canonical_path.starts_with(&canonical_root) {
        return Err(RecordsError::UnsafePath {
            path: records.file.clone(),
        });
    }
    let text = std::fs::read_to_string(canonical_path).map_err(|source| RecordsError::Read {
        path: records.file.clone(),
        source,
    })?;
    let value: Value = serde_json::from_str(&text).map_err(|source| RecordsError::Parse {
        path: records.file.clone(),
        source,
    })?;
    let bundle = schema_bundle(&value, keys, &records.file)?;
    Ok(model(records, &bundle))
}

fn model(records: &Records, bundle: &SchemaBundle) -> RecordsModel {
    let mut findings = Vec::new();
    let mut areas: Vec<BundleNode> = records
        .areas
        .iter()
        .map(|area| node(area_id(&area.id), area.title.clone(), NodeKind::Area, None))
        .collect();

    for (index, area) in records.areas.iter().enumerate() {
        for pattern in &area.collections {
            if !bundle.collections.keys().any(|name| claims(pattern, name)) {
                findings.push(format!(
                    "records.areas[{index}].collections: \"{pattern}\" names no collection in {}",
                    records.file
                ));
            }
        }
    }

    let mut collections = Vec::new();
    let mut scopes = BTreeMap::new();
    let mut unplaced = false;
    for (name, schema) in &bundle.collections {
        let owners: Vec<&str> = records
            .areas
            .iter()
            .filter(|area| area.collections.iter().any(|p| claims(p, name)))
            .map(|area| area.id.as_str())
            .collect();
        let area = match owners.as_slice() {
            [] => {
                findings.push(format!(
                    "records.areas: the collection \"{name}\" is in no area; add it to one"
                ));
                unplaced = true;
                OTHER_AREA
            }
            [only] => only,
            [first, ..] => {
                findings.push(format!(
                    "records.areas: the collection \"{name}\" is in more than one area ({}); keep it in one",
                    owners.join(", ")
                ));
                first
            }
        };
        let mut collection = node(
            collection_id(name),
            name.clone(),
            NodeKind::Collection,
            Some(area_id(area)),
        );
        collection.summary = first_sentence(&schema.description);
        scopes.insert(collection.id.clone(), schema.scope);
        collections.push(collection);
    }
    if unplaced {
        areas.push(node(
            area_id(OTHER_AREA),
            "Other".to_owned(),
            NodeKind::Area,
            None,
        ));
    }

    let mut seen = BTreeSet::new();
    let mut edges = Vec::new();
    for (name, schema) in &bundle.collections {
        let mut refs = Vec::new();
        for (field, inner) in &schema.properties {
            refs_in(field, inner, &mut refs);
        }
        for (field, targets) in refs {
            for target in targets {
                if !bundle.collections.contains_key(target) {
                    findings.push(format!(
                        "{}: {name}.{field} refs \"{target}\", which is no collection",
                        records.file
                    ));
                    continue;
                }
                if seen.insert((name.clone(), target.clone(), field.clone())) {
                    edges.push(Edge {
                        from: collection_id(name),
                        to: collection_id(target),
                        kind: EdgeKind::Refs,
                        label: Some(field.clone()),
                        derived: Some(Derived {
                            source: records.file.clone(),
                        }),
                        evidence: None,
                    });
                }
            }
        }
    }

    RecordsModel {
        areas,
        collections,
        scopes,
        edges,
        findings,
    }
}

#[cfg(test)]
mod tests {
    use super::super::model::RecordArea;
    use super::*;

    #[test]
    fn a_pattern_is_a_name_or_a_prefix() {
        assert!(claims("bot_*", "bot_command"));
        assert!(claims("action*", "action"));
        assert!(!claims("action", "action_set"));
    }

    #[test]
    fn the_summary_is_the_first_sentence() {
        assert_eq!(
            first_sentence("One button. IMMUTABLE: an action is replaced.").as_deref(),
            Some("One button.")
        );
        assert_eq!(
            first_sentence("No full stop").as_deref(),
            Some("No full stop")
        );
        assert_eq!(first_sentence("  "), None);
    }

    #[test]
    fn generic_legacy_and_configured_extension_keys_produce_the_same_records()
    -> Result<(), Box<dyn std::error::Error>> {
        let root = tempfile::tempdir()?;
        let records = Records {
            file: "schemas.json".to_owned(),
            areas: vec![RecordArea {
                id: "storage".to_owned(),
                title: "Storage".to_owned(),
                collections: vec!["a".to_owned(), "b".to_owned()],
            }],
        };
        let generic = r#"{"x-rosetta-collections":{"a":{"description":"An entry. More detail.","x-rosetta-scope":"container","properties":{"target":{"x-rosetta-ref-to":["b"]}}},"b":{"x-rosetta-scope":"private"}}}"#;
        std::fs::write(root.path().join(&records.file), generic)?;
        let expected = read_records(root.path(), &records, &RecordsConfig::default())?;
        assert_eq!(expected.collections.len(), 2);
        assert_eq!(expected.edges.len(), 1);
        assert_eq!(expected.edges[0].from, "records:a");
        assert_eq!(expected.edges[0].to, "records:b");

        std::fs::write(
            root.path().join(&records.file),
            generic.replace("x-rosetta", "x-virgil"),
        )?;
        assert_eq!(
            read_records(root.path(), &records, &RecordsConfig::default())?,
            expected
        );

        std::fs::write(
            root.path().join(&records.file),
            generic.replace("x-rosetta", "x-example"),
        )?;
        let custom = RecordsConfig {
            collections_key: "x-example-collections".to_owned(),
            scope_key: "x-example-scope".to_owned(),
            ref_to_key: "x-example-ref-to".to_owned(),
        };
        assert_eq!(read_records(root.path(), &records, &custom)?, expected);
        Ok(())
    }
}
