//! The repo's workspace members, read from the root Cargo.toml and the root
//! package.json the way cargo and bun read them, without running either.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::Deserialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Manager {
    Cargo,
    Bun,
}

impl Manager {
    /// The root manifest a member is declared in.
    pub fn manifest(self) -> &'static str {
        match self {
            Manager::Cargo => "Cargo.toml",
            Manager::Bun => "package.json",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Member {
    /// Repo-relative path with `/` separators.
    pub path: String,
    /// The package name the member's own manifest declares.
    pub name: String,
    pub manager: Manager,
    /// Paths of the workspace members it depends on.
    pub depends_on: Vec<String>,
    /// Every package its own manifest names, sorted by name.
    pub packages: Vec<Package>,
}

impl Member {
    /// The member's own manifest, repo-relative.
    pub fn manifest(&self) -> String {
        format!("{}/{}", self.path, self.manager.manifest())
    }
}

/// A package a member's manifest depends on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Package {
    /// The registry name, after any Cargo `package =` rename.
    pub name: String,
    /// The Cargo features the member turns on: its own entry's and those of
    /// the `[workspace.dependencies]` entry it inherits.
    pub features: Vec<String>,
    /// Only for building and testing: a dev-dependency or devDependency.
    pub dev: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum WorkspaceError {
    #[error("{path}: {source}")]
    Read {
        path: String,
        source: std::io::Error,
    },
    #[error("{path}: {source}")]
    Toml {
        path: String,
        source: Box<toml::de::Error>,
    },
    #[error("{path}: {source}")]
    Json {
        path: String,
        source: serde_json::Error,
    },
    #[error("{path}: {why}")]
    Pattern { path: String, why: String },
    #[error("{path}: has no package name")]
    Unnamed { path: String },
}

#[derive(Debug, Default, Deserialize)]
struct CargoManifest {
    #[serde(default)]
    package: Option<CargoPackage>,
    #[serde(default)]
    workspace: Option<CargoWorkspace>,
    #[serde(default)]
    dependencies: BTreeMap<String, CargoDependency>,
    #[serde(default, rename = "dev-dependencies")]
    dev_dependencies: BTreeMap<String, CargoDependency>,
    #[serde(default)]
    target: BTreeMap<String, CargoTarget>,
}

#[derive(Debug, Deserialize)]
struct CargoPackage {
    name: String,
}

#[derive(Debug, Default, Deserialize)]
struct CargoWorkspace {
    #[serde(default)]
    members: Vec<String>,
    #[serde(default)]
    exclude: Vec<String>,
    #[serde(default)]
    dependencies: BTreeMap<String, CargoDependency>,
}

#[derive(Debug, Default, Deserialize)]
struct CargoTarget {
    #[serde(default)]
    dependencies: BTreeMap<String, CargoDependency>,
    #[serde(default, rename = "dev-dependencies")]
    dev_dependencies: BTreeMap<String, CargoDependency>,
}

/// A dependency entry: a version string, or a table that may name a path,
/// rename the package, turn on features or defer to
/// `[workspace.dependencies]`.
#[derive(Debug)]
enum CargoDependency {
    Version,
    Detailed {
        path: Option<String>,
        workspace: Option<bool>,
        package: Option<String>,
        features: Vec<String>,
    },
}

#[derive(Debug, Deserialize)]
struct CargoDependencyTable {
    #[serde(default)]
    path: Option<String>,
    #[serde(default)]
    workspace: Option<bool>,
    #[serde(default)]
    package: Option<String>,
    #[serde(default)]
    features: Vec<String>,
}

impl<'de> Deserialize<'de> for CargoDependency {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Visitor;
        impl<'de> serde::de::Visitor<'de> for Visitor {
            type Value = CargoDependency;

            fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("a version string or a dependency table")
            }

            fn visit_str<E: serde::de::Error>(self, _: &str) -> Result<Self::Value, E> {
                Ok(CargoDependency::Version)
            }

            fn visit_map<A: serde::de::MapAccess<'de>>(
                self,
                map: A,
            ) -> Result<Self::Value, A::Error> {
                let table = CargoDependencyTable::deserialize(
                    serde::de::value::MapAccessDeserializer::new(map),
                )?;
                Ok(CargoDependency::Detailed {
                    path: table.path,
                    workspace: table.workspace,
                    package: table.package,
                    features: table.features,
                })
            }
        }
        deserializer.deserialize_any(Visitor)
    }
}

