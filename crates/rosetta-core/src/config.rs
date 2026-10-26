//! Optional repository configuration. The local core has useful defaults.

use std::collections::BTreeSet;
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::repo::SOURCE_DIR;

pub const CONFIG_FILE: &str = "rosetta.toml";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub source: SourceConfig,
    pub refs: RefsConfig,
    pub name_sources: Vec<NameSource>,
    pub records: RecordsConfig,
    pub derivation: DerivationConfig,
    pub path_scopes: Vec<PathScope>,
    pub tracker: TrackerConfig,
    pub lint: LintConfig,
    pub names: NamesConfig,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SourceConfig {
    pub directory: String,
}

impl Default for SourceConfig {
    fn default() -> Self {
        Self {
            directory: SOURCE_DIR.to_owned(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct RefsConfig {
    /// Optional local Whetstone rulebook, including statements and enforcers.
    pub rulebook: Option<String>,
    /// Additional JSON files containing rule, principle or document ids.
    pub providers: Vec<RefProvider>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct RecordsConfig {
    pub collections_key: String,
    pub scope_key: String,
    pub ref_to_key: String,
}

impl Default for RecordsConfig {
    fn default() -> Self {
        Self {
            collections_key: "x-rosetta-collections".to_owned(),
            scope_key: "x-rosetta-scope".to_owned(),
            ref_to_key: "x-rosetta-ref-to".to_owned(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RefProvider {
    pub kind: RefKind,
    pub path: String,
    pub pointer: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RefKind {
    Rule,
    Principle,
    Document,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NameSource {
    pub glob: String,
    pub pointer: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct DerivationConfig {
    pub iac: bool,
    pub drivers: bool,
}

impl Default for DerivationConfig {
    fn default() -> Self {
        Self {
            iac: true,
            drivers: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PathScope {
    pub context: String,
    pub paths: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct TrackerConfig {
    /// Executable and arguments. Empty means no tracker command is run.
    pub command: Vec<String>,
    /// JSON pointer into the command's response.
    pub closed_pointer: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct LintConfig {
    pub strict: bool,
    /// Empty means the formats supported by the command being run.
    pub formats: Vec<String>,
    /// Empty means all supported tracked copy files.
    pub include: Vec<String>,
    pub exclude: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct NamesConfig {
    /// Rust files with clap Parser/Subcommand/ValueEnum declarations. None
    /// uses common CLI paths; an empty list disables discovery.
    pub rust_cli: Option<Vec<String>>,
    /// TypeScript files with commander or yargs command declarations.
    pub typescript_cli: Option<Vec<String>>,
    /// TypeScript sources where `id` or `*Collection("id")` declares a name.
    pub typescript_ids: Vec<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("{path}: {source}")]
    Read {
        path: String,
        source: std::io::Error,
    },
    #[error("{path}:{line}: {source}")]
    Parse {
        path: String,
        line: usize,
        source: Box<toml::de::Error>,
    },
    #[error("{path}: source.directory must be a non-empty path inside the repository")]
    SourceDirectory { path: String },
    #[error("{path}: path must stay inside the repository")]
    UnsafePath { path: String },
    #[error("{path}: {source}")]
    Json {
        path: String,
        source: Box<serde_json::Error>,
    },
    #[error("{path} at {pointer}: {why}")]
    Values {
        path: String,
        pointer: String,
        why: String,
    },
    #[error("{pattern}: invalid glob: {why}")]
    Glob { pattern: String, why: String },
    #[error("path_scopes context must be non-empty with at least one path glob")]
    EmptyScope,
    #[error("{path}: records.{key} must be a non-empty JSON property name")]
    EmptyRecordsKey { path: String, key: &'static str },
}

fn valid_directory(directory: &str) -> bool {
    !directory.is_empty()
        && Path::new(directory)
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
}

/// A document reference either names a declared local ID or an existing file
/// inside the repository. Relative paths and symlinks cannot escape it.
pub fn document_ref_exists(root: &Path, declared: &BTreeSet<String>, path: &str) -> bool {
    if path == "." {
        return root.is_dir();
    }
    if !valid_directory(path) {
        return false;
    }
    if declared.contains(path) {
        return true;
    }
    let (Ok(root), Ok(target)) = (
        std::fs::canonicalize(root),
        std::fs::canonicalize(root.join(path)),
    ) else {
        return false;
    };
    target.starts_with(root)
}

impl Config {
    /// Paths declared as a source for this reference kind, for diagnostics.
    pub fn ref_source(&self, kind: RefKind) -> Option<String> {
        let mut paths = Vec::new();
        if kind != RefKind::Document {
            paths.extend(self.refs.rulebook.iter().cloned());
        }
        paths.extend(
            self.refs
                .providers
                .iter()
                .filter(|provider| provider.kind == kind)
                .map(|provider| provider.path.clone()),
        );
        paths.sort();
        paths.dedup();
        (!paths.is_empty()).then(|| paths.join(", "))
    }

    /// Load configuration from `source_dir/rosetta.toml` if present.
    /// An explicit source directory takes precedence over the file's setting.
    pub fn load(root: &Path, source_dir: Option<&str>) -> Result<Self, ConfigError> {
        let lookup = source_dir.unwrap_or(SOURCE_DIR);
        if !valid_directory(lookup) {
            return Err(ConfigError::SourceDirectory {
                path: lookup.to_owned(),
            });
        }
        let relative = Path::new(lookup).join(CONFIG_FILE);
        let path = root.join(&relative);
        let text = match std::fs::read_to_string(&path) {
            Ok(text) => text,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
            Err(source) => {
                return Err(ConfigError::Read {
                    path: relative.display().to_string(),
                    source,
                });
            }
        };
        let mut config: Config = if text.is_empty() {
            Self::default()
        } else {
            toml::from_str(&text).map_err(|source: toml::de::Error| {
                let line = source.span().map_or(1, |span| {
                    text[..span.start].bytes().filter(|b| *b == b'\n').count() + 1
                });
                ConfigError::Parse {
                    path: relative.display().to_string(),
                    line,
                    source: Box::new(source),
                }
            })?
        };
        if let Some(source_dir) = source_dir {
            config.source.directory = source_dir.to_owned();
        }
        if !valid_directory(&config.source.directory) {
            return Err(ConfigError::SourceDirectory {
                path: relative.display().to_string(),
            });
        }
        if let Some(rulebook) = &config.refs.rulebook
            && !valid_directory(rulebook)
        {
            return Err(ConfigError::UnsafePath {
                path: rulebook.clone(),
            });
        }
        for provider in &config.refs.providers {
            if !valid_directory(&provider.path) {
                return Err(ConfigError::UnsafePath {
                    path: provider.path.clone(),
                });
            }
        }
        for (key, value) in [
            ("collections_key", &config.records.collections_key),
            ("scope_key", &config.records.scope_key),
            ("ref_to_key", &config.records.ref_to_key),
        ] {
            if value.trim().is_empty() {
                return Err(ConfigError::EmptyRecordsKey {
                    path: relative.display().to_string(),
                    key,
                });
            }
        }
        for name_source in &config.name_sources {
            if !valid_directory(&name_source.glob) {
                return Err(ConfigError::UnsafePath {
                    path: name_source.glob.clone(),
                });
            }
        }
        for scope in &config.path_scopes {
            if scope.context.trim().is_empty() || scope.paths.is_empty() {
                return Err(ConfigError::EmptyScope);
            }
            for pattern in &scope.paths {
                if !valid_directory(pattern) {
                    return Err(ConfigError::UnsafePath {
                        path: pattern.clone(),
                    });
                }
                glob::Pattern::new(pattern).map_err(|error| ConfigError::Glob {
                    pattern: pattern.clone(),
                    why: error.to_string(),
                })?;
            }
        }
        for pattern in config.lint.include.iter().chain(&config.lint.exclude) {
            if !valid_directory(pattern) {
                return Err(ConfigError::UnsafePath {
                    path: pattern.clone(),
                });
            }
            glob::Pattern::new(pattern).map_err(|error| ConfigError::Glob {
                pattern: pattern.clone(),
                why: error.to_string(),
            })?;
        }
        for pattern in config
            .names
            .rust_cli
            .iter()
            .flatten()
            .chain(config.names.typescript_cli.iter().flatten())
            .chain(&config.names.typescript_ids)
        {
            if !valid_directory(pattern) {
                return Err(ConfigError::UnsafePath {
                    path: pattern.clone(),
                });
            }
            glob::Pattern::new(pattern).map_err(|error| ConfigError::Glob {
                pattern: pattern.clone(),
                why: error.to_string(),
            })?;
        }
        Ok(config)
    }

    /// Contexts whose repository-relative path glob covers this path.
    pub fn contexts_for_path(&self, path: &str) -> Result<BTreeSet<String>, ConfigError> {
        if !valid_directory(path) {
            return Err(ConfigError::UnsafePath {
                path: path.to_owned(),
            });
        }
        let mut contexts = BTreeSet::new();
        for scope in &self.path_scopes {
            for pattern in &scope.paths {
                let glob = glob::Pattern::new(pattern).map_err(|error| ConfigError::Glob {
                    pattern: pattern.clone(),
                    why: error.to_string(),
                })?;
                if glob.matches(path) {
                    contexts.insert(scope.context.clone());
                    break;
                }
            }
        }
        Ok(contexts)
    }

    /// Source files are always resolved beneath the selected repository.
    pub fn source_file(&self, root: &Path, file: &str) -> PathBuf {
        root.join(&self.source.directory).join(file)
    }

    /// Names a configured JSON reference provider exposes. An array may
    /// contain strings or objects with `id` or `name`; an object contributes
    /// its keys. Missing pointers and unsupported shapes are errors.
    pub fn ref_values(&self, root: &Path, kind: RefKind) -> Result<BTreeSet<String>, ConfigError> {
        let mut names = BTreeSet::new();
        for provider in self
            .refs
            .providers
            .iter()
            .filter(|provider| provider.kind == kind)
        {
            names.extend(json_names(root, &provider.path, &provider.pointer)?);
        }
        Ok(names)
    }

    /// Names from explicitly selected JSON files. A missing glob match is
    /// allowed; a matched file with invalid JSON or pointer is an error.
    pub fn name_values(&self, root: &Path) -> Result<BTreeSet<String>, ConfigError> {
        let mut names = BTreeSet::new();
        for source in &self.name_sources {
            let pattern = root.join(&source.glob).to_string_lossy().into_owned();
            let matches = glob::glob(&pattern).map_err(|error| ConfigError::Glob {
                pattern: source.glob.clone(),
                why: error.to_string(),
            })?;
            for path in matches {
                let path = path.map_err(|error| ConfigError::Glob {
                    pattern: source.glob.clone(),
                    why: error.to_string(),
                })?;
                if path.is_file() {
                    let relative =
                        path.strip_prefix(root)
                            .map_err(|_| ConfigError::UnsafePath {
                                path: path.display().to_string(),
                            })?;
                    names.extend(json_names(
                        root,
                        &relative.to_string_lossy(),
                        &source.pointer,
                    )?);
                }
            }
        }
        Ok(names)
    }
}

fn json_names(root: &Path, relative: &str, pointer: &str) -> Result<BTreeSet<String>, ConfigError> {
    let path = root.join(relative);
    let canonical_root = std::fs::canonicalize(root).map_err(|source| ConfigError::Read {
        path: root.display().to_string(),
        source,
    })?;
    let canonical_path = std::fs::canonicalize(&path).map_err(|source| ConfigError::Read {
        path: relative.to_owned(),
        source,
    })?;
    if !canonical_path.starts_with(&canonical_root) {
        return Err(ConfigError::UnsafePath {
            path: relative.to_owned(),
        });
    }
    let text = std::fs::read_to_string(&path).map_err(|source| ConfigError::Read {
        path: relative.to_owned(),
        source,
    })?;
    let value: serde_json::Value =
        serde_json::from_str(&text).map_err(|source| ConfigError::Json {
            path: relative.to_owned(),
            source: Box::new(source),
        })?;
    let selected = value.pointer(pointer).ok_or_else(|| ConfigError::Values {
        path: relative.to_owned(),
        pointer: pointer.to_owned(),
        why: "pointer does not resolve".to_owned(),
    })?;
    let invalid = || ConfigError::Values {
        path: relative.to_owned(),
        pointer: pointer.to_owned(),
        why: "expected an object of names or an array of strings or named objects".to_owned(),
    };
    match selected {
        serde_json::Value::Object(values) => Ok(values.keys().cloned().collect()),
        serde_json::Value::Array(values) => values
            .iter()
            .map(|value| {
                value
                    .as_str()
                    .or_else(|| value.get("id").and_then(serde_json::Value::as_str))
                    .or_else(|| value.get("name").and_then(serde_json::Value::as_str))
                    .map(str::to_owned)
                    .ok_or_else(invalid)
            })
            .collect(),
        _ => Err(invalid()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_config_uses_local_defaults() -> Result<(), Box<dyn std::error::Error>> {
        let root = tempfile::tempdir()?;
        let config = Config::load(root.path(), None)?;
        assert_eq!(config.source.directory, ".rosetta");
        assert_eq!(config.refs.rulebook, None);
        assert!(config.name_sources.is_empty());
        assert_eq!(config.records.collections_key, "x-rosetta-collections");
        assert_eq!(config.records.scope_key, "x-rosetta-scope");
        assert_eq!(config.records.ref_to_key, "x-rosetta-ref-to");
        assert!(config.derivation.iac);
        assert!(config.tracker.command.is_empty());
        Ok(())
    }

    #[test]
    fn explicit_source_wins_and_unknown_key_names_its_line()
    -> Result<(), Box<dyn std::error::Error>> {
        let root = tempfile::tempdir()?;
        std::fs::create_dir(root.path().join(".rosetta"))?;
        std::fs::write(
            root.path().join(".rosetta/rosetta.toml"),
            "[source]\ndirectory = '.legacy'\n[derivation]\niac = false\n",
        )?;
        let config = Config::load(root.path(), None)?;
        assert_eq!(config.source.directory, ".legacy");
        assert!(!config.derivation.iac);
        assert_eq!(
            Config::load(root.path(), Some(".rosetta"))?
                .source
                .directory,
            ".rosetta"
        );
        std::fs::write(
            root.path().join(".rosetta/rosetta.toml"),
            "[lint]\nstrcit = true\n",
        )?;
        let Err(error) = Config::load(root.path(), None) else {
            return Err("unknown key unexpectedly accepted".into());
        };
        assert!(error.to_string().contains("rosetta.toml:2:"));
        Ok(())
    }

    #[test]
    fn source_directory_cannot_escape_the_checkout() -> Result<(), Box<dyn std::error::Error>> {
        let root = tempfile::tempdir()?;
        assert!(Config::load(root.path(), Some("../other")).is_err());
        assert!(Config::load(root.path(), Some("/tmp/other")).is_err());
        Ok(())
    }

    #[test]
    fn rulebook_path_stays_local_and_records_keys_are_named()
    -> Result<(), Box<dyn std::error::Error>> {
        let root = tempfile::tempdir()?;
        std::fs::create_dir(root.path().join(".rosetta"))?;
        let config_file = root.path().join(".rosetta/rosetta.toml");
        std::fs::write(&config_file, "[refs]\nrulebook = '../outside.json'\n")?;
        assert!(matches!(
            Config::load(root.path(), None),
            Err(ConfigError::UnsafePath { .. })
        ));
        std::fs::write(&config_file, "[records]\nscope_key = ''\n")?;
        assert!(matches!(
            Config::load(root.path(), None),
            Err(ConfigError::EmptyRecordsKey {
                key: "scope_key",
                ..
            })
        ));
        Ok(())
    }

    #[test]
    fn configured_json_providers_and_name_globs_resolve_inside_the_repo()
    -> Result<(), Box<dyn std::error::Error>> {
        let root = tempfile::tempdir()?;
        std::fs::create_dir(root.path().join(".rosetta"))?;
        std::fs::create_dir(root.path().join("catalog"))?;
        std::fs::write(
            root.path().join("catalog/terms.json"),
            r#"{"rules":[{"id":"rule.ready"}],"documents":["docs/guide.md"],"names":{"Widget":{}}}"#,
        )?;
        std::fs::write(
            root.path().join(".rosetta/rosetta.toml"),
            "[[refs.providers]]\nkind = 'rule'\npath = 'catalog/terms.json'\npointer = '/rules'\n[[refs.providers]]\nkind = 'document'\npath = 'catalog/terms.json'\npointer = '/documents'\n[[name_sources]]\nglob = 'catalog/*.json'\npointer = '/names'\n",
        )?;
        let config = Config::load(root.path(), None)?;
        assert_eq!(
            config.ref_values(root.path(), RefKind::Rule)?,
            BTreeSet::from(["rule.ready".to_owned()])
        );
        assert_eq!(
            config.ref_values(root.path(), RefKind::Document)?,
            BTreeSet::from(["docs/guide.md".to_owned()])
        );
        assert_eq!(
            config.name_values(root.path())?,
            BTreeSet::from(["Widget".to_owned()])
        );
        std::fs::write(
            root.path().join(".rosetta/rosetta.toml"),
            "[[name_sources]]\nglob = '../outside/*.json'\npointer = '/names'\n",
        )?;
        assert!(Config::load(root.path(), None).is_err());
        Ok(())
    }
}
