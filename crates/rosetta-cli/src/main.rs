//! `rosetta`: glossary and atlas commands for a local repository.

use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use rosetta_atlas::cli::{
    AtlasCommand, ConfigCommand, GlossaryCommand, Host, run_atlas, run_config, run_glossary_from,
};

mod hooks;
mod init;

#[derive(Debug, Parser)]
#[command(name = "rosetta", about = "A repo's glossary and atlas")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    Init(init::InitArgs),
    /// Print a short AGENTS.md or CLAUDE.md pointer for this repository.
    AgentsSnippet,
    #[command(subcommand)]
    Hooks(hooks::HooksCommand),
    #[command(subcommand)]
    Config(ConfigCommand),
    Glossary {
        /// Repository-relative glossary JSON file; overrides rosetta.toml.
        #[arg(long, global = true)]
        source: Option<String>,
        #[command(subcommand)]
        command: GlossaryCommand,
    },
    #[command(subcommand)]
    Atlas(AtlasCommand),
    /// Serve a local repository over MCP stdio.
    Mcp {
        /// Repository root; defaults to the current directory.
        #[arg(long)]
        repo: Option<PathBuf>,
    },
}

const AGENTS_SNIPPET: &str = concat!(
    "Read skills/rosetta/SKILL.md when editing concepts, names, or repository structure.\n",
    "Treat .rosetta/glossary.json and .rosetta/atlas.json as source; render GLOSSARY.md and ATLAS.md.\n",
    "Use rosetta glossary context and rosetta atlas context for task paths; run their checks before handoff.\n",
);

fn main() -> ExitCode {
    let cli = Cli::parse();
    if let Command::Mcp { repo } = &cli.command {
        let result = (|| -> Result<(), Box<dyn std::error::Error>> {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()?;
            runtime.block_on(rosetta_mcp::serve(
                repo.clone().unwrap_or(std::env::current_dir()?),
            ))
        })();
        return match result {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => {
                let _ = writeln!(std::io::stderr().lock(), "error: {error}");
                ExitCode::FAILURE
            }
        };
    }
    let mut out = std::io::stdout().lock();
    let mut err = std::io::stderr().lock();
    let result: Result<rosetta_atlas::cli::Outcome, Box<dyn std::error::Error>> =
        match Host::current() {
            Err(error) => Err(Box::new(error)),
            Ok(host) => match &cli.command {
                Command::Init(args) => init::run(args, &mut out).map_err(Into::into),
                Command::AgentsSnippet => out
                    .write_all(AGENTS_SNIPPET.as_bytes())
                    .map(|()| rosetta_atlas::cli::Outcome::Passed)
                    .map_err(Into::into),
                Command::Hooks(command) => hooks::run(command, &host, &mut out),
                Command::Config(command) => {
                    run_config(command, &host, &mut out).map_err(Into::into)
                }
                Command::Glossary { source, command } => {
                    run_glossary_from(command, &host, source.as_deref(), &mut out, &mut err)
                        .map_err(Into::into)
                }
                Command::Atlas(command) => {
                    run_atlas(command, &host, &mut out, &mut err).map_err(Into::into)
                }
                Command::Mcp { .. } => unreachable!("handled before stdout is locked"),
            },
        };
    match result {
        Ok(outcome) => ExitCode::from(outcome.exit_code() as u8),
        Err(error) => {
            let _ = writeln!(err, "error: {error}");
            ExitCode::from(2)
        }
    }
}
