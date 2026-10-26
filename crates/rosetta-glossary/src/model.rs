//! The `rosetta.glossary/v1` shape of `.rosetta/glossary.json`.
//!
//! Field order is the order the file is written in, so a typed round trip
//! reproduces the committed file.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub const GLOSSARY_SCHEMA: &str = "rosetta.glossary/v1";
/// Accepted for repositories that predate the Rosetta carve.
pub const VIRGIL_GLOSSARY_SCHEMA: &str = "virgil.glossary/v1";
pub const DEFINITION_MAX_CHARS: usize = 280;
pub const DEFINITION_MAX_SENTENCES: usize = 2;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Glossary {
    pub schema: String,
    pub context: String,
    pub purpose: String,
    pub sections: Vec<GlossarySection>,
    pub terms: Vec<GlossaryTerm>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct GlossarySection {
    pub id: String,
    pub title: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum TermStatus {
    Settled,
    Proposed,
}

impl TermStatus {
    pub const ALL: [TermStatus; 2] = [TermStatus::Settled, TermStatus::Proposed];

    pub fn as_str(self) -> &'static str {
        match self {
            TermStatus::Settled => "settled",
            TermStatus::Proposed => "proposed",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct AvoidWord {
    pub word: String,
    /// Another product's own qualified name, which copy may still use.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub except: Option<Vec<String>>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct TermRefs {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rules: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub principles: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub docs: Option<Vec<String>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct TermDecision {
    pub on: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub page: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct GlossaryTerm {
    pub id: String,
    pub name: String,
    pub section: String,
    pub definition: String,
    /// Named path contexts from rosetta.toml. Empty means repository-wide.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub contexts: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub avoid: Option<Vec<AvoidWord>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refs: Option<TermRefs>,
    pub status: TermStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decided: Option<TermDecision>,
    /// The epic that introduced a proposed term; its landing settles the term.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub epic: Option<String>,
}

impl GlossaryTerm {
    pub fn avoid_words(&self) -> &[AvoidWord] {
        self.avoid.as_deref().unwrap_or_default()
    }
}

/// One finding, printed as `path: why`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Problem {
    pub path: String,
    pub why: String,
}

impl Problem {
    pub fn new(path: impl Into<String>, why: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            why: why.into(),
        }
    }
}

impl std::fmt::Display for Problem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.path, self.why)
    }
}