#[derive(Debug, Default, Deserialize)]
struct PackageJson {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    workspaces: Option<BunWorkspaces>,
    #[serde(default)]
    dependencies: BTreeMap<String, String>,
    #[serde(default, rename = "devDependencies")]
    dev_dependencies: BTreeMap<String, String>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum BunWorkspaces {
    List(Vec<String>),
    Object {
        #[serde(default)]
        packages: Vec<String>,
    },
}

fn read(root: &Path, path: &str) -> Result<String, WorkspaceError> {
    std::fs::read_to_string(root.join(path)).map_err(|source| WorkspaceError::Read {
        path: path.to_owned(),
        source,
    })
}

fn read_toml(root: &Path, path: &str) -> Result<CargoManifest, WorkspaceError> {
    toml::from_str(&read(root, path)?).map_err(|source| WorkspaceError::Toml {
        path: path.to_owned(),
        source: Box::new(source),
    })
}

fn read_package_json(root: &Path, path: &str) -> Result<PackageJson, WorkspaceError> {
    serde_json::from_str(&read(root, path)?).map_err(|source| WorkspaceError::Json {
        path: path.to_owned(),
        source,
    })
}

/// Joins and normalises a repo-relative path (`a/b/../c` is `a/c`).
pub fn normalize(path: &str) -> String {
    let mut parts: Vec<&str> = Vec::new();
    for part in path.split(['/', '\\']) {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            part => parts.push(part),
        }
    }
    parts.join("/")
}

/// Expands workspace patterns into the directories that hold `manifest`, in
/// sorted order. A pattern starting with `!` removes what it matches.
fn expand(
    root: &Path,
    source: &str,
    patterns: &[String],
    exclude: &[String],
    manifest: &str,
) -> Result<Vec<String>, WorkspaceError> {
    let mut include: Vec<String> = Vec::new();
    let mut remove: Vec<String> = exclude.iter().map(|p| normalize(p)).collect();
    for pattern in patterns {
        match pattern.strip_prefix('!') {
            Some(negated) => remove.push(normalize(negated)),
            None => include.push(normalize(pattern)),
        }
    }
    let removals: Vec<glob::Pattern> = remove
        .iter()
        .map(|pattern| {
            glob::Pattern::new(pattern).map_err(|error| WorkspaceError::Pattern {
                path: source.to_owned(),
                why: format!("{pattern}: {error}"),
            })
        })
        .collect::<Result<_, _>>()?;
    let mut found: Vec<String> = Vec::new();
    for pattern in include {
        let full = root.join(&pattern);
        let paths =
            glob::glob(&full.to_string_lossy()).map_err(|error| WorkspaceError::Pattern {
                path: source.to_owned(),
                why: format!("{pattern}: {error}"),
            })?;
        for entry in paths.flatten() {
            let Some(relative) = relative_path(root, &entry) else {
                continue;
            };
            if !entry.join(manifest).is_file()
                || removals.iter().any(|removal| removal.matches(&relative))
            {
                continue;
            }
            found.push(relative);
        }
    }
    found.sort();
    found.dedup();
    Ok(found)
}

pub fn relative_path(root: &Path, path: &Path) -> Option<String> {
    let relative: PathBuf = path.strip_prefix(root).ok()?.to_path_buf();
    Some(normalize(&relative.to_string_lossy()))
}

fn path_dependency(base: &str, dependency: &CargoDependency) -> Option<String> {
    match dependency {
        CargoDependency::Detailed {
            path: Some(path), ..
        } => Some(normalize(&format!("{base}/{path}"))),
        _ => None,
    }
}

