//! The `glossary` and `atlas` commands as clap types and run functions, so
//! the product CLI embeds them unchanged and the `virgil-atlas` binary is a
//! thin main over them. Output goes to the writers given; `--json` prints the
//! typed structs.

use std::collections::BTreeSet;
use std::ffi::OsString;
use std::io::Write;
use std::path::{Component, Path, PathBuf};

use clap::{Args, Subcommand, ValueEnum};
use serde::Serialize;

use crate::atlas::assertions;
use crate::atlas::change::{self, IntentProposal};
use crate::atlas::focus::{self, Level, Query};
use crate::atlas::{self, AtlasLoadError, BundleNode, Edge};
use crate::config;
use crate::glossary::{
    self, GLOSSARY_OUTPUT, GLOSSARY_SOURCE, GlossaryError, GlossaryTerm, NewTerm, draft,
    epic::NoEpicStatus,
};
use crate::glossary_context;
use crate::json_text;
use crate::open::{self, Brand};
use crate::repo::{find_root, git_toplevel};
use crate::site;
use rosetta_glossary::lint;
use rosetta_glossary::names;

#[derive(Debug, Clone, Subcommand)]
pub enum GlossaryCommand {
    /// Print the glossary JSON Schema without needing a repository.
    Schema,
    /// Write GLOSSARY.md and the rule.glossary-terms query from .rosetta/glossary.json.
    Render,
    /// Validate .rosetta/glossary.json and fail when a generated file differs from its render.
    Check,
    /// Write a Vale style, Whetstone query, or JSON word list from settled terms.
    Emit { target: GlossaryEmitTarget },
    /// Append a proposed term and render.
    Add(AddArgs),
    /// Prepare bounded inputs for an agent, or validate and join its answer.
    Draft(DraftArgs),
    /// Print one term, by id or name.
    Show {
        term: String,
        #[arg(long)]
        json: bool,
    },
    /// Print every term.
    List {
        #[arg(long)]
        json: bool,
    },
    /// Print a bounded vocabulary slice for paths or an Atlas view.
    Context(GlossaryContextArgs),
    /// Warn when copy uses a settled term's Avoid words.
    Lint(LintArgs),
    /// Warn when package, CLI, schema or Atlas names use Avoid words.
    Names(NamesArgs),
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum GlossaryEmitTarget {
    Vale,
    Whetstone,
    Json,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, ValueEnum)]
pub enum LintFormat {
    #[default]
    Text,
    Json,
    Sarif,
}

#[derive(Debug, Clone, Args)]
pub struct LintArgs {
    /// Scan only tracked files changed since the merge base with this ref.
    #[arg(long, conflicts_with_all = ["files", "stdin", "message_file"])]
    pub base: Option<String>,
    /// Scan only these repository-relative files.
    #[arg(long, num_args = 1.., conflicts_with_all = ["stdin", "message_file"])]
    pub files: Vec<String>,
    /// Scan standard input as plain copy.
    #[arg(long, conflicts_with = "message_file")]
    pub stdin: bool,
    /// Scan a commit-message file supplied by a hook manager as plain copy.
    #[arg(long, value_name = "PATH")]
    pub message_file: Option<PathBuf>,
    #[arg(long, value_enum, default_value_t = LintFormat::Text)]
    pub format: LintFormat,
    /// Fail when any avoided word is found. Warnings are the default.
    #[arg(long)]
    pub strict: bool,
}

#[derive(Debug, Clone, Args)]
pub struct NamesArgs {
    /// Scan only tracked names in files changed since the merge base with this ref.
    #[arg(long, conflicts_with = "files")]
    pub base: Option<String>,
    /// Scan only these repository-relative files.
    #[arg(long, num_args = 1..)]
    pub files: Vec<String>,
    #[arg(long, value_enum, default_value_t = LintFormat::Text)]
    pub format: LintFormat,
    /// Fail when any avoided name is found. Warnings are the default.
    #[arg(long)]
    pub strict: bool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, ValueEnum)]
pub enum ContextFormat {
    #[default]
    Text,
    Json,
}

#[derive(Debug, Clone, Args)]
pub struct GlossaryContextArgs {
    /// Repository-relative paths whose vocabulary is needed.
    #[arg(long = "for")]
    pub paths: Vec<String>,
    /// Reopen the exact selection from `atlas context`.
    #[arg(long, conflicts_with = "paths")]
    pub view: Option<String>,
    #[arg(long, default_value_t = 2000)]
    pub max_chars: usize,
    #[arg(long, value_enum, default_value_t = ContextFormat::Text)]
    pub format: ContextFormat,
}

#[derive(Debug, Clone, Subcommand)]
pub enum ConfigCommand {
    /// Print the effective configuration for the local repository.
    Show {
        #[arg(long)]
        json: bool,
    },
}

#[derive(Debug, Clone, Args)]
pub struct AddArgs {
    #[arg(long)]
    pub name: String,
    /// What the thing is, in at most two sentences and 280 characters.
    #[arg(long)]
    pub definition: String,
    /// A section id from .rosetta/glossary.json.
    #[arg(long)]
    pub section: String,
    /// The plan page that settled the term (a virgil://page URL).
    #[arg(long)]
    pub page: Option<String>,
    /// The epic whose landing settles the term.
    #[arg(long)]
    pub epic: Option<String>,
}

