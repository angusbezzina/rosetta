//! A repository's glossary and atlas.
//!
//! The glossary (`.rosetta/glossary.json`) defines the words a repo uses and
//! generates GLOSSARY.md and the gate that enforces them. The atlas
//! (`.rosetta/atlas.json`) is a typed model of what the repo is made of, kept
//! true by derivation from its workspaces and emitted as one Mermaid source
//! per view. Neither depends on anything Virgil-specific, so any repo can
//! carry them.

pub mod atlas;
pub mod cli;
pub mod curation;
pub mod glossary;
pub mod glossary_context;
pub mod open;
pub mod site;
pub use rosetta_core::{config, diff, json_text, repo, rulebook, text, workspace};