/// Every package a Cargo manifest names, each with the features it turns
/// on, inheriting from `workspace` where an entry says `workspace = true`.
fn cargo_packages(
    manifest: &CargoManifest,
    workspace: &BTreeMap<String, CargoDependency>,
) -> Vec<Package> {
    let targets = manifest.target.values();
    let runtime = manifest
        .dependencies
        .iter()
        .chain(targets.clone().flat_map(|t| t.dependencies.iter()))
        .map(|entry| (entry, false));
    let dev = manifest
        .dev_dependencies
        .iter()
        .chain(targets.flat_map(|t| t.dev_dependencies.iter()))
        .map(|entry| (entry, true));
    let mut packages: BTreeMap<(String, bool), Package> = BTreeMap::new();
    for ((key, dependency), dev) in runtime.chain(dev) {
        let mut name = key.clone();
        let mut features = Vec::new();
        let mut absorb = |dependency: &CargoDependency| {
            if let CargoDependency::Detailed {
                package,
                features: own,
                ..
            } = dependency
            {
                if let Some(package) = package {
                    name.clone_from(package);
                }
                features.extend(own.iter().cloned());
            }
        };
        if let CargoDependency::Detailed {
            workspace: Some(true),
            ..
        } = dependency
            && let Some(inherited) = workspace.get(key)
        {
            absorb(inherited);
        }
        absorb(dependency);
        let entry = packages
            .entry((name.clone(), dev))
            .or_insert_with(|| Package {
                name,
                features: Vec::new(),
                dev,
            });
        entry.features.extend(features);
        entry.features.sort();
        entry.features.dedup();
    }
    packages.into_values().collect()
}

/// Every package a package.json names.
fn npm_packages(manifest: &PackageJson) -> Vec<Package> {
    let runtime = manifest.dependencies.keys().map(|name| (name, false));
    let dev = manifest.dev_dependencies.keys().map(|name| (name, true));
    let mut packages: Vec<Package> = runtime
        .chain(dev)
        .map(|(name, dev)| Package {
            name: name.clone(),
            features: Vec::new(),
            dev,
        })
        .collect();
    packages.sort_by(|a, b| (&a.name, a.dev).cmp(&(&b.name, b.dev)));
    packages
}

/// The members of the root Cargo workspace; empty when there is no root
/// Cargo.toml or it declares no workspace.
pub fn cargo_members(root: &Path) -> Result<Vec<Member>, WorkspaceError> {
    if !root.join("Cargo.toml").is_file() {
        return Ok(Vec::new());
    }
    let manifest = read_toml(root, "Cargo.toml")?;
    let Some(workspace) = manifest.workspace else {
        return Ok(Vec::new());
    };
    let paths = expand(
        root,
        "Cargo.toml",
        &workspace.members,
        &workspace.exclude,
        "Cargo.toml",
    )?;
    let mut members = Vec::new();
    for path in &paths {
        let manifest_path = format!("{path}/Cargo.toml");
        let crate_manifest = read_toml(root, &manifest_path)?;
        let name = crate_manifest
            .package
            .as_ref()
            .map(|package| package.name.clone())
            .ok_or_else(|| WorkspaceError::Unnamed {
                path: manifest_path.clone(),
            })?;
        let dependencies = crate_manifest.dependencies.iter().chain(
            crate_manifest
                .target
                .values()
                .flat_map(|target| target.dependencies.iter()),
        );
        let mut depends_on: Vec<String> = Vec::new();
        for (key, dependency) in dependencies {
            let resolved = match dependency {
                CargoDependency::Detailed {
                    workspace: Some(true),
                    ..
                } => workspace
                    .dependencies
                    .get(key)
                    .and_then(|root_dep| path_dependency("", root_dep)),
                other => path_dependency(path, other),
            };
            if let Some(target) = resolved
                && paths.contains(&target)
                && &target != path
            {
                depends_on.push(target);
            }
        }
        depends_on.sort();
        depends_on.dedup();
        members.push(Member {
            path: path.clone(),
            name,
            manager: Manager::Cargo,
            depends_on,
            packages: cargo_packages(&crate_manifest, &workspace.dependencies),
        });
    }
    Ok(members)
}

