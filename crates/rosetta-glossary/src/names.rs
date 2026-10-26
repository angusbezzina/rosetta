//! Avoided words at public naming boundaries. File selection is Git-based;
//! extraction is syntax-aware where the name is declared in code.

use std::collections::BTreeSet;
use std::path::{Component, Path};

use rayon::prelude::*;
use rosetta_core::config::{Config, ConfigError};
use rosetta_core::json_text::{self, Node, Value};
use serde::Serialize;

use crate::lint::{self, LintError};
use crate::model::{Glossary, TermStatus};

#[path = "names_rust.rs"]
mod rust_cli;
#[path = "names_ts.rs"]
mod typescript;

const RUST_CLI_DEFAULT: &[&str] = &["**/src/cli.rs", "**/src/main.rs"];
const TYPESCRIPT_CLI_DEFAULT: &[&str] = &["**/*cli*.ts", "**/*command*.ts", "**/scripts/*.ts"];

#[derive(Debug, thiserror::Error)]
pub enum NamesError {
    #[error(transparent)]
    Lint(#[from] LintError),
    #[error(transparent)]
    Config(#[from] ConfigError),
    #[error("{0}: {1}")]
    Io(String, #[source] std::io::Error),
    #[error("{0}: path must be a file inside the repository")]
    UnsafePath(String),
    #[error("{0}: invalid UTF-8 source")]
    Utf8(String),
    #[error("{0}: {1}")]
    Json(String, #[source] json_text::ParseError),
    #[error("{0}: {1}")]
    Toml(String, #[source] toml::de::Error),
    #[error("{0}: source parser could not be initialized")]
    Parser(String),
    #[error("{0}: invalid name-source glob: {1}")]
    Glob(String, #[source] glob::PatternError),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct NameFinding {
    pub path: String,
    pub line: usize,
    pub column: usize,
    pub kind: String,
    pub name: String,
    pub term_id: String,
    pub term: String,
    pub avoided: String,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Named {
    pub name: String,
    pub kind: &'static str,
    pub offset: usize,
}

#[derive(Debug)]
struct Rule {
    term_id: String,
    term: String,
    avoided: String,
    words: Vec<String>,
    except: Vec<Vec<String>>,
    contexts: Vec<String>,
}

pub struct NameChecker {
    rules: Vec<Rule>,
    code_names: BTreeSet<String>,
    selectors: Selectors,
}

struct Selectors {
    rust_cli: Vec<glob::Pattern>,
    typescript_cli: Vec<glob::Pattern>,
    typescript_ids: Vec<glob::Pattern>,
    json_names: Vec<(glob::Pattern, String)>,
    atlas: String,
}

impl Selectors {
    fn patterns(source: &[String], defaults: &[&str]) -> Result<Vec<glob::Pattern>, NamesError> {
        let values = if source.is_empty() {
            defaults
                .iter()
                .map(|item| (*item).to_owned())
                .collect::<Vec<_>>()
        } else {
            source.to_vec()
        };
        values
            .into_iter()
            .map(|value| glob::Pattern::new(&value).map_err(|error| NamesError::Glob(value, error)))
            .collect()
    }

    fn new(config: &Config) -> Result<Self, NamesError> {
        let rust_cli = match &config.names.rust_cli {
            Some(patterns) => Self::patterns(patterns, &[])?,
            None => Self::patterns(&[], RUST_CLI_DEFAULT)?,
        };
        let typescript_cli = match &config.names.typescript_cli {
            Some(patterns) => Self::patterns(patterns, &[])?,
            None => Self::patterns(&[], TYPESCRIPT_CLI_DEFAULT)?,
        };
        let typescript_ids = Self::patterns(&config.names.typescript_ids, &[])?;
        let json_names = config
            .name_sources
            .iter()
            .map(|source| {
                Ok((
                    glob::Pattern::new(&source.glob)
                        .map_err(|error| NamesError::Glob(source.glob.clone(), error))?,
                    source.pointer.clone(),
                ))
            })
            .collect::<Result<Vec<_>, NamesError>>()?;
        Ok(Self {
            rust_cli,
            typescript_cli,
            typescript_ids,
            json_names,
            atlas: format!("{}/atlas.json", config.source.directory),
        })
    }

    fn matches(patterns: &[glob::Pattern], path: &str) -> bool {
        patterns.iter().any(|pattern| pattern.matches(path))
    }

    fn supported(&self, path: &str) -> bool {
        path == "Cargo.toml"
            || path.ends_with("/Cargo.toml")
            || path == "package.json"
            || path.ends_with("/package.json")
            || path == self.atlas
            || (path.ends_with(".json")
                && self
                    .json_names
                    .iter()
                    .any(|(pattern, _)| pattern.matches(path)))
            || (path.ends_with(".rs") && Self::matches(&self.rust_cli, path))
            || ((path.ends_with(".ts") || path.ends_with(".tsx"))
                && (Self::matches(&self.typescript_cli, path)
                    || Self::matches(&self.typescript_ids, path)))
    }
}

impl NameChecker {
    pub fn new(glossary: &Glossary, config: &Config) -> Result<Self, NamesError> {
        let mut rules = Vec::new();
        let mut code_names = BTreeSet::new();
        for term in &glossary.terms {
            code_names.extend(term.code.iter().flatten().cloned());
            if term.status != TermStatus::Settled {
                continue;
            }
            for avoid in term.avoid_words() {
                rules.push(Rule {
                    term_id: term.id.clone(),
                    term: term.name.clone(),
                    avoided: avoid.word.clone(),
                    words: split_words(&avoid.word),
                    except: avoid
                        .except
                        .iter()
                        .flatten()
                        .map(|phrase| split_words(phrase))
                        .collect(),
                    contexts: term.contexts.clone(),
                });
            }
        }
        Ok(Self {
            rules,
            code_names,
            selectors: Selectors::new(config)?,
        })
    }

    pub fn selected_files(
        &self,
        root: &Path,
        base: Option<&str>,
    ) -> Result<Vec<String>, NamesError> {
        Ok(lint::tracked_files(root, base)?
            .into_iter()
            .filter(|path| {
                !path.contains("/generated/")
                    && !path.contains("/node_modules/")
                    && !path.contains("/fixtures/")
                    && !path.ends_with(".test.ts")
                    && !path.ends_with(".spec.ts")
                    && self.selectors.supported(path)
            })
            .collect())
    }

    pub fn check_files(
        &self,
        root: &Path,
        config: &Config,
        paths: &[String],
    ) -> Result<Vec<NameFinding>, NamesError> {
        let root = root
            .canonicalize()
            .map_err(|error| NamesError::Io(root.display().to_string(), error))?;
        let batches = paths
            .par_iter()
            .map(|path| {
                let relative = Path::new(path);
                if !relative
                    .components()
                    .all(|part| matches!(part, Component::Normal(_)))
                {
                    return Err(NamesError::UnsafePath(path.clone()));
                }
                let candidate = root.join(relative);
                if !candidate.exists() {
                    return Ok(Vec::new());
                } // A changed file was deleted.
                let absolute = candidate
                    .canonicalize()
                    .map_err(|error| NamesError::Io(path.clone(), error))?;
                if !absolute.starts_with(&root) || !absolute.is_file() {
                    return Err(NamesError::UnsafePath(path.clone()));
                }
                let text = String::from_utf8(
                    std::fs::read(absolute).map_err(|error| NamesError::Io(path.clone(), error))?,
                )
                .map_err(|_| NamesError::Utf8(path.clone()))?;
                self.check_text(path, &text, config)
            })
            .collect::<Result<Vec<_>, _>>()?;
        let mut findings = batches.into_iter().flatten().collect::<Vec<_>>();
        findings.sort_by(|left, right| {
            (
                &left.path,
                left.line,
                left.column,
                &left.term_id,
                &left.name,
            )
                .cmp(&(
                    &right.path,
                    right.line,
                    right.column,
                    &right.term_id,
                    &right.name,
                ))
        });
        Ok(findings)
    }

    pub fn check_text(
        &self,
        path: &str,
        text: &str,
        config: &Config,
    ) -> Result<Vec<NameFinding>, NamesError> {
        let mut names = Vec::new();
        if path.ends_with("Cargo.toml") {
            names.extend(cargo_names(path, text)?);
        }
        if path.ends_with("package.json") {
            names.extend(package_names(path, text)?);
        }
        if path == self.selectors.atlas {
            names.extend(atlas_names(path, text)?);
        }
        for (pattern, pointer) in &self.selectors.json_names {
            if pattern.matches(path) {
                names.extend(json_names(path, text, pointer)?);
            }
        }
        if path.ends_with(".rs") && Selectors::matches(&self.selectors.rust_cli, path) {
            names.extend(rust_cli::commands(path, text)?);
        }
        if path.ends_with(".ts") || path.ends_with(".tsx") {
            let commands = Selectors::matches(&self.selectors.typescript_cli, path);
            let ids = Selectors::matches(&self.selectors.typescript_ids, path);
            if commands || ids {
                names.extend(typescript::names(path, text, commands, ids)?);
            }
        }
        let contexts = config.contexts_for_path(path)?;
        let mut findings = Vec::new();
        for named in names {
            if self.code_names.contains(&named.name) {
                continue;
            }
            let have = split_words(&named.name);
            for rule in &self.rules {
                if !rule.contexts.is_empty()
                    && !rule
                        .contexts
                        .iter()
                        .any(|context| contexts.contains(context))
                {
                    continue;
                }
                if !holds(&have, &rule.words)
                    || rule.except.iter().any(|except| holds(&have, except))
                {
                    continue;
                }
                let (line, column) = location(text, named.offset);
                findings.push(NameFinding {
                    path: path.to_owned(), line, column, kind: named.kind.to_owned(), name: named.name.clone(),
                    term_id: rule.term_id.clone(), term: rule.term.clone(), avoided: rule.avoided.clone(),
                    message: format!("{} {:?} uses avoided word {:?}; use {} or record an exemption in the glossary", named.kind, named.name, rule.avoided, rule.term),
                });
            }
        }
        findings.sort_by(|left, right| {
            (left.line, left.column, &left.term_id, &left.name).cmp(&(
                right.line,
                right.column,
                &right.term_id,
                &right.name,
            ))
        });
        findings.dedup();
        Ok(findings)
    }
}

/// Words in an identifier, splitting snake, kebab and lower-to-upper camel
/// boundaries. A plural names the same thing for an Avoid rule.
pub fn split_words(value: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut current = String::new();
    let mut previous: Option<char> = None;
    for ch in value.chars() {
        if !ch.is_ascii_alphanumeric() {
            if !current.is_empty() {
                words.push(std::mem::take(&mut current));
            }
            previous = None;
            continue;
        }
        if ch.is_ascii_uppercase()
            && previous.is_some_and(|before| before.is_ascii_lowercase() || before.is_ascii_digit())
            && !current.is_empty()
        {
            words.push(std::mem::take(&mut current));
        }
        current.push(ch.to_ascii_lowercase());
        previous = Some(ch);
    }
    if !current.is_empty() {
        words.push(current);
    }
    words
}

fn holds(have: &[String], target: &[String]) -> bool {
    !target.is_empty()
        && have.windows(target.len()).any(|window| {
            window
                .iter()
                .zip(target)
                .all(|(word, desired)| word == desired || word == &format!("{desired}s"))
        })
}

pub(crate) fn location(text: &str, offset: usize) -> (usize, usize) {
    let safe = offset.min(text.len());
    let before = &text[..safe];
    let line = before.bytes().filter(|byte| *byte == b'\n').count() + 1;
    let column = before.rsplit('\n').next().unwrap_or("").chars().count() + 1;
    (line, column)
}

fn cargo_names(path: &str, text: &str) -> Result<Vec<Named>, NamesError> {
    let doc: toml::Value =
        toml::from_str(text).map_err(|error| NamesError::Toml(path.to_owned(), error))?;
    let Some(name) = doc
        .get("package")
        .and_then(|item| item.get("name"))
        .and_then(toml::Value::as_str)
    else {
        return Ok(Vec::new());
    };
    let mut section = "";
    let mut offset = 0;
    for line in text.split_inclusive('\n') {
        let trimmed = line.trim();
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            section = trimmed;
        }
        if section == "[package]" && trimmed.starts_with("name") && trimmed.contains('=') {
            let at = line.find(name).unwrap_or(0);
            return Ok(vec![Named {
                name: name.to_owned(),
                kind: "crate",
                offset: offset + at,
            }]);
        }
        offset += line.len();
    }
    Ok(Vec::new())
}

fn package_names(path: &str, text: &str) -> Result<Vec<Named>, NamesError> {
    let doc = json_text::parse(text).map_err(|error| NamesError::Json(path.to_owned(), error))?;
    Ok(doc
        .get("name")
        .and_then(|node| {
            node.as_str().map(|name| Named {
                name,
                kind: "package",
                offset: node.span.start + 1,
            })
        })
        .into_iter()
        .collect())
}

fn atlas_names(path: &str, text: &str) -> Result<Vec<Named>, NamesError> {
    let doc = json_text::parse(text).map_err(|error| NamesError::Json(path.to_owned(), error))?;
    Ok(doc
        .get("nodes")
        .into_iter()
        .flat_map(Node::items)
        .filter_map(|item| {
            item.get("label").and_then(|node| {
                node.as_str().map(|name| Named {
                    name,
                    kind: "atlas label",
                    offset: node.span.start + 1,
                })
            })
        })
        .collect())
}

fn json_pointer<'a>(root: &'a Node, pointer: &str) -> Option<&'a Node> {
    if pointer.is_empty() {
        return Some(root);
    }
    let mut node = root;
    for part in pointer.strip_prefix('/')?.split('/') {
        let key = part.replace("~1", "/").replace("~0", "~");
        node = match &node.value {
            Value::Object(_) => node.get(&key)?,
            Value::Array(items) => items.get(key.parse::<usize>().ok()?)?,
            _ => return None,
        };
    }
    Some(node)
}

fn json_names(path: &str, text: &str, pointer: &str) -> Result<Vec<Named>, NamesError> {
    let doc = json_text::parse(text).map_err(|error| NamesError::Json(path.to_owned(), error))?;
    let Some(node) = json_pointer(&doc, pointer) else {
        return Ok(Vec::new());
    };
    let mut names = Vec::new();
    match &node.value {
        Value::Object(members) => {
            let mut search = node.span.start;
            for member in members {
                let within = &text[search..node.span.end];
                let offset = within
                    .find(&member.raw_key)
                    .map_or(node.span.start, |at| search + at + 1);
                search = offset.saturating_add(member.raw_key.len());
                names.push(Named {
                    name: member.key.clone(),
                    kind: "configured identifier",
                    offset,
                });
            }
        }
        Value::Array(items) => {
            for item in items {
                if let Some(name) = item.as_str() {
                    names.push(Named {
                        name,
                        kind: "configured identifier",
                        offset: item.span.start + 1,
                    });
                } else if let Some(field) = item.get("id").or_else(|| item.get("name"))
                    && let Some(name) = field.as_str()
                {
                    names.push(Named {
                        name,
                        kind: "configured identifier",
                        offset: field.span.start + 1,
                    });
                }
            }
        }
        _ => {}
    }
    Ok(names)
}

#[cfg(test)]
mod tests {
    use super::{NameChecker, split_words};
    use crate::model::Glossary;
    use rosetta_core::config::{Config, NameSource};

    fn glossary() -> Result<Glossary, serde_json::Error> {
        serde_json::from_value(serde_json::json!({
            "schema": "rosetta.glossary/v1", "context": "fixture", "purpose": "fixture",
            "sections": [{ "id": "terms", "title": "Terms" }],
            "terms": [
                { "id": "term.group", "name": "Group", "section": "terms", "definition": "A place.", "status": "settled",
                  "avoid": [{ "word": "workspace", "except": ["Linear workspace"] }], "code": ["workspace_legacy"] },
                { "id": "term.summary", "name": "Summary", "section": "terms", "definition": "A report.", "status": "proposed",
                  "avoid": [{ "word": "recap" }] }
            ]
        }))
    }

    #[test]
    fn names_split_at_identifier_boundaries_and_respect_code_and_exceptions()
    -> Result<(), Box<dyn std::error::Error>> {
        assert_eq!(split_words("WorkspaceDraft"), ["workspace", "draft"]);
        assert_eq!(split_words("workspace_draft"), ["workspace", "draft"]);
        assert_eq!(split_words("workspacelike"), ["workspacelike"]);
        let config = Config::default();
        let checker = NameChecker::new(&glossary()?, &config)?;
        let cargo =
            "[package]\nname = \"virgil-workspaces\"\n[dependencies]\nname = \"workspace\"\n";
        let findings = checker.check_text("crates/x/Cargo.toml", cargo, &config)?;
        assert_eq!(
            findings
                .iter()
                .map(|finding| (finding.line, finding.name.as_str()))
                .collect::<Vec<_>>(),
            [(2, "virgil-workspaces")]
        );
        assert!(
            checker
                .check_text(
                    "apps/x/package.json",
                    "{\"name\":\"workspace_legacy\"}",
                    &config
                )?
                .is_empty()
        );
        assert!(
            checker
                .check_text(
                    "apps/x/package.json",
                    "{\"name\":\"@scope/Linear-workspaces\"}",
                    &config
                )?
                .is_empty()
        );
        assert_eq!(
            checker
                .check_text(
                    "apps/x/package.json",
                    "{\"name\":\"@scope/workspace-ui\"}",
                    &config
                )?
                .len(),
            1
        );
        assert_eq!(
            checker
                .check_text(
                    ".rosetta/atlas.json",
                    "{\"nodes\":[{\"id\":\"x\",\"label\":\"Workspace store\"}]}",
                    &config
                )?
                .len(),
            1
        );
        assert!(
            checker
                .check_text(
                    ".rosetta/atlas.json",
                    "{\"nodes\":[{\"id\":\"x\",\"label\":\"Recap store\"}]}",
                    &config
                )?
                .is_empty()
        );
        Ok(())
    }

    #[test]
    fn configured_json_names_are_naming_boundaries() -> Result<(), Box<dyn std::error::Error>> {
        let mut config = Config::default();
        config.name_sources.push(NameSource {
            glob: "schema.json".into(),
            pointer: "/collections".into(),
        });
        let checker = NameChecker::new(&glossary()?, &config)?;
        let text =
            "{\n  \"collections\": {\n    \"workspace_draft\": {},\n    \"group\": {}\n  }\n}";
        let found = checker.check_text("schema.json", text, &config)?;
        assert_eq!(
            found
                .iter()
                .map(|item| (item.name.as_str(), item.line))
                .collect::<Vec<_>>(),
            [("workspace_draft", 3)]
        );
        Ok(())
    }
}
