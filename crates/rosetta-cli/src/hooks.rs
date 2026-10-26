//! Project-local agent hooks. The executable owns only the entries it
//! installs, and hook processes read the agent's JSON input from stdin.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;

use clap::Subcommand;
use rosetta_atlas::cli::{Host, Outcome};
use rosetta_core::config::Config;
use rosetta_core::repo::{find_root, git_toplevel};
use rosetta_glossary::Glossary;
use rosetta_glossary::lint::Linter;
use rosetta_glossary::names::NameChecker;
use serde_json::{Value, json};

#[derive(Debug, Clone, Subcommand)]
pub enum HooksCommand {
    /// Add project hooks to the selected agent's settings.
    Install { target: Target },
    /// Remove only Rosetta's project hooks.
    Uninstall { target: Target },
    /// Called by an installed hook; reads the agent's JSON from stdin.
    Run { target: Target, event: Event },
}

#[derive(Debug, Clone, Copy, clap::ValueEnum)]
pub enum Target {
    Claude,
    Cursor,
}

#[derive(Debug, Clone, Copy, clap::ValueEnum)]
pub enum Event {
    PostEdit,
    Stop,
}

type HookResult<T> = Result<T, Box<dyn std::error::Error>>;

pub fn run(command: &HooksCommand, host: &Host, out: &mut dyn Write) -> HookResult<Outcome> {
    match command {
        HooksCommand::Install {
            target: Target::Claude,
        } => change_claude_settings(host, true, out),
        HooksCommand::Uninstall {
            target: Target::Claude,
        } => change_claude_settings(host, false, out),
        HooksCommand::Install {
            target: Target::Cursor,
        } => change_cursor_settings(host, true, out),
        HooksCommand::Uninstall {
            target: Target::Cursor,
        } => change_cursor_settings(host, false, out),
        HooksCommand::Run { target, event } => run_event(*target, *event, host, out),
    }
}

fn root(host: &Host) -> HookResult<PathBuf> {
    Ok(find_root(&host.cwd)
        .or_else(|| git_toplevel(&host.cwd))
        .ok_or_else(|| format!("no Rosetta or Git repository above {}", host.cwd.display()))?)
}

fn owned_hook(event: Event) -> Value {
    json!({
        "type": "command",
        "command": "rosetta",
        "args": ["hooks", "run", "claude", match event { Event::PostEdit => "post-edit", Event::Stop => "stop" }],
        "timeout": 10
    })
}

fn owned_group(event: Event) -> Value {
    json!({
        "matcher": match event { Event::PostEdit => "Edit|Write", Event::Stop => "" },
        "hooks": [owned_hook(event)]
    })
}

fn is_owned_hook(hook: &Value, event: Event) -> bool {
    let owned = owned_hook(event);
    ["type", "command", "args"]
        .iter()
        .all(|key| hook.get(key) == owned.get(key))
}

fn change_claude_settings(host: &Host, install: bool, out: &mut dyn Write) -> HookResult<Outcome> {
    let path = root(host)?.join(".claude/settings.json");
    let original = match std::fs::read_to_string(&path) {
        Ok(text) => Some(text),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => return Err(Box::new(error)),
    };
    if !install && original.is_none() {
        writeln!(out, "No Rosetta Claude hooks installed")?;
        return Ok(Outcome::Passed);
    }
    let mut settings: Value = match &original {
        Some(text) => serde_json::from_str(text)?,
        None => json!({}),
    };
    let object = settings
        .as_object_mut()
        .ok_or(".claude/settings.json must be a JSON object")?;
    if install {
        object.entry("hooks").or_insert_with(|| json!({}));
    }
    if let Some(hooks) = object.get_mut("hooks") {
        let hooks = hooks
            .as_object_mut()
            .ok_or(".claude/settings.json hooks must be an object")?;
        for (name, event) in [("PostToolUse", Event::PostEdit), ("Stop", Event::Stop)] {
            if install {
                hooks.entry(name).or_insert_with(|| json!([]));
            }
            if let Some(groups) = hooks.get_mut(name) {
                let groups = groups
                    .as_array_mut()
                    .ok_or(".claude/settings.json hook event must be an array")?;
                if install {
                    let matcher = owned_group(event)["matcher"].clone();
                    let present =
                        groups.iter().any(|group| {
                            group.get("matcher") == Some(&matcher)
                                && group.get("hooks").and_then(Value::as_array).is_some_and(
                                    |items| items.iter().any(|hook| is_owned_hook(hook, event)),
                                )
                        });
                    if !present {
                        groups.push(owned_group(event));
                    }
                } else {
                    // Also remove our handler from a shared group, preserving
                    // every other hook and the group's matcher.
                    groups.retain_mut(|group| {
                        let Some(group_hooks) =
                            group.get_mut("hooks").and_then(Value::as_array_mut)
                        else {
                            return true;
                        };
                        let count = group_hooks.len();
                        group_hooks.retain(|hook| !is_owned_hook(hook, event));
                        count == group_hooks.len() || !group_hooks.is_empty()
                    });
                }
            }
            if !install
                && hooks
                    .get(name)
                    .and_then(Value::as_array)
                    .is_some_and(Vec::is_empty)
            {
                hooks.remove(name);
            }
        }
    }
    if original
        .as_deref()
        .is_some_and(|text| serde_json::from_str::<Value>(text).ok().as_ref() == Some(&settings))
    {
        writeln!(
            out,
            "Rosetta Claude hooks already {}",
            if install { "installed" } else { "absent" }
        )?;
        return Ok(Outcome::Passed);
    }
    std::fs::create_dir_all(root(host)?.join(".claude"))?;
    let mut text = serde_json::to_string_pretty(&settings)?;
    text.push('\n');
    std::fs::write(&path, text)?;
    writeln!(
        out,
        "{} Rosetta Claude hooks in {}",
        if install { "Installed" } else { "Removed" },
        path.display()
    )?;
    Ok(Outcome::Passed)
}

