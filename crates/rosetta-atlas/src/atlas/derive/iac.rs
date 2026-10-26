//! The cloud resources a repository declares as code: every Pulumi program
//! (a tracked `Pulumi.yaml`) in TypeScript, for any provider, and every
//! tracked Terraform file. One node per resource type per file.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use regex::Regex;

use super::{DeriveError, derived, last_segment, path_id};
use crate::atlas::model::{Family, Node, NodeKind, Refs, Tier};
use crate::text::kebab_of;

/// A directory that declares resources, and the file that says so.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Program {
    /// Repo-relative; empty for the repo root.
    pub dir: String,
    /// The `Pulumi.yaml`, or a directory's first Terraform file.
    pub source: String,
    pub resources: Vec<Node>,
}

/// Tests and their fixtures build resources that never deploy.
fn is_test_or_fixture(path: &str) -> bool {
    path.ends_with(".test.ts")
        || path
            .split('/')
            .rev()
            .skip(1)
            .any(|segment| segment.contains("fixtures"))
}

fn parent_dir(path: &str) -> &str {
    path.rsplit_once('/').map_or("", |(dir, _)| dir)
}

fn is_below(path: &str, dir: &str) -> bool {
    dir.is_empty()
        || path
            .strip_prefix(dir)
            .is_some_and(|rest| rest.starts_with('/'))
}

/// `DatabaseInstance` as `database-instance`, `IAMMember` as `iam-member`.
pub(super) fn kebab_camel(name: &str) -> String {
    let chars: Vec<char> = name.chars().collect();
    let mut out = String::new();
    for (index, c) in chars.iter().enumerate() {
        let previous = index.checked_sub(1).and_then(|i| chars.get(i));
        let next = chars.get(index + 1);
        let starts_word = c.is_ascii_uppercase()
            && previous.is_some_and(|p| {
                p.is_ascii_lowercase()
                    || p.is_ascii_digit()
                    || (p.is_ascii_uppercase() && next.is_some_and(char::is_ascii_lowercase))
            });
        if starts_word {
            out.push('-');
        }
        out.push(*c);
    }
    kebab_of(&out)
}

/// A cloud provider whose resources the family table knows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Provider {
    Gcp,
    Aws,
    Azure,
    Cloudflare,
    Kubernetes,
    Other,
}

impl Provider {
    fn of_pulumi_package(package: &str) -> Self {
        match package {
            "gcp" | "google-native" => Provider::Gcp,
            "aws" | "aws-native" | "awsx" => Provider::Aws,
            "azure-native" | "azure" => Provider::Azure,
            "cloudflare" => Provider::Cloudflare,
            "kubernetes" => Provider::Kubernetes,
            _ => Provider::Other,
        }
    }

    fn of_terraform_prefix(prefix: &str) -> Self {
        match prefix {
            "google" => Provider::Gcp,
            "aws" => Provider::Aws,
            "azurerm" => Provider::Azure,
            "cloudflare" => Provider::Cloudflare,
            "kubernetes" => Provider::Kubernetes,
            _ => Provider::Other,
        }
    }

