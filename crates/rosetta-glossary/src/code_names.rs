//! The identifiers a term's `code` list may name: configured JSON names or
//! workspace package names. A guess is not a code name.

use std::collections::BTreeSet;
use std::path::Path;

use rosetta_core::config::{Config, ConfigError};
use rosetta_core::workspace::{self, WorkspaceError};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CodeNames {
    pub collections: BTreeSet<String>,
    pub packages: BTreeSet<String>,
    sources: Vec<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum CodeNamesError {
    #[error(transparent)]
    Config(#[from] ConfigError),
    #[error(transparent)]
    Workspace(#[from] WorkspaceError),
}

impl CodeNames {
    /// Reads configured JSON names and package names from the repo.
    pub fn from_repo(root: &Path) -> Result<Self, CodeNamesError> {
        let config = Config::load(root, None)?;
        Self::from_repo_with_config(root, &config)
    }

    pub fn from_repo_with_config(root: &Path, config: &Config) -> Result<Self, CodeNamesError> {
        let collections = config.name_values(root)?;
        let mut packages: BTreeSet<String> = workspace::all_members(root)?
            .into_iter()
            .map(|member| member.name)
            .collect();
        packages.extend(
            workspace::excluded_members(root)?
                .into_iter()
                .map(|member| member.name),
        );
        packages.extend(workspace::root_package_name(root)?);
        packages.extend(workspace::root_cargo_name(root)?);
        Ok(Self {
            collections,
            packages,
            sources: config
                .name_sources
                .iter()
                .map(|source| format!("{}#{}", source.glob, source.pointer))
                .collect(),
        })
    }

    pub fn source_kinds(&self) -> String {
        if self.sources.is_empty() {
            "a Cargo package or a package.json name (no JSON name source configured)".to_owned()
        } else {
            format!(
                "a Cargo package, a package.json name or a JSON name from {}",
                self.sources.join(", ")
            )
        }
    }

    pub fn resolves(&self, name: &str) -> bool {
        self.collections.contains(name) || self.packages.contains(name)
    }
}