fn change_cursor_settings(host: &Host, install: bool, out: &mut dyn Write) -> HookResult<Outcome> {
    let root = root(host)?;
    let path = root.join(".cursor/hooks.json");
    let original = match std::fs::read_to_string(&path) {
        Ok(text) => Some(text),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => return Err(Box::new(error)),
    };
    if !install && original.is_none() {
        writeln!(out, "No Rosetta Cursor hooks installed")?;
        return Ok(Outcome::Passed);
    }
    let mut settings: Value = match &original {
        Some(text) => serde_json::from_str(text)?,
        None => json!({"version": 1, "hooks": {}}),
    };
    let object = settings
        .as_object_mut()
        .ok_or(".cursor/hooks.json must be a JSON object")?;
    if install {
        match object.get("version") {
            Some(Value::Number(version)) if version.as_u64() == Some(1) => {}
            None => {
                object.insert("version".to_owned(), json!(1));
            }
            _ => return Err(".cursor/hooks.json must use version 1".into()),
        }
        object.entry("hooks").or_insert_with(|| json!({}));
    }
    if let Some(hooks) = object.get_mut("hooks") {
        let hooks = hooks
            .as_object_mut()
            .ok_or(".cursor/hooks.json hooks must be an object")?;
        if install {
            hooks.entry("postToolUse").or_insert_with(|| json!([]));
        }
        let mut removed_owned = false;
        if let Some(entries) = hooks.get_mut("postToolUse") {
            let entries = entries
                .as_array_mut()
                .ok_or(".cursor/hooks.json postToolUse must be an array")?;
            let command = "rosetta hooks run cursor post-edit";
            if install {
                if !entries
                    .iter()
                    .any(|entry| entry.get("command").and_then(Value::as_str) == Some(command))
                {
                    entries
                        .push(json!({"command": command, "matcher": "Write|Edit", "timeout": 10}));
                }
            } else {
                let before = entries.len();
                entries
                    .retain(|entry| entry.get("command").and_then(Value::as_str) != Some(command));
                removed_owned = entries.len() != before;
            }
        }
        if removed_owned
            && hooks
                .get("postToolUse")
                .and_then(Value::as_array)
                .is_some_and(Vec::is_empty)
        {
            hooks.remove("postToolUse");
        }
    }
    if original
        .as_deref()
        .is_some_and(|text| serde_json::from_str::<Value>(text).ok().as_ref() == Some(&settings))
    {
        writeln!(
            out,
            "Rosetta Cursor hooks already {}",
            if install { "installed" } else { "absent" }
        )?;
        return Ok(Outcome::Passed);
    }
    std::fs::create_dir_all(root.join(".cursor"))?;
    let mut text = serde_json::to_string_pretty(&settings)?;
    text.push('\n');
    std::fs::write(&path, text)?;
    writeln!(
        out,
        "{} Rosetta Cursor hooks in {}",
        if install { "Installed" } else { "Removed" },
        path.display()
    )?;
    Ok(Outcome::Passed)
}