    /// Prefixes of a resource's normalised service and type (`sqldatabaseinstance`
    /// for `gcp.sql.DatabaseInstance` and `google_sql_database_instance`)
    /// that hold state, then those that run code.
    fn table(self) -> (&'static [&'static str], &'static [&'static str]) {
        match self {
            Provider::Gcp => (
                &[
                    "sql",
                    "storage",
                    "secretmanager",
                    "kms",
                    "artifactregistry",
                    "redis",
                    "firestore",
                    "bigquery",
                    "alloydb",
                    "spanner",
                    "bigtable",
                    "filestore",
                    "memorystore",
                    "cloudtasks",
                    "pubsub",
                ],
                &["cloudrun", "cloudfunctions"],
            ),
            Provider::Aws => (
                &[
                    "s3",
                    "rds",
                    "dbinstance",
                    "dbcluster",
                    "dynamodb",
                    "elasticache",
                    "memorydb",
                    "sqs",
                    "sns",
                    "secretsmanager",
                    "kms",
                    "ecr",
                    "efs",
                    "docdb",
                    "neptune",
                    "kinesis",
                    "opensearch",
                    "redshift",
                ],
                &[
                    "lambda",
                    "ecs",
                    "eks",
                    "apprunner",
                    "elasticbeanstalk",
                    "batch",
                    "ec2instance",
                    "instance",
                ],
            ),
            Provider::Azure => (
                &[
                    "storage",
                    "sql",
                    "mssql",
                    "dbforpostgresql",
                    "dbformysql",
                    "postgresql",
                    "mysql",
                    "documentdb",
                    "cosmosdb",
                    "cache",
                    "redis",
                    "keyvault",
                    "servicebus",
                    "eventhub",
                    "containerregistry",
                ],
                &[
                    "web",
                    "appcontainerapp",
                    "containerapp",
                    "containerinstance",
                    "containergroup",
                    "containerservice",
                    "kubernetescluster",
                    "linuxwebapp",
                    "windowswebapp",
                    "linuxfunctionapp",
                    "windowsfunctionapp",
                    "functionapp",
                    "appservice",
                ],
            ),
            Provider::Cloudflare => (
                &["r2", "workerskv", "d1", "queue"],
                &["workerscript", "workersscript", "pagesproject"],
            ),
            Provider::Kubernetes => (
                &["persistentvolume"],
                &[
                    "deployment",
                    "statefulset",
                    "daemonset",
                    "replicaset",
                    "cronjob",
                    "job",
                ],
            ),
            Provider::Other => (&[], &[]),
        }
    }
}

fn normalise(text: &str) -> String {
    text.chars()
        .filter(char::is_ascii_alphanumeric)
        .map(|c| c.to_ascii_lowercase())
        .collect()
}

/// Words that make a resource a grant or a binding: IAM wiring, not state
/// or compute, whatever service it is declared under (`SecretIamMember`,
/// `aws_s3_bucket_policy`, a Kubernetes `RoleBinding`).
const WIRING: [&str; 6] = ["iam", "policy", "role", "binding", "grant", "permission"];

/// What a resource is to a reader: `store` when it holds state, `service`
/// when it runs code, else `external` (network, identity and other wiring).
/// `service` is the resource's module path or Terraform type without its
/// provider; `resource` its own name, empty for Terraform.
pub(super) fn resource_family(provider: Provider, service: &str, resource: &str) -> Family {
    let key = normalise(&format!("{service}{resource}"));
    if WIRING.iter().any(|word| key.contains(word)) {
        return Family::External;
    }
    let name = normalise(resource);
    let (stores, services) = provider.table();
    let starts = |text: &str, prefixes: &[&str]| {
        !text.is_empty() && prefixes.iter().any(|p| text.starts_with(p))
    };
    if starts(&key, stores) {
        Family::Store
    } else if starts(&key, services) {
        Family::Service
    } else if starts(&name, stores) {
        Family::Store
    } else if starts(&name, services) {
        Family::Service
    } else {
        Family::External
    }
}

fn regex(pattern: &str, source: &str) -> Result<Regex, DeriveError> {
    Regex::new(pattern).map_err(|error| DeriveError::Io {
        path: source.to_owned(),
        source: std::io::Error::other(error),
    })
}

fn read(root: &Path, path: &str) -> Result<String, DeriveError> {
    std::fs::read_to_string(root.join(path)).map_err(|source| DeriveError::Io {
        path: path.to_owned(),
        source,
    })
}

/// The top-level `main:` of a Pulumi.yaml, the only key read from it.
fn pulumi_main(text: &str) -> Option<String> {
    text.lines().find_map(|line| {
        let value = line.strip_prefix("main:")?.trim();
        let value = value.split(" #").next().unwrap_or(value).trim();
        let value = value.trim_matches(|c| c == '"' || c == '\'');
        (!value.is_empty()).then(|| value.to_owned())
    })
}

