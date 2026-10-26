//! The parts of a configured local Whetstone rulebook the glossary and atlas read: the
//! rule and principle ids refs resolve against, their statements, and the one
//! enforcer query the glossary generates.

use std::collections::BTreeSet;
use std::path::Path;

use serde::Deserialize;

use crate::config::{Config, ConfigError};
use crate::json_text;

/// The rule whose enforcer query is generated from the glossary's Avoid words.
pub const GLOSSARY_RULE: &str = "rule.glossary-terms";

#[derive(Debug, Clone, Deserialize)]
pub struct Rulebook {
    pub principles: Vec<RulebookEntry>,
    pub rules: Vec<RulebookRule>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RulebookEntry {
    pub id: String,
    pub statement: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RulebookRule {
    pub id: String,
    pub statement: String,
    #[serde(default)]
    pub enforcer: Option<Enforcer>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Enforcer {
    #[serde(default)]
    pub query: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum RulebookError {
    #[error(transparent)]
    Config(#[from] ConfigError),
    #[error("set refs.rulebook in .rosetta/rosetta.toml to use a Whetstone rulebook")]
    NotConfigured,
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
    #[error("rulebook JSON: {0}")]
    Text(#[from] json_text::ParseError),
    #[error("rulebook JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("{GLOSSARY_RULE}.enforcer.query is missing from the configured rulebook")]
    NoGlossaryQuery,
}

impl Rulebook {
    pub fn load(root: &Path) -> Result<Self, RulebookError> {
        let config = Config::load(root, None)?;
        let path = config.refs.rulebook.ok_or(RulebookError::NotConfigured)?;
        Self::read(root, &path)
    }

    /// The rulebook, or `None` when the repo has none.
    pub fn load_optional(root: &Path) -> Result<Option<Self>, RulebookError> {
        let config = Config::load(root, None)?;
        Self::load_optional_for(root, &config)
    }

    pub fn load_optional_for(root: &Path, config: &Config) -> Result<Option<Self>, RulebookError> {
        config
            .refs
            .rulebook
            .as_deref()
            .map(|path| Self::read(root, path))
            .transpose()
    }

    fn read(root: &Path, path: &str) -> Result<Self, RulebookError> {
        let canonical_root = std::fs::canonicalize(root).map_err(|source| RulebookError::Read {
            path: root.display().to_string(),
            source,
        })?;
        let canonical_path =
            std::fs::canonicalize(root.join(path)).map_err(|source| RulebookError::Read {
                path: path.to_owned(),
                source,
            })?;
        if !canonical_path.starts_with(&canonical_root) {
            return Err(RulebookError::UnsafePath {
                path: path.to_owned(),
            });
        }
        let text =
            std::fs::read_to_string(canonical_path).map_err(|source| RulebookError::Read {
                path: path.to_owned(),
                source,
            })?;
        serde_json::from_str(&text).map_err(|source| RulebookError::Parse {
            path: path.to_owned(),
            source,
        })
    }

    pub fn rule_ids(&self) -> BTreeSet<String> {
        self.rules.iter().map(|rule| rule.id.clone()).collect()
    }

    pub fn principle_ids(&self) -> BTreeSet<String> {
        self.principles
            .iter()
            .map(|principle| principle.id.clone())
            .collect()
    }

    pub fn glossary_query(&self) -> Option<&str> {
        self.rules
            .iter()
            .find(|rule| rule.id == GLOSSARY_RULE)
            .and_then(|rule| rule.enforcer.as_ref())
            .and_then(|enforcer| enforcer.query.as_deref())
    }
}

/// Replaces the glossary rule's enforcer query in the rulebook's text and
/// leaves every other byte as it was.
pub fn replace_glossary_query(text: &str, query: &str) -> Result<String, RulebookError> {
    let root = json_text::parse(text)?;
    let node = root
        .get("rules")
        .map(json_text::Node::items)
        .unwrap_or_default()
        .iter()
        .find(|rule| rule.get("id").and_then(|id| id.as_str()).as_deref() == Some(GLOSSARY_RULE))
        .and_then(|rule| rule.get("enforcer"))
        .and_then(|enforcer| enforcer.get("query"))
        .ok_or(RulebookError::NoGlossaryQuery)?;
    let literal = serde_json::to_string(query)?;
    let mut out = String::with_capacity(text.len() + literal.len());
    out.push_str(&text[..node.span.start]);
    out.push_str(&literal);
    out.push_str(&text[node.span.end..]);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replaces_only_the_glossary_query() -> Result<(), Box<dyn std::error::Error>> {
        let text = "{\n  \"principles\": [],\n  \"rules\": [\n    { \"id\": \"rule.other\", \"statement\": \"s\", \"enforcer\": { \"query\": \"keep\" } },\n    {\n      \"id\": \"rule.glossary-terms\",\n      \"statement\": \"s\",\n      \"enforcer\": {\n        \"kind\": \"ast\",\n        \"query\": \"old\",\n        \"language\": \"typescript\"\n      }\n    }\n  ]\n}\n";
        let out = replace_glossary_query(text, "new \"q\"")?;
        assert_eq!(out, text.replace("\"old\"", "\"new \\\"q\\\"\""));
        let rulebook: Rulebook = serde_json::from_str(&out)?;
        assert_eq!(rulebook.glossary_query(), Some("new \"q\""));
        Ok(())
    }
}