#[derive(Debug, Clone, Args)]
pub struct DraftArgs {
    /// Write request.json and prompt.md to this directory.
    #[arg(long, conflicts_with = "answer")]
    pub request: Option<PathBuf>,
    /// Read a Draft JSON answer and add only valid new terms as proposed.
    #[arg(long = "from", conflicts_with = "request")]
    pub answer: Option<PathBuf>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, ValueEnum)]
pub enum RenderFormat {
    #[default]
    Mermaid,
    Md,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, ValueEnum)]
pub enum ChangeFormat {
    #[default]
    Text,
    Json,
    Md,
}

#[derive(Debug, Clone, Subcommand)]
pub enum AtlasCommand {
    /// Print the atlas JSON Schema without needing a repository.
    Schema,
    /// Write the checked Markdown projection to ATLAS.md.
    Export,
    /// Merge the nodes derived from the workspaces and infra into .rosetta/atlas.json.
    Sync,
    /// Validate .rosetta/atlas.json and fail when it differs from a sync.
    Check {
        /// Also require summaries and tiers on parts, and glossary links on named parts.
        #[arg(long)]
        strict: bool,
        /// Print stable, structured findings for agents and CI.
        #[arg(long)]
        json: bool,
    },
    /// Print one view's Mermaid source.
    Render {
        #[arg(long)]
        view: String,
        /// The part or area a view with roots shows (connections takes any
        /// part with an edge); defaults to the view's default root.
        #[arg(long)]
        root: Option<String>,
        #[arg(long)]
        out: Option<PathBuf>,
        #[arg(long, value_enum, default_value_t = RenderFormat::Mermaid)]
        format: RenderFormat,
    },
    /// Print one node with its children and edges.
    Show {
        node: String,
        #[arg(long)]
        json: bool,
    },
    /// Write the rosetta.atlas-bundle/v1 JSON a viewer renders.
    Bundle {
        #[arg(long)]
        out: Option<PathBuf>,
    },
    /// Open the interactive Atlas for this local checkout on loopback.
    Open {
        /// Optional JSON file with primary, secondary and tertiary colours.
        #[arg(long)]
        brand: Option<PathBuf>,
        /// Loopback port; zero chooses a free port.
        #[arg(long, default_value_t = 0)]
        port: u16,
        /// Open a focused view reference from `atlas context`.
        #[arg(long)]
        view: Option<String>,
    },
    /// Export a read-only static viewer for file URLs or a static host.
    Site {
        /// Directory receiving index.html, assets and model JSON.
        #[arg(long)]
        out: PathBuf,
        /// Optional JSON file with primary, secondary and tertiary colours.
        #[arg(long)]
        brand: Option<PathBuf>,
    },
    /// Explain a bounded semantic slice with a reference the viewer can reopen.
    Context(ContextArgs),
    /// List the curated capabilities that connect purpose to parts and code.
    Capabilities {
        #[arg(long)]
        json: bool,
    },
    /// Show the parts and direct connections a repository path may affect.
    Impact {
        path: String,
        #[arg(long, value_enum, default_value_t = ChangeFormat::Text)]
        format: ChangeFormat,
    },
    /// Compare the current tree and optional intent proposal with a Git baseline.
    Changed {
        #[arg(long)]
        base: String,
        #[arg(long)]
        proposal: Option<PathBuf>,
        #[arg(long, value_enum, default_value_t = ChangeFormat::Text)]
        format: ChangeFormat,
    },
    /// Check explicit structural rules against the current local snapshot.
    Verify {
        /// Exit nonzero on violations or unknown checks; warnings are the default.
        #[arg(long)]
        strict: bool,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Debug, Clone, Args)]
pub struct ContextArgs {
    /// Reopen a prior view reference; a changed snapshot requires --refresh.
    #[arg(long)]
    pub view: Option<String>,
    /// Explicitly regenerate a stale view on the current snapshot.
    #[arg(long, requires = "view")]
    pub refresh: bool,
    /// Select a curated capability by stable id.
    #[arg(long)]
    pub capability: Option<String>,
    /// Select one or more part ids.
    #[arg(long)]
    pub root: Vec<String>,
    /// Select the most specific part covering each repository-relative path.
    #[arg(long = "for")]
    pub paths: Vec<String>,
    #[arg(long, value_enum, default_value_t = Level::Component)]
    pub level: Level,
    #[arg(long, default_value_t = 1)]
    pub depth: u8,
    /// Keep only these relation kinds.
    #[arg(long, value_enum)]
    pub kind: Vec<atlas::model::EdgeKind>,
    #[arg(long, default_value_t = 24)]
    pub max_nodes: usize,
    #[arg(long, default_value_t = 48)]
    pub max_edges: usize,
    /// Limit text output; JSON returns the complete selected view.
    #[arg(long, default_value_t = 1500)]
    pub max_chars: usize,
    #[arg(long)]
    pub json: bool,
}

/// What the embedding CLI supplies.
#[derive(Debug, Clone)]
pub struct Host {
    /// Where the repo root search starts.
    pub cwd: PathBuf,
    /// PATH supplied by an embedding host for explicit external adapters.
    pub path_env: Option<OsString>,
}