/// The root package.json's own name, when it has one.
pub fn root_package_name(root: &Path) -> Result<Option<String>, WorkspaceError> {
    if !root.join("package.json").is_file() {
        return Ok(None);
    }
    Ok(read_package_json(root, "package.json")?.name)
}

/// The root Cargo package name, when the root manifest declares a package.
pub fn root_cargo_name(root: &Path) -> Result<Option<String>, WorkspaceError> {
    if !root.join("Cargo.toml").is_file() {
        return Ok(None);
    }
    Ok(read_toml(root, "Cargo.toml")?
        .package
        .map(|package| package.name))
}

/// The members of the root bun workspace; empty without a root package.json.
pub fn bun_members(root: &Path) -> Result<Vec<Member>, WorkspaceError> {
    if !root.join("package.json").is_file() {
        return Ok(Vec::new());
    }
    let patterns = match read_package_json(root, "package.json")?.workspaces {
        None => return Ok(Vec::new()),
        Some(BunWorkspaces::List(list)) => list,
        Some(BunWorkspaces::Object { packages }) => packages,
    };
    let paths = expand(root, "package.json", &patterns, &[], "package.json")?;
    let mut manifests = Vec::new();
    for path in &paths {
        let manifest_path = format!("{path}/package.json");
        let manifest = read_package_json(root, &manifest_path)?;
        let name = manifest
            .name
            .clone()
            .ok_or_else(|| WorkspaceError::Unnamed {
                path: manifest_path.clone(),
            })?;
        manifests.push((path.clone(), name, manifest));
    }
    let by_name: BTreeMap<&str, &str> = manifests
        .iter()
        .map(|(path, name, _)| (name.as_str(), path.as_str()))
        .collect();
    let mut members = Vec::new();
    for (path, name, manifest) in &manifests {
        let mut depends_on: Vec<String> = manifest
            .dependencies
            .iter()
            .filter(|(_, version)| version.starts_with("workspace:"))
            .filter_map(|(dependency, _)| by_name.get(dependency.as_str()))
            .filter(|target| **target != path.as_str())
            .map(|target| (*target).to_owned())
            .collect();
        depends_on.sort();
        depends_on.dedup();
        members.push(Member {
            path: path.clone(),
            name: name.clone(),
            manager: Manager::Bun,
            depends_on,
            packages: npm_packages(manifest),
        });
    }
    Ok(members)
}

/// Packages the root manifests name but keep out of their workspaces (an
/// `exclude` entry, a `!` pattern): each is still a package of the repo, with
/// its own lockfile. A Cargo one's `depends_on` lists its path dependencies,
/// unfiltered, since they may point into the root workspace.
pub fn excluded_members(root: &Path) -> Result<Vec<Member>, WorkspaceError> {
    let mut members = Vec::new();
    if root.join("Cargo.toml").is_file()
        && let Some(workspace) = read_toml(root, "Cargo.toml")?.workspace
    {
        let paths = expand(root, "Cargo.toml", &workspace.exclude, &[], "Cargo.toml")?;
        for path in paths {
            let manifest = read_toml(root, &format!("{path}/Cargo.toml"))?;
            let Some(package) = manifest.package.as_ref() else {
                continue;
            };
            let mut depends_on: Vec<String> = manifest
                .dependencies
                .values()
                .chain(
                    manifest
                        .target
                        .values()
                        .flat_map(|target| target.dependencies.values()),
                )
                .filter_map(|dependency| path_dependency(&path, dependency))
                .filter(|target| target != &path)
                .collect();
            depends_on.sort();
            depends_on.dedup();
            // An excluded package is its own workspace root, so it inherits
            // from its own `[workspace.dependencies]`.
            let own = manifest
                .workspace
                .as_ref()
                .map(|workspace| &workspace.dependencies);
            let packages = cargo_packages(&manifest, own.unwrap_or(&BTreeMap::new()));
            members.push(Member {
                path,
                name: package.name.clone(),
                manager: Manager::Cargo,
                depends_on,
                packages,
            });
        }
    }
    if root.join("package.json").is_file() {
        let patterns = match read_package_json(root, "package.json")?.workspaces {
            Some(BunWorkspaces::List(list)) => list,
            Some(BunWorkspaces::Object { packages }) => packages,
            None => Vec::new(),
        };
        let negated: Vec<String> = patterns
            .iter()
            .filter_map(|pattern| pattern.strip_prefix('!').map(str::to_owned))
            .collect();
        for path in expand(root, "package.json", &negated, &[], "package.json")? {
            let manifest = read_package_json(root, &format!("{path}/package.json"))?;
            if let Some(name) = manifest.name.clone() {
                members.push(Member {
                    path,
                    name,
                    manager: Manager::Bun,
                    depends_on: Vec::new(),
                    packages: npm_packages(&manifest),
                });
            }
        }
    }
    members.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(members)
}