/// Every program the tracked files declare, Pulumi programs first, each in
/// path order.
pub fn programs(root: &Path, tracked: &[String]) -> Result<Vec<Program>, DeriveError> {
    let tracked_set: BTreeSet<&str> = tracked.iter().map(String::as_str).collect();
    let mut pulumi: Vec<(String, String)> = Vec::new();
    for file in tracked {
        if !matches!(last_segment(file), "Pulumi.yaml" | "Pulumi.yml") || is_test_or_fixture(file) {
            continue;
        }
        let dir = parent_dir(file);
        // `main` names the program's entry file or directory; the sources
        // are everything below the directory it is in.
        let program_dir = match pulumi_main(&read(root, file)?) {
            Some(main) => {
                let joined = crate::workspace::normalize(&format!("{dir}/{main}"));
                if tracked_set.contains(joined.as_str()) {
                    parent_dir(&joined).to_owned()
                } else {
                    joined
                }
            }
            None => dir.to_owned(),
        };
        pulumi.push((program_dir, file.clone()));
    }
    pulumi.sort();
    pulumi.dedup_by(|a, b| a.0 == b.0);

    let mut out = Vec::new();
    for (dir, source) in &pulumi {
        // A file belongs to the deepest program above it.
        let files: Vec<&String> = tracked
            .iter()
            .filter(|file| file.ends_with(".ts") && !file.ends_with(".d.ts"))
            .filter(|file| !is_test_or_fixture(file))
            .filter(|file| is_below(file, dir))
            .filter(|file| {
                !pulumi.iter().any(|(other, _)| {
                    other.len() > dir.len() && is_below(other, dir) && is_below(file, other)
                })
            })
            .collect();
        let mut resources = Vec::new();
        for file in files {
            resources.extend(pulumi_resources(root, file, dir)?);
        }
        out.push(Program {
            dir: dir.clone(),
            source: source.clone(),
            resources,
        });
    }

    let mut terraform: BTreeMap<&str, Vec<&String>> = BTreeMap::new();
    for file in tracked {
        if file.ends_with(".tf") && !is_test_or_fixture(file) {
            terraform.entry(parent_dir(file)).or_default().push(file);
        }
    }
    let declaration = regex(r#"(?m)^\s*resource\s+"([A-Za-z0-9_-]+)"\s+"[^"]*""#, ".tf")?;
    for (dir, files) in terraform {
        let mut resources = Vec::new();
        for file in &files {
            let text = read(root, file)?;
            let types: BTreeSet<String> = declaration
                .captures_iter(&text)
                .map(|capture| capture[1].to_owned())
                .collect();
            let stem = last_segment(file).trim_end_matches(".tf");
            let file_id = path_id(file.trim_end_matches(".tf"));
            for kind in types {
                let (prefix, service) = kind.split_once('_').unwrap_or((kind.as_str(), ""));
                resources.push(resource_node(
                    format!("{file_id}-{}", kebab_of(&kind)),
                    format!("{kind} · {stem}"),
                    resource_family(Provider::of_terraform_prefix(prefix), service, ""),
                    dir,
                    file,
                ));
            }
        }
        if !resources.is_empty() {
            out.push(Program {
                dir: dir.to_owned(),
                source: files.first().map(|f| (*f).clone()).unwrap_or_default(),
                resources,
            });
        }
    }
    Ok(out)
}

fn resource_node(id: String, label: String, family: Family, dir: &str, file: &str) -> Node {
    Node {
        id,
        label,
        family,
        kind: NodeKind::Resource,
        parent: Some(if dir.is_empty() {
            super::ROOT_ID.to_owned()
        } else {
            path_id(dir)
        }),
        path: Some(file.to_owned()),
        summary: None,
        tier: Some(Tier::Cloud),
        refs: Refs::default(),
        derived: derived(file),
    }
}

/// The resources one TypeScript file of a Pulumi program constructs: each
/// `import * as <alias> from "@pulumi/<package>"` binds a provider, and
/// `new <alias>.<module>.<Resource>(` declares a resource of it.
fn pulumi_resources(root: &Path, file: &str, dir: &str) -> Result<Vec<Node>, DeriveError> {
    let text = read(root, file)?;
    let import = regex(
        r#"import\s+\*\s+as\s+([A-Za-z_$][A-Za-z0-9_$]*)\s+from\s+["']@pulumi/([a-z0-9-]+)["']"#,
        file,
    )?;
    let mut found: BTreeSet<(String, String, Family)> = BTreeSet::new();
    for capture in import.captures_iter(&text) {
        let (alias, package) = (&capture[1], &capture[2]);
        // `@pulumi/pulumi` is the engine itself: its classes are not resources.
        if package == "pulumi" {
            continue;
        }
        let provider = Provider::of_pulumi_package(package);
        let constructor = regex(
            &format!(
                r"new\s+{}((?:\.[A-Za-z_][A-Za-z0-9_]*)*)\.([A-Z][A-Za-z0-9_]*)\s*\(",
                regex::escape(alias)
            ),
            file,
        )?;
        for capture in constructor.captures_iter(&text) {
            let module = capture[1].trim_start_matches('.').to_owned();
            let resource = capture[2].to_owned();
            let family = resource_family(provider, &module, &resource);
            found.insert((module, resource, family));
        }
    }
    let stem = last_segment(file).trim_end_matches(".ts");
    let file_id = path_id(file.trim_end_matches(".ts"));
    Ok(found
        .into_iter()
        .map(|(module, resource, family)| {
            let (id, label) = if module.is_empty() {
                (
                    format!("{file_id}-{}", kebab_camel(&resource)),
                    format!("{resource} · {stem}"),
                )
            } else {
                (
                    format!("{file_id}-{}-{}", kebab_of(&module), kebab_camel(&resource)),
                    format!("{module}.{resource} · {stem}"),
                )
            };
            resource_node(id, label, family, dir, file)
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::{Family, Provider, kebab_camel, pulumi_main, resource_family};

    #[test]
    fn a_grant_on_a_store_is_wiring_not_state() {
        let gcp = |service, resource| resource_family(Provider::Gcp, service, resource);
        assert_eq!(gcp("secretmanager", "Secret"), Family::Store);
        assert_eq!(gcp("secretmanager", "SecretIamMember"), Family::External);
        assert_eq!(gcp("kms", "CryptoKeyIAMMember"), Family::External);
        assert_eq!(gcp("storage", "BucketIAMMember"), Family::External);
        assert_eq!(gcp("cloudtasks", "QueueIamMember"), Family::External);
        assert_eq!(gcp("cloudrunv2", "Job"), Family::Service);
        assert_eq!(gcp("compute", "Network"), Family::External);
    }

    #[test]
    fn pulumi_and_terraform_names_of_one_resource_agree() {
        let aws = |service, resource| resource_family(Provider::Aws, service, resource);
        assert_eq!(aws("s3", "Bucket"), Family::Store);
        assert_eq!(aws("s3_bucket", ""), Family::Store);
        assert_eq!(aws("s3_bucket_policy", ""), Family::External);
        assert_eq!(aws("rds", "Instance"), Family::Store);
        assert_eq!(aws("db_instance", ""), Family::Store);
        assert_eq!(aws("lambda", "Function"), Family::Service);
        assert_eq!(aws("lambda_permission", ""), Family::External);
        assert_eq!(aws("iam", "Role"), Family::External);
        assert_eq!(
            resource_family(Provider::Gcp, "sql_database_instance", ""),
            Family::Store
        );
        assert_eq!(
            resource_family(Provider::Kubernetes, "apps.v1", "Deployment"),
            Family::Service
        );
        assert_eq!(
            resource_family(Provider::Kubernetes, "rbac.v1", "RoleBinding"),
            Family::External
        );
        assert_eq!(
            resource_family(Provider::Cloudflare, "", "R2Bucket"),
            Family::Store
        );
        assert_eq!(
            resource_family(Provider::Other, "random", "RandomPassword"),
            Family::External
        );
    }

    #[test]
    fn splits_words_and_keeps_acronyms_whole() {
        assert_eq!(kebab_camel("DatabaseInstance"), "database-instance");
        assert_eq!(kebab_camel("IAMMember"), "iam-member");
        assert_eq!(kebab_camel("URLMap"), "url-map");
        assert_eq!(kebab_camel("SecretIamMember"), "secret-iam-member");
        assert_eq!(kebab_camel("Account"), "account");
    }

    #[test]
    fn reads_only_the_top_level_main() {
        assert_eq!(
            pulumi_main("name: x\nruntime:\n  main: no\nmain: index.ts\n").as_deref(),
            Some("index.ts")
        );
        assert_eq!(
            pulumi_main("main: \"src/\" # entry\n").as_deref(),
            Some("src/")
        );
        assert_eq!(pulumi_main("name: x\n"), None);
    }
}