impl Host {
    /// A host for the current process.
    pub fn current() -> std::io::Result<Self> {
        Ok(Self {
            cwd: std::env::current_dir()?,
            path_env: std::env::var_os("PATH"),
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Passed,
    Failed,
}

impl Outcome {
    pub fn exit_code(self) -> i32 {
        match self {
            Outcome::Passed => 0,
            Outcome::Failed => 1,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum CliError {
    #[error("no .rosetta/ directory with repo.toml, glossary.json or atlas.json above {0}")]
    NoRepo(PathBuf),
    #[error(transparent)]
    Glossary(#[from] GlossaryError),
    #[error(transparent)]
    Atlas(#[from] AtlasLoadError),
    #[error(transparent)]
    Config(#[from] config::ConfigError),
    #[error(transparent)]
    ConfigFormat(#[from] toml::ser::Error),
    #[error(transparent)]
    Format(#[from] json_text::FormatError),
    #[error("{path}: {source}")]
    Write {
        path: String,
        source: std::io::Error,
    },
    #[error("output: {0}")]
    Output(#[from] std::io::Error),
    #[error("no term \"{0}\" in {GLOSSARY_SOURCE}")]
    UnknownTerm(String),
    #[error("term name {name:?} has several scoped meanings; use one of these ids: {ids}")]
    AmbiguousTerm { name: String, ids: String },
    #[error(transparent)]
    DraftInput(#[from] draft::DraftInputError),
    #[error("{path}: {source}")]
    DraftJson {
        path: String,
        source: serde_json::Error,
    },
    #[error("{path}: {source}")]
    DraftRead {
        path: String,
        source: std::io::Error,
    },
    #[error("{0}: answer exceeds 1 MiB")]
    DraftTooLarge(String),
    #[error("--source must name a file inside the repository")]
    InvalidSource,
    #[error(transparent)]
    Open(#[from] open::OpenError),
    #[error(transparent)]
    Site(#[from] site::SiteError),
    #[error(transparent)]
    Focus(#[from] focus::FocusError),
    #[error(transparent)]
    GlossaryContext(#[from] glossary_context::ContextError),
    #[error(transparent)]
    Change(#[from] change::ChangeError),
    #[error(transparent)]
    Assertion(#[from] assertions::AssertionError),
    #[error(transparent)]
    Lint(#[from] lint::LintError),
    #[error(transparent)]
    Names(#[from] names::NamesError),
    #[error("standard input: {0}")]
    LintInput(std::io::Error),
    #[error("{path}: {source}")]
    ProposalRead {
        path: String,
        source: std::io::Error,
    },
    #[error("{path}: {source}")]
    ProposalJson {
        path: String,
        source: serde_json::Error,
    },
    #[error("{0}: proposal exceeds 64 KiB")]
    ProposalTooLarge(String),
}

fn root_of(host: &Host) -> Result<PathBuf, CliError> {
    find_root(&host.cwd).ok_or_else(|| CliError::NoRepo(host.cwd.clone()))
}

fn emit(text: &str, out_path: Option<&Path>, out: &mut dyn Write) -> Result<(), CliError> {
    match out_path {
        Some(path) => std::fs::write(path, text).map_err(|source| CliError::Write {
            path: path.display().to_string(),
            source,
        }),
        None => Ok(out.write_all(text.as_bytes())?),
    }
}

pub fn run_config(
    command: &ConfigCommand,
    host: &Host,
    out: &mut dyn Write,
) -> Result<Outcome, CliError> {
    let root = find_root(&host.cwd)
        .or_else(|| git_toplevel(&host.cwd))
        .ok_or_else(|| CliError::NoRepo(host.cwd.clone()))?;
    let config = config::Config::load(&root, None)?;
    match command {
        ConfigCommand::Show { json: true } => {
            emit(&json_text::to_pretty_string(&config)?, None, out)?
        }
        ConfigCommand::Show { json: false } => emit(&toml::to_string_pretty(&config)?, None, out)?,
    }
    Ok(Outcome::Passed)
}

/// Each atlas finding on its own `path: why` line, as `atlas check` prints them.
pub fn write_findings(
    err: &mut dyn Write,
    findings: impl IntoIterator<Item = impl std::fmt::Display>,
) -> std::io::Result<()> {
    for finding in findings {
        writeln!(err, "{finding}")?;
    }
    Ok(())
}

pub fn run_glossary(
    command: &GlossaryCommand,
    host: &Host,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<Outcome, CliError> {
    run_glossary_from(command, host, None, out, err)
}

pub fn run_glossary_from(
    command: &GlossaryCommand,
    host: &Host,
    selected_source: Option<&str>,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<Outcome, CliError> {
    if let GlossaryCommand::Schema = command {
        let schema = schemars::schema_for!(glossary::Glossary);
        emit(&json_text::to_pretty_string(&schema)?, None, out)?;
        return Ok(Outcome::Passed);
    }
    let root = find_root(&host.cwd)
        .or_else(|| git_toplevel(&host.cwd))
        .ok_or_else(|| CliError::NoRepo(host.cwd.clone()))?;
    let config = config::Config::load(&root, None)?;
    let source = match selected_source {
        Some(source) => {
            if source.is_empty()
                || !Path::new(source)
                    .components()
                    .all(|component| matches!(component, Component::Normal(_)))
            {
                return Err(CliError::InvalidSource);
            }
            source.to_owned()
        }
        None => format!("{}/glossary.json", config.source.directory),
    };
    let context = glossary::repo_context(&root)?;
    let load = || {
        let loaded = glossary::load_glossary_from(&root, &source, &context);
        (loaded.glossary, loaded.problems)
    };
    match command {
        GlossaryCommand::Schema => unreachable!("schema was handled before repository lookup"),
        GlossaryCommand::Render => {
            let (found, problems) = load();
            let Some(found) = found else {
                for problem in problems {
                    writeln!(err, "{problem}")?;
                }
                return Ok(Outcome::Failed);
            };
            glossary::render_files_from(&root, &source, &found)?;
            writeln!(out, "Rendered {GLOSSARY_OUTPUT} from {source}")?;
            Ok(Outcome::Passed)
        }
        GlossaryCommand::Emit { target } => {
            let (found, problems) = load();
            let Some(found) = found else {
                for problem in problems {
                    writeln!(err, "{problem}")?;
                }
                return Ok(Outcome::Failed);
            };
            let target = match target {
                GlossaryEmitTarget::Vale => glossary::emit::Target::Vale,
                GlossaryEmitTarget::Whetstone => glossary::emit::Target::Whetstone,
                GlossaryEmitTarget::Json => glossary::emit::Target::Json,
            };
            for path in glossary::emit_files(&root, &found, target)? {
                writeln!(out, "Wrote {path}")?;
            }
            Ok(Outcome::Passed)
        }
        GlossaryCommand::Check => {
            let report = glossary::check_glossary_from(&root, &source, &context, &NoEpicStatus)?;
            for warning in &report.warnings {
                writeln!(err, "warning: {warning}")?;
            }
            for problem in &report.problems {
                writeln!(err, "{problem}")?;
            }
            for diff in &report.diffs {
                writeln!(err, "{diff}")?;
            }
            if report.passed() {
                writeln!(out, "{GLOSSARY_OUTPUT} matches {source}")?;
                Ok(Outcome::Passed)
            } else {
                Ok(Outcome::Failed)
            }
        }
        GlossaryCommand::Lint(args) => {
            let (found, problems) = load();
            let Some(found) = found else {
                for problem in problems {
                    writeln!(err, "{problem}")?;
                }
                return Ok(Outcome::Failed);
            };
            let linter = lint::Linter::new(&found)?;
            let (findings, file_count) = if args.stdin {
                let mut text = String::new();
                std::io::Read::read_to_string(&mut std::io::stdin(), &mut text)
                    .map_err(CliError::LintInput)?;
                (linter.lint_text("<stdin>", &text, &BTreeSet::new())?, 1)
            } else if let Some(path) = &args.message_file {
                let text = std::fs::read_to_string(path).map_err(|source| CliError::DraftRead {
                    path: path.display().to_string(),
                    source,
                })?;
                (
                    linter.lint_text("<commit-message>", &text, &BTreeSet::new())?,
                    1,
                )
            } else {
                let paths = if args.files.is_empty() {
                    lint::selected_files(&root, args.base.as_deref(), &config)?
                } else {
                    args.files.clone()
                };
                let count = paths.len();
                (linter.lint_files(&root, &config, &paths)?, count)
            };
            let strict = args.strict || config.lint.strict;
            match args.format {
                LintFormat::Text => {
                    for finding in &findings {
                        writeln!(
                            out,
                            "{}:{}:{}: {}: {} (term {})",
                            finding.path,
                            finding.line,
                            finding.column,
                            if strict { "error" } else { "warning" },
                            finding.message,
                            finding.term_id
                        )?;
                    }
                    writeln!(
                        out,
                        "{} finding(s) in {} selected file(s)",
                        findings.len(),
                        file_count
                    )?;
                }
                LintFormat::Json => emit(
                    &json_text::to_pretty_string(&serde_json::json!({
                        "schema": "rosetta.lint/v1", "strict": strict, "files_checked": file_count, "findings": findings,
                    }))?,
                    None,
                    out,
                )?,
                LintFormat::Sarif => {
                    let rules = findings.iter().map(|finding| (&finding.term_id, &finding.term)).collect::<BTreeSet<_>>()
                        .into_iter().map(|(id, term)| serde_json::json!({"id": id, "shortDescription": {"text": format!("Use {term} in copy")}})).collect::<Vec<_>>();
                    let results = findings.iter().map(|finding| serde_json::json!({
                        "ruleId": finding.term_id, "level": if strict { "error" } else { "warning" },
                        "message": { "text": finding.message },
                        "locations": [{ "physicalLocation": {
                            "artifactLocation": { "uri": finding.path },
                            "region": { "startLine": finding.line, "startColumn": finding.column }
                        }}]
                    })).collect::<Vec<_>>();
                    emit(
                        &json_text::to_pretty_string(&serde_json::json!({
                            "$schema": "https://json.schemastore.org/sarif-2.1.0.json", "version": "2.1.0",
                            "runs": [{ "tool": { "driver": { "name": "Rosetta glossary", "rules": rules } }, "results": results }],
                        }))?,
                        None,
                        out,
                    )?;
                }
            }
            Ok(if strict && !findings.is_empty() {
                Outcome::Failed
            } else {
                Outcome::Passed
            })
        }
        GlossaryCommand::Names(args) => {
            let (found, problems) = load();
            let Some(found) = found else {
                for problem in problems {
                    writeln!(err, "{problem}")?;
                }
                return Ok(Outcome::Failed);
            };
            let checker = names::NameChecker::new(&found, &config)?;
            let paths = if args.files.is_empty() {
                checker.selected_files(&root, args.base.as_deref())?
            } else {
                args.files.clone()
            };
            let findings = checker.check_files(&root, &config, &paths)?;
            let strict = args.strict || config.lint.strict;
            match args.format {
                LintFormat::Text => {
                    for finding in &findings {
                        writeln!(
                            out,
                            "{}:{}:{}: {}: {} (term {})",
                            finding.path,
                            finding.line,
                            finding.column,
                            if strict { "error" } else { "warning" },
                            finding.message,
                            finding.term_id
                        )?;
                    }
                    writeln!(
                        out,
                        "{} name finding(s) in {} selected file(s)",
                        findings.len(),
                        paths.len()
                    )?;
                }
                LintFormat::Json => emit(
                    &json_text::to_pretty_string(&serde_json::json!({
                        "schema": "rosetta.names/v1", "strict": strict, "files_checked": paths.len(), "findings": findings,
                    }))?,
                    None,
                    out,
                )?,
                LintFormat::Sarif => {
                    let rules = findings.iter().map(|finding| (&finding.term_id, &finding.term)).collect::<BTreeSet<_>>()
                        .into_iter().map(|(id, term)| serde_json::json!({"id": id, "shortDescription": {"text": format!("Use {term} at naming boundaries")}})).collect::<Vec<_>>();
                    let results = findings.iter().map(|finding| serde_json::json!({
                        "ruleId": finding.term_id, "level": if strict { "error" } else { "warning" },
                        "message": { "text": finding.message },
                        "locations": [{ "physicalLocation": {
                            "artifactLocation": { "uri": finding.path },
                            "region": { "startLine": finding.line, "startColumn": finding.column }
                        }}]
                    })).collect::<Vec<_>>();
                    emit(
                        &json_text::to_pretty_string(&serde_json::json!({
                            "$schema": "https://json.schemastore.org/sarif-2.1.0.json", "version": "2.1.0",
                            "runs": [{ "tool": { "driver": { "name": "Rosetta glossary names", "rules": rules } }, "results": results }],
                        }))?,
                        None,
                        out,
                    )?;
                }
            }
            Ok(if strict && !findings.is_empty() {
                Outcome::Failed
            } else {
                Outcome::Passed
            })
        }
        GlossaryCommand::Add(args) => {
            let added = glossary::add_term_from(
                &root,
                &source,
                &context,
                NewTerm {
                    name: args.name.clone(),
                    definition: args.definition.clone(),
                    section: args.section.clone(),
                    page: args.page.clone(),
                    epic: args.epic.clone(),
                },
                &glossary::date_utc(time::OffsetDateTime::now_utc().unix_timestamp() * 1_000),
            );
            match added {
                Ok(term) => {
                    writeln!(out, "Added {} ({}) as proposed", term.name, term.id)?;
                    Ok(Outcome::Passed)
                }
                Err(GlossaryError::Invalid(problems)) => {
                    for problem in problems {
                        writeln!(err, "{problem}")?;
                    }
                    Ok(Outcome::Failed)
                }
                Err(error) => Err(error.into()),
            }
        }
        GlossaryCommand::Draft(args) => match (&args.request, &args.answer) {
            (Some(directory), None) => {
                let inputs = draft::gather_inputs_from(
                    &root,
                    &config.source.directory,
                    &draft::existing_names_from(&root, &source),
                )?;
                let request = draft::DraftRequest::new(inputs);
                std::fs::create_dir_all(directory).map_err(|source| CliError::Write {
                    path: directory.display().to_string(),
                    source,
                })?;
                for (name, body) in [
                    ("request.json", json_text::to_pretty_string(&request)?),
                    ("prompt.md", draft::PROMPT.to_owned()),
                ] {
                    let path = directory.join(name);
                    std::fs::write(&path, body).map_err(|source| CliError::Write {
                        path: path.display().to_string(),
                        source,
                    })?;
                    writeln!(out, "Wrote {}", path.display())?;
                }
                Ok(Outcome::Passed)
            }
            (None, Some(path)) => {
                let bytes = std::fs::read(path).map_err(|source| CliError::DraftRead {
                    path: path.display().to_string(),
                    source,
                })?;
                if bytes.len() > 1024 * 1024 {
                    return Err(CliError::DraftTooLarge(path.display().to_string()));
                }
                let answer: draft::Draft =
                    serde_json::from_slice(&bytes).map_err(|source| CliError::DraftJson {
                        path: path.display().to_string(),
                        source,
                    })?;
                let project = if root.join(&source).is_file() {
                    let loaded = glossary::load_glossary_from(&root, &source, &context);
                    loaded
                        .glossary
                        .ok_or(GlossaryError::Invalid(loaded.problems))?
                        .context
                } else {
                    atlas::starter_atlas(&root)?.project.name
                };
                let today = time::OffsetDateTime::now_utc().date().to_string();
                let merged =
                    draft::apply_draft_from(&root, &source, &context, &answer, &project, &today)?;
                writeln!(
                    out,
                    "{} proposed terms added, {} already present, {} rejected",
                    merged.added.len(),
                    merged.already_present.len(),
                    merged.dropped.len()
                )?;
                for dropped in &merged.dropped {
                    writeln!(
                        err,
                        "rejected {}: {}",
                        dropped.name,
                        dropped.reasons.join("; ")
                    )?;
                }
                Ok(Outcome::Passed)
            }
            (None, None) => {
                writeln!(
                    out,
                    "Run `rosetta glossary draft --request <dir>`, give that directory's prompt.md and request.json to your coding agent, then run `rosetta glossary draft --from <dir>/answer.json`. All joined terms remain proposed until reviewed."
                )?;
                Ok(Outcome::Passed)
            }
            (Some(_), Some(_)) => unreachable!("clap rejects simultaneous request and answer"),
        },
        GlossaryCommand::Show { term, json } => {
            let (found, problems) = load();
            let Some(found) = found else {
                for problem in problems {
                    writeln!(err, "{problem}")?;
                }
                return Ok(Outcome::Failed);
            };
            let entry = glossary::find_term(&found, term).ok_or_else(|| {
                let matches: Vec<&str> = found
                    .terms
                    .iter()
                    .filter(|entry| entry.name.eq_ignore_ascii_case(term))
                    .map(|entry| entry.id.as_str())
                    .collect();
                if matches.len() > 1 {
                    CliError::AmbiguousTerm {
                        name: term.clone(),
                        ids: matches.join(", "),
                    }
                } else {
                    CliError::UnknownTerm(term.clone())
                }
            })?;
            if *json {
                emit(&json_text::to_pretty_string(entry)?, None, out)?;
            } else {
                write_term(out, entry)?;
            }
            Ok(Outcome::Passed)
        }
        GlossaryCommand::List { json } => {
            let (found, problems) = load();
            let Some(found) = found else {
                for problem in problems {
                    writeln!(err, "{problem}")?;
                }
                return Ok(Outcome::Failed);
            };
            let mut terms: Vec<&GlossaryTerm> = found.terms.iter().collect();
            terms.sort_by(|left, right| left.id.cmp(&right.id));
            if *json {
                emit(&json_text::to_pretty_string(&terms)?, None, out)?;
            } else {
                for term in terms {
                    writeln!(out, "{}\t{}\t{}", term.id, term.name, term.status.as_str())?;
                }
            }
            Ok(Outcome::Passed)
        }
        GlossaryCommand::Context(args) => {
            let (found, problems) = load();
            let Some(found) = found else {
                for problem in problems {
                    writeln!(err, "{problem}")?;
                }
                return Ok(Outcome::Failed);
            };
            let report = glossary_context::select(
                &root,
                &config,
                &found,
                &args.paths,
                args.view.as_deref(),
            )?;
            match args.format {
                ContextFormat::Json => emit(&json_text::to_pretty_string(&report)?, None, out)?,
                ContextFormat::Text => emit(
                    &glossary_context::render_text(&report, args.max_chars)?,
                    None,
                    out,
                )?,
            }
            Ok(Outcome::Passed)
        }
    }
}

fn write_term(out: &mut dyn Write, term: &GlossaryTerm) -> std::io::Result<()> {
    writeln!(out, "{} ({}, {})", term.name, term.id, term.status.as_str())?;
    writeln!(
        out,
        "{}",
        glossary::wrap(&term.definition, glossary::render::WRAP_COLUMNS)
    )?;
    for avoid in term.avoid_words() {
        writeln!(out, "avoid: {}", avoid.word)?;
    }
    for code in term.code.iter().flatten() {
        writeln!(out, "code: {code}")?;
    }
    if let Some(epic) = &term.epic {
        writeln!(out, "epic: {epic}")?;
    }
    Ok(())
}

/// A node with what connects to it, as `atlas show --json` prints it.
#[derive(Debug, Clone, Serialize)]
pub struct NodeDetail {
    pub node: BundleNode,
    pub children: Vec<String>,
    pub edges_out: Vec<Edge>,
    pub edges_in: Vec<Edge>,
}

pub fn run_atlas(
    command: &AtlasCommand,
    host: &Host,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<Outcome, CliError> {
    if let AtlasCommand::Schema = command {
        let schema = schemars::schema_for!(atlas::Atlas);
        emit(&json_text::to_pretty_string(&schema)?, None, out)?;
        return Ok(Outcome::Passed);
    }
    // A sync writes the first .rosetta/atlas.json, so it also takes a git
    // checkout that has no .rosetta/ yet.
    let root = match command {
        AtlasCommand::Sync => find_root(&host.cwd)
            .or_else(|| git_toplevel(&host.cwd))
            .ok_or_else(|| CliError::NoRepo(host.cwd.clone()))?,
        _ => root_of(host)?,
    };
    let config = config::Config::load(&root, None)?;
    let source_dir = config.source.directory.as_str();
    let atlas_source = format!("{source_dir}/atlas.json");
    match command {
        AtlasCommand::Schema => unreachable!("schema was handled before repository lookup"),
        AtlasCommand::Open { brand, port, view } => {
            let brand = brand
                .as_deref()
                .map(Brand::from_file)
                .transpose()?
                .unwrap_or_default();
            open::serve(&root, source_dir, &brand, *port, view.as_deref(), out)?;
            Ok(Outcome::Passed)
        }
        AtlasCommand::Site {
            out: directory,
            brand,
        } => {
            let brand = brand
                .as_deref()
                .map(Brand::from_file)
                .transpose()?
                .unwrap_or_default();
            site::export(&root, source_dir, &brand, directory)?;
            writeln!(out, "Wrote static Atlas to {}", directory.display())?;
            Ok(Outcome::Passed)
        }
        AtlasCommand::Context(args) => {
            let bundle = atlas::bundle_repo_from(&root, source_dir)?;
            let view = if let Some(reference) = &args.view {
                if args.refresh {
                    focus::refresh(&bundle, reference)?
                } else {
                    focus::reopen(&bundle, reference)?
                }
            } else {
                focus::select(
                    &bundle,
                    Query {
                        capability: args.capability.clone(),
                        roots: args.root.clone(),
                        paths: args.paths.clone(),
                        level: args.level,
                        depth: args.depth,
                        kinds: args.kind.clone(),
                        max_nodes: args.max_nodes,
                        max_edges: args.max_edges,
                    },
                )?
            };
            if args.json {
                emit(&json_text::to_pretty_string(&view)?, None, out)?;
            } else {
                emit(&focus::render_context(&view, args.max_chars)?, None, out)?;
            }
            Ok(Outcome::Passed)
        }
        AtlasCommand::Impact { path, format } => {
            let report = change::impact_repo(&root, source_dir, path)?;
            match format {
                ChangeFormat::Text => emit(&change::render_impact(&report, false), None, out)?,
                ChangeFormat::Md => emit(&change::render_impact(&report, true), None, out)?,
                ChangeFormat::Json => emit(&json_text::to_pretty_string(&report)?, None, out)?,
            }
            Ok(Outcome::Passed)
        }
        AtlasCommand::Changed {
            base,
            proposal,
            format,
        } => {
            let proposal = proposal
                .as_ref()
                .map(|path| {
                    let bytes = std::fs::read(path).map_err(|source| CliError::ProposalRead {
                        path: path.display().to_string(),
                        source,
                    })?;
                    if bytes.len() > 64 * 1024 {
                        return Err(CliError::ProposalTooLarge(path.display().to_string()));
                    }
                    serde_json::from_slice::<IntentProposal>(&bytes).map_err(|source| {
                        CliError::ProposalJson {
                            path: path.display().to_string(),
                            source,
                        }
                    })
                })
                .transpose()?;
            let report = change::changed(&root, source_dir, base, proposal.as_ref())?;
            match format {
                ChangeFormat::Text => emit(&change::render_changed(&report, false), None, out)?,
                ChangeFormat::Md => emit(&change::render_changed(&report, true), None, out)?,
                ChangeFormat::Json => emit(&json_text::to_pretty_string(&report)?, None, out)?,
            }
            Ok(Outcome::Passed)
        }
        AtlasCommand::Verify { strict, json } => {
            let report = assertions::verify(&root, source_dir)?;
            if *json {
                emit(&json_text::to_pretty_string(&report)?, None, out)?;
            } else {
                emit(&assertions::render(&report), None, out)?;
            }
            if *strict
                && report
                    .findings
                    .iter()
                    .any(|finding| finding.status != assertions::Status::Pass)
            {
                Ok(Outcome::Failed)
            } else {
                Ok(Outcome::Passed)
            }
        }
        AtlasCommand::Capabilities { json } => {
            let model = atlas::load_atlas_from(&root, source_dir)?;
            let mut capabilities = model.capabilities;
            capabilities.sort_by(|left, right| left.id.cmp(&right.id));
            if *json {
                emit(&json_text::to_pretty_string(&capabilities)?, None, out)?;
            } else if capabilities.is_empty() {
                writeln!(
                    out,
                    "No curated capabilities yet. The structural atlas remains available; add a reviewed capability to {atlas_source} to connect purpose, terms, parts and implementation paths."
                )?;
            } else {
                for capability in capabilities {
                    writeln!(
                        out,
                        "{}\t{}\t{:?}\t{}",
                        capability.id, capability.name, capability.status, capability.purpose
                    )?;
                }
            }
            Ok(Outcome::Passed)
        }
        AtlasCommand::Export => {
            let changed = atlas::export::export_repo_from(&root, source_dir)?;
            writeln!(
                out,
                "{} {}",
                if changed {
                    "Exported"
                } else {
                    "Already current:"
                },
                atlas::export::ATLAS_OUTPUT
            )?;
            Ok(Outcome::Passed)
        }
        AtlasCommand::Sync => {
            let changed = atlas::sync_atlas_from(&root, source_dir)?;
            let what = if changed {
                "Synced"
            } else {
                "Already in sync:"
            };
            writeln!(out, "{what} {atlas_source}")?;
            Ok(Outcome::Passed)
        }
        AtlasCommand::Check { strict, json } => {
            let findings = atlas::check_atlas_with_mode_from(&root, source_dir, *strict)?;
            if *json {
                let objects = findings
                    .iter()
                    .map(|finding| {
                        let (path, message) =
                            finding.split_once(": ").unwrap_or(("", finding.as_str()));
                        serde_json::json!({
                            "path": if path.is_empty() { None } else { Some(path) },
                            "message": message,
                        })
                    })
                    .collect::<Vec<_>>();
                emit(
                    &json_text::to_pretty_string(&serde_json::json!({
                        "schema": "rosetta.atlas-check/v1",
                        "strict": strict,
                        "passed": findings.is_empty(),
                        "findings": objects,
                    }))?,
                    None,
                    out,
                )?;
                return Ok(if findings.is_empty() {
                    Outcome::Passed
                } else {
                    Outcome::Failed
                });
            }
            if findings.is_empty() {
                writeln!(out, "{atlas_source} is valid and in sync")?;
                return Ok(Outcome::Passed);
            }
            write_findings(err, findings)?;
            Ok(Outcome::Failed)
        }
        AtlasCommand::Render {
            view,
            root: picked,
            out: out_path,
            format,
        } => {
            let bundle = atlas::bundle_repo_from(&root, source_dir)?;
            let source = atlas::render_view(&bundle, view, picked.as_deref())?;
            let rendered = match format {
                RenderFormat::Mermaid => source,
                RenderFormat::Md => {
                    let selected = bundle
                        .views
                        .iter()
                        .find(|candidate| candidate.id == *view)
                        .ok_or_else(|| AtlasLoadError::UnknownView(view.clone()))?;
                    let mut markdown = format!(
                        "## {}\n\n{}\n\n",
                        selected.title.replace(['\r', '\n'], " "),
                        selected.description
                    );
                    if let Some(focus) = picked.as_deref().or(Some(selected.default_root.as_str()))
                        && !focus.is_empty()
                    {
                        markdown.push_str(&format!("Focus: `{focus}`.\n\n"));
                    }
                    markdown.push_str("Legend: arrows show flow. Reads point from data source to reader; writes point from writer to destination. Labels name relationships; cylinders are stores; groups are tiers or areas.\n\n```mermaid\n");
                    markdown.push_str(&atlas::page::without_clicks(&source));
                    markdown.push_str("```\n");
                    markdown
                }
            };
            emit(&rendered, out_path.as_deref(), out)?;
            Ok(Outcome::Passed)
        }
        AtlasCommand::Show { node, json } => {
            let bundle = atlas::bundle_repo_from(&root, source_dir)?;
            let found = bundle
                .nodes
                .iter()
                .find(|candidate| &candidate.id == node)
                .ok_or_else(|| AtlasLoadError::UnknownNode(node.clone()))?;
            let detail = NodeDetail {
                node: found.clone(),
                children: bundle
                    .nodes
                    .iter()
                    .filter(|child| child.parent.as_deref() == Some(node.as_str()))
                    .map(|child| child.id.clone())
                    .collect(),
                edges_out: bundle
                    .edges
                    .iter()
                    .filter(|e| &e.from == node)
                    .cloned()
                    .collect(),
                edges_in: bundle
                    .edges
                    .iter()
                    .filter(|e| &e.to == node)
                    .cloned()
                    .collect(),
            };
            if *json {
                emit(&json_text::to_pretty_string(&detail)?, None, out)?;
            } else {
                write_node(out, &detail)?;
            }
            Ok(Outcome::Passed)
        }
        AtlasCommand::Bundle { out: out_path } => {
            let bundle = atlas::bundle_repo_from(&root, source_dir)?;
            emit(
                &json_text::to_pretty_string(&bundle)?,
                out_path.as_deref(),
                out,
            )?;
            Ok(Outcome::Passed)
        }
    }
}

fn write_node(out: &mut dyn Write, detail: &NodeDetail) -> std::io::Result<()> {
    let node = &detail.node;
    writeln!(
        out,
        "{} ({}, {} {})",
        node.label,
        node.id,
        node.family.as_str(),
        node.kind.as_str()
    )?;
    if let Some(path) = &node.path {
        writeln!(out, "path: {path}")?;
    }
    if let Some(summary) = &node.summary {
        writeln!(out, "{summary}")?;
    }
    if let Some(lines) = node.lines {
        writeln!(out, "lines: {lines}")?;
    }
    if let Some(updated) = &node.updated {
        writeln!(out, "updated: {} {}", updated.at, updated.subject)?;
    }
    if let Some(term) = &node.refs.term {
        writeln!(out, "term: {term}")?;
    }
    for rule in node.refs.rules.iter().chain(&node.refs.principles) {
        writeln!(out, "rule: {rule}")?;
    }
    for doc in &node.refs.docs {
        writeln!(out, "doc: {doc}")?;
    }
    for child in &detail.children {
        writeln!(out, "child: {child}")?;
    }
    for edge in &detail.edges_out {
        writeln!(out, "{} {} {}", edge.from, edge.kind.as_str(), edge.to)?;
    }
    for edge in &detail.edges_in {
        writeln!(out, "{} {} {}", edge.from, edge.kind.as_str(), edge.to)?;
    }
    Ok(())
}