fn run_event(
    target: Target,
    event: Event,
    host: &Host,
    out: &mut dyn Write,
) -> HookResult<Outcome> {
    let mut input = String::new();
    std::io::stdin().read_to_string(&mut input)?;
    let input: Value = serde_json::from_str(&input)?;
    if matches!(event, Event::Stop)
        && input.get("stop_hook_active").and_then(Value::as_bool) == Some(true)
    {
        return Ok(Outcome::Passed);
    }
    let cwd = input
        .get("cwd")
        .and_then(Value::as_str)
        .map(PathBuf::from)
        .unwrap_or_else(|| host.cwd.clone());
    let root = find_root(&cwd)
        .or_else(|| git_toplevel(&cwd))
        .ok_or("hook cwd is not a repository")?;
    let config = Config::load(&root, None)?;
    let source = root.join(&config.source.directory).join("glossary.json");
    let glossary: Glossary = serde_json::from_slice(&std::fs::read(&source)?)?;
    let linter = Linter::new(&glossary)?;
    let paths = match event {
        Event::PostEdit => {
            let Some(file) = input
                .pointer("/tool_input/file_path")
                .or_else(|| input.get("file_path"))
                .and_then(Value::as_str)
            else {
                return Ok(Outcome::Passed);
            };
            let Some(relative) = relative_file(&root, &cwd, Path::new(file))? else {
                return Ok(Outcome::Passed);
            };
            vec![relative]
        }
        Event::Stop => {
            let base = branch_base(&root);
            rosetta_glossary::lint::selected_files(&root, Some(&base), &config)?
        }
    };
    let mut lines = Vec::new();
    for finding in linter.lint_files(&root, &config, &paths)? {
        lines.push(format!(
            "{}:{}: use '{}' instead of '{}' (term {})",
            finding.path, finding.line, finding.term, finding.avoided, finding.term_id
        ));
    }
    if matches!(event, Event::PostEdit) {
        let checker = NameChecker::new(&glossary, &config)?;
        for finding in checker.check_files(&root, &config, &paths)? {
            lines.push(format!(
                "{}:{}: use '{}' instead of '{}' for {} (term {})",
                finding.path,
                finding.line,
                finding.term,
                finding.avoided,
                finding.kind,
                finding.term_id
            ));
        }
    }
    if !lines.is_empty() {
        lines.sort();
        lines.dedup();
        let remaining = lines.len().saturating_sub(25);
        lines.truncate(25);
        if remaining > 0 {
            lines.push(format!(
                "{remaining} more finding(s); run rosetta glossary lint for the full report"
            ));
        }
        let context = format!("Rosetta vocabulary findings:\n{}", lines.join("\n"));
        let response = match target {
            Target::Claude => {
                json!({"hookSpecificOutput": {"hookEventName": match event { Event::PostEdit => "PostToolUse", Event::Stop => "Stop" }, "additionalContext": context}})
            }
            Target::Cursor => json!({"additional_context": context}),
        };
        writeln!(out, "{}", response)?;
    }
    Ok(Outcome::Passed)
}

fn relative_file(root: &Path, cwd: &Path, file: &Path) -> HookResult<Option<String>> {
    let absolute = if file.is_absolute() {
        file.to_path_buf()
    } else {
        cwd.join(file)
    };
    if !absolute.is_file() {
        return Ok(None);
    }
    let root = root.canonicalize()?;
    let absolute = absolute.canonicalize()?;
    let Ok(relative) = absolute.strip_prefix(root) else {
        return Ok(None);
    };
    Ok(Some(relative.to_string_lossy().replace('\\', "/")))
}

fn branch_base(root: &Path) -> String {
    if let Ok(value) = std::env::var("ROSETTA_HOOK_BASE")
        && !value.trim().is_empty()
    {
        return value;
    }
    let remote_head = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["symbolic-ref", "--quiet", "refs/remotes/origin/HEAD"])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .map(|value| value.trim().to_owned());
    if let Some(reference) = remote_head.filter(|reference| !reference.is_empty()) {
        return reference;
    }
    for reference in ["origin/main", "main", "origin/master", "master", "HEAD"] {
        let exists = Command::new("git")
            .arg("-C")
            .arg(root)
            .args([
                "rev-parse",
                "--verify",
                "--quiet",
                &format!("{reference}^{{commit}}"),
            ])
            .output()
            .is_ok_and(|output| output.status.success());
        if exists {
            return reference.to_owned();
        }
    }
    "HEAD".to_owned()
}