/// Every member of both workspaces, Cargo first, each sorted by path.
pub fn all_members(root: &Path) -> Result<Vec<Member>, WorkspaceError> {
    let mut members = cargo_members(root)?;
    members.extend(bun_members(root)?);
    Ok(members)
}

#[cfg(test)]
mod tests {
    use super::{Manager, cargo_members};

    #[test]
    fn windows_separators_become_repo_relative_paths() -> Result<(), Box<dyn std::error::Error>> {
        let root = tempfile::tempdir()?;
        std::fs::create_dir_all(root.path().join("crates/api"))?;
        std::fs::create_dir_all(root.path().join("crates/worker"))?;
        std::fs::write(
            root.path().join("Cargo.toml"),
            "[workspace]\nmembers = ['crates\\*']\n",
        )?;
        std::fs::write(
            root.path().join("crates/api/Cargo.toml"),
            "[package]\nname = 'api'\n",
        )?;
        std::fs::write(
            root.path().join("crates/worker/Cargo.toml"),
            "[package]\nname = 'worker'\n[dependencies]\napi = { path = '..\\api' }\n",
        )?;

        let members = cargo_members(root.path())?;
        assert_eq!(members.len(), 2);
        let worker = members
            .iter()
            .find(|member| member.name == "worker")
            .ok_or("worker member")?;
        assert_eq!(worker.path, "crates/worker");
        assert_eq!(worker.depends_on, ["crates/api"]);
        Ok(())
    }

    #[test]
    fn inherited_renamed_dependency_keeps_registry_name_and_features()
    -> Result<(), Box<dyn std::error::Error>> {
        let root = tempfile::tempdir()?;
        std::fs::create_dir_all(root.path().join("crates/api"))?;
        std::fs::create_dir_all(root.path().join("crates/util"))?;
        std::fs::write(
            root.path().join("Cargo.toml"),
            "[workspace]\nmembers = ['crates/api', 'crates/util']\n[workspace.dependencies]\nalias = { package = 'actual', version = '1', features = ['serde'] }\n",
        )?;
        std::fs::write(
            root.path().join("crates/api/Cargo.toml"),
            "[package]\nname = 'api'\n[dependencies]\nalias = { workspace = true, features = ['extra'] }\nhelper = { path = '../util' }\n",
        )?;
        std::fs::write(
            root.path().join("crates/util/Cargo.toml"),
            "[package]\nname = 'util'\n",
        )?;

        let members = cargo_members(root.path())?;
        let api = members
            .iter()
            .find(|member| member.name == "api")
            .ok_or("api member")?;
        assert_eq!(api.manager, Manager::Cargo);
        assert_eq!(api.depends_on, ["crates/util"]);
        let renamed = api
            .packages
            .iter()
            .find(|package| package.name == "actual")
            .ok_or("renamed package")?;
        assert_eq!(renamed.features, ["extra", "serde"]);
        assert!(!renamed.dev);
        Ok(())
    }
}
