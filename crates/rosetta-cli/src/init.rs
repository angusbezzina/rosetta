//! A local repository's first evidenced map and honest glossary starter.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Instant;

use clap::Args;
use rosetta_atlas::atlas::{self, AtlasLoadError};
use rosetta_atlas::cli::Outcome;
use rosetta_atlas::glossary::draft::{self, Draft, DraftInputError};
use rosetta_core::json_text;
use rosetta_glossary::{Glossary, GlossaryError};

#[derive(Debug, Clone, Args)]
pub struct InitArgs {
    /// A local Git repository directory.
    #[arg(default_value = ".")]
    pub path: PathBuf,
    #[arg(long)]
    pub no_glossary: bool,
    #[arg(long)]
    pub no_atlas: bool,
    /// A JSON answer prepared by a person or agent, added only as proposals.
    #[arg(long)]
    pub draft_answer: Option<PathBuf>,
}

#[derive(Debug, thiserror::Error)]
pub enum InitError {
    #[error("remote repository URLs are not supported by init; pass a local checkout path")]
    RemoteUnsupported,
    #[error("choose at least one of atlas or glossary")]
    NothingSelected,
    #[error("--draft-answer requires the glossary")]
    DraftWithoutGlossary,
    #[error("{path}: {source}")]
    Io {
        path: String,
        source: std::io::Error,
    },
    #[error("{0} is not a local Git repository")]
    NoRepository(String),
    #[error("{0} has no supported Cargo.toml or package.json root manifest")]
    NoManifest(String),
    #[error("atlas check failed: {0:?}")]
    AtlasCheck(Vec<String>),
    #[error(transparent)]
    Atlas(#[from] AtlasLoadError),
    #[error(transparent)]
    Glossary(#[from] GlossaryError),
    #[error(transparent)]
    DraftInput(#[from] DraftInputError),
    #[error(transparent)]
    DraftJson(#[from] serde_json::Error),
    #[error(transparent)]
    Format(#[from] json_text::FormatError),
    #[error(transparent)]
    Config(#[from] rosetta_core::config::ConfigError),
}

fn io(path: &Path) -> impl FnOnce(std::io::Error) -> InitError + '_ {
    move |source| InitError::Io {
        path: path.display().to_string(),
        source,
    }
}

fn remote_like(path: &str) -> bool {
    ["https://", "http://", "ssh://", "git://", "git@"]
        .iter()
        .any(|prefix| path.starts_with(prefix))
}

pub fn run(args: &InitArgs, out: &mut dyn Write) -> Result<Outcome, InitError> {
    let started = Instant::now();
    if remote_like(&args.path.to_string_lossy()) {
        return Err(InitError::RemoteUnsupported);
    }
    if args.no_glossary && args.no_atlas {
        return Err(InitError::NothingSelected);
    }
    if args.no_glossary && args.draft_answer.is_some() {
        return Err(InitError::DraftWithoutGlossary);
    }
    let path = std::fs::canonicalize(&args.path).map_err(io(&args.path))?;
    let root = rosetta_core::repo::git_toplevel(&path)
        .ok_or_else(|| InitError::NoRepository(path.display().to_string()))?;
    if !root.join("Cargo.toml").is_file() && !root.join("package.json").is_file() {
        return Err(InitError::NoManifest(root.display().to_string()));
    }
    let config = rosetta_core::config::Config::load(&root, None)?;
    let source_dir = config.source.directory.as_str();
    let atlas_source = format!("{source_dir}/atlas.json");
    let glossary_source = format!("{source_dir}/glossary.json");

    if !args.no_atlas {
        let changed = atlas::sync_atlas_from(&root, source_dir)?;
        let model = atlas::load_atlas_from(&root, source_dir)?;
        writeln!(
            out,
            "{} {atlas_source} ({} evidenced nodes, {} relationships)",
            if changed { "Wrote" } else { "Kept" },
            model.nodes.len(),
            model.edges.len()
        )
        .map_err(io(&root))?;
    }

    if !args.no_glossary {
        let glossary_path = root.join(&glossary_source);
        if !glossary_path.is_file() {
            let project = atlas::starter_atlas(&root)?.project.name;
            let starter = Glossary {
                schema: rosetta_glossary::model::GLOSSARY_SCHEMA.to_owned(),
                context: project,
                purpose: "Domain terms for this repository await human review.".to_owned(),
                sections: Vec::new(),
                terms: Vec::new(),
            };
            std::fs::create_dir_all(root.join(source_dir)).map_err(io(&glossary_path))?;
            std::fs::write(&glossary_path, json_text::to_pretty_string(&starter)?)
                .map_err(io(&glossary_path))?;
            writeln!(out, "Wrote {glossary_source} (no reviewed terms)").map_err(io(&root))?;
        } else {
            writeln!(out, "Kept {glossary_source}").map_err(io(&root))?;
        }
        let context = rosetta_glossary::repo_context(&root)?;
        let loaded = rosetta_glossary::load_glossary_from(&root, &glossary_source, &context);
        let glossary = loaded
            .glossary
            .ok_or(GlossaryError::Invalid(loaded.problems))?;
        rosetta_glossary::render_files_from(&root, &glossary_source, &glossary)?;
        writeln!(
            out,
            "Rendered GLOSSARY.md ({} reviewed or proposed terms)",
            glossary.terms.len()
        )
        .map_err(io(&root))?;

        if let Some(answer) = &args.draft_answer {
            let text = std::fs::read_to_string(answer).map_err(io(answer))?;
            let draft: Draft = serde_json::from_str(&text)?;
            let today = time::OffsetDateTime::now_utc().date().to_string();
            let merged = draft::apply_draft_from(
                &root,
                &glossary_source,
                &context,
                &draft,
                &glossary.context,
                &today,
            )?;
            writeln!(
                out,
                "Joined draft: {} proposed terms added, {} rejected",
                merged.added.len(),
                merged.dropped.len()
            )
            .map_err(io(&root))?;
            for dropped in &merged.dropped {
                writeln!(
                    out,
                    "  Rejected {}: {}",
                    dropped.name,
                    dropped.reasons.join("; ")
                )
                .map_err(io(&root))?;
            }
        }
        let inputs = draft::gather_inputs_from(
            &root,
            source_dir,
            &draft::existing_names_from(&root, &glossary_source),
        )?;
        let request = draft::DraftRequest::new(inputs);
        let request_relative = format!("{source_dir}/glossary-draft-request.json");
        let request_path = root.join(&request_relative);
        std::fs::write(&request_path, json_text::to_pretty_string(&request)?)
            .map_err(io(&request_path))?;
        writeln!(
            out,
            "Wrote {request_relative} for optional agent-assisted drafting"
        )
        .map_err(io(&root))?;
    }
    if !args.no_atlas {
        let exported = atlas::export::export_repo_from(&root, source_dir)?;
        let findings = atlas::check_atlas_from(&root, source_dir)?;
        if !findings.is_empty() {
            return Err(InitError::AtlasCheck(findings));
        }
        writeln!(out, "{} ATLAS.md", if exported { "Wrote" } else { "Kept" }).map_err(io(&root))?;
    }
    writeln!(
        out,
        "Known structure is evidenced by local files; domain meaning still needs review."
    )
    .map_err(io(&root))?;
    if !args.no_atlas {
        writeln!(out, "Inspect the map with `rosetta atlas render --view overview`; run `rosetta atlas check` in CI.")
            .map_err(io(&root))?;
    }
    if !args.no_glossary {
        let agent_files = if args.no_atlas {
            "GLOSSARY.md"
        } else {
            "ATLAS.md and GLOSSARY.md"
        };
        writeln!(out, "Review the draft request, add or settle terms, and run `rosetta glossary check` in CI. Tell agents in AGENTS.md to read {agent_files} before changing the repo.")
            .map_err(io(&root))?;
    }
    writeln!(out, "Elapsed: {:.1}s", started.elapsed().as_secs_f64()).map_err(io(&root))?;
    Ok(Outcome::Passed)
}
