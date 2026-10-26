use std::io::Write;
use std::path::Path;
use std::process::{Command, Output, Stdio};

fn run(
    root: &Path,
    args: &[&str],
    input: Option<&serde_json::Value>,
) -> Result<Output, Box<dyn std::error::Error>> {
    let mut command = Command::new(env!("CARGO_BIN_EXE_rosetta"));
    command.current_dir(root).args(args);
    if let Some(input) = input {
        let mut child = command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        child
            .stdin
            .take()
            .ok_or("hook stdin")?
            .write_all(&serde_json::to_vec(input)?)?;
        Ok(child.wait_with_output()?)
    } else {
        Ok(command.output()?)
    }
}

#[test]
fn claude_hooks_preserve_settings_and_report_edits_and_stop()
-> Result<(), Box<dyn std::error::Error>> {
    let repo = tempfile::tempdir()?;
    let git = |args: &[&str]| -> Result<(), Box<dyn std::error::Error>> {
        let result = Command::new("git")
            .arg("-C")
            .arg(repo.path())
            .args(args)
            .output()?;
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        Ok(())
    };
    git(&["init", "-q"])?;
    std::fs::create_dir_all(repo.path().join(".rosetta"))?;
    std::fs::create_dir_all(repo.path().join(".claude"))?;
    std::fs::create_dir_all(repo.path().join("src"))?;
    std::fs::write(
        repo.path().join(".rosetta/glossary.json"),
        serde_json::to_vec(&serde_json::json!({
            "schema": "rosetta.glossary/v1", "context": "fixture", "purpose": "Shared vocabulary",
            "sections": [{"id":"terms","title":"Terms"}],
            "terms": [{"id":"term.group","name":"Group","section":"terms","definition":"A shared place.","status":"settled","avoid":[{"word":"workspace"}]}]
        }))?,
    )?;
    std::fs::write(
        repo.path().join("src/Panel.tsx"),
        "export const Panel = () => <p>Group members</p>;\n",
    )?;
    std::fs::write(
        repo.path().join("package.json"),
        "{\"name\":\"group-ui\"}\n",
    )?;
    let original = serde_json::json!({
        "permissions": {"allow": ["Bash(git status)"]},
        "hooks": {"SessionStart": [{"matcher":"", "hooks":[{"type":"command","command":"bd prime --hook-json"}]}],
                  "Stop": [{"matcher":"", "hooks":[]}]}
    });
    std::fs::write(
        repo.path().join(".claude/settings.json"),
        serde_json::to_vec_pretty(&original)?,
    )?;
    git(&[
        "add",
        ".rosetta/glossary.json",
        "src/Panel.tsx",
        "package.json",
    ])?;
    git(&[
        "-c",
        "user.name=Test",
        "-c",
        "user.email=test@example.invalid",
        "commit",
        "-qm",
        "base",
    ])?;

    for _ in 0..2 {
        let result = run(repo.path(), &["hooks", "install", "claude"], None)?;
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
    }
    let settings: serde_json::Value =
        serde_json::from_slice(&std::fs::read(repo.path().join(".claude/settings.json"))?)?;
    assert_eq!(
        settings["hooks"]["PostToolUse"].as_array().map(Vec::len),
        Some(1)
    );
    assert_eq!(settings["hooks"]["Stop"].as_array().map(Vec::len), Some(2));
    assert_eq!(settings["hooks"]["PostToolUse"][0]["matcher"], "Edit|Write");
    assert_eq!(
        settings["hooks"]["PostToolUse"][0]["hooks"][0]["args"],
        serde_json::json!(["hooks", "run", "claude", "post-edit"])
    );

    std::fs::write(
        repo.path().join("src/Panel.tsx"),
        "export const Panel = () => <p>Workspace members</p>;\n",
    )?;
    let input = serde_json::json!({"cwd":repo.path(), "hook_event_name":"PostToolUse", "tool_name":"Write", "tool_input":{"file_path":repo.path().join("src/Panel.tsx")}});
    let result = run(
        repo.path(),
        &["hooks", "run", "claude", "post-edit"],
        Some(&input),
    )?;
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let feedback: serde_json::Value = serde_json::from_slice(&result.stdout)?;
    assert_eq!(
        feedback["hookSpecificOutput"]["hookEventName"],
        "PostToolUse"
    );
    let context = feedback["hookSpecificOutput"]["additionalContext"]
        .as_str()
        .ok_or("additionalContext")?;
    assert!(
        context.contains("src/Panel.tsx:1: use 'Group' instead of 'workspace'"),
        "{context}"
    );

    std::fs::write(
        repo.path().join("package.json"),
        "{\"name\":\"workspace-ui\"}\n",
    )?;
    let manifest = serde_json::json!({"cwd":repo.path(), "tool_input":{"file_path":repo.path().join("package.json")}});
    let result = run(
        repo.path(),
        &["hooks", "run", "claude", "post-edit"],
        Some(&manifest),
    )?;
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let context = String::from_utf8(result.stdout)?;
    assert!(
        context.contains("package.json:1: use 'Group' instead of 'workspace'"),
        "{context}"
    );

    let stop =
        serde_json::json!({"cwd":repo.path(),"hook_event_name":"Stop","stop_hook_active":false});
    let result = run(
        repo.path(),
        &["hooks", "run", "claude", "stop"],
        Some(&stop),
    )?;
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let context = String::from_utf8(result.stdout)?;
    assert!(
        context.contains("src/Panel.tsx:1: use 'Group' instead of 'workspace'"),
        "{context}"
    );
    let active = serde_json::json!({"cwd":repo.path(),"stop_hook_active":true});
    assert!(
        run(
            repo.path(),
            &["hooks", "run", "claude", "stop"],
            Some(&active)
        )?
        .stdout
        .is_empty()
    );

    let result = run(repo.path(), &["hooks", "uninstall", "claude"], None)?;
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let restored: serde_json::Value =
        serde_json::from_slice(&std::fs::read(repo.path().join(".claude/settings.json"))?)?;
    assert_eq!(restored, original);

    assert!(
        run(repo.path(), &["hooks", "install", "claude"], None)?
            .status
            .success()
    );
    let mut changed: serde_json::Value =
        serde_json::from_slice(&std::fs::read(repo.path().join(".claude/settings.json"))?)?;
    changed["hooks"]["PostToolUse"][0]["hooks"][0]["timeout"] = serde_json::json!(2);
    changed["hooks"]["PostToolUse"][0]["hooks"]
        .as_array_mut()
        .ok_or("hooks array")?
        .push(serde_json::json!({"type":"command","command":"echo keep"}));
    std::fs::write(
        repo.path().join(".claude/settings.json"),
        serde_json::to_vec_pretty(&changed)?,
    )?;
    assert!(
        run(repo.path(), &["hooks", "install", "claude"], None)?
            .status
            .success()
    );
    let after_reinstall: serde_json::Value =
        serde_json::from_slice(&std::fs::read(repo.path().join(".claude/settings.json"))?)?;
    assert_eq!(
        after_reinstall["hooks"]["PostToolUse"]
            .as_array()
            .map(Vec::len),
        Some(1)
    );
    assert!(
        run(repo.path(), &["hooks", "uninstall", "claude"], None)?
            .status
            .success()
    );
    let after_uninstall: serde_json::Value =
        serde_json::from_slice(&std::fs::read(repo.path().join(".claude/settings.json"))?)?;
    assert_eq!(after_uninstall["permissions"], original["permissions"]);
    assert_eq!(
        after_uninstall["hooks"]["SessionStart"],
        original["hooks"]["SessionStart"]
    );
    assert_eq!(after_uninstall["hooks"]["Stop"], original["hooks"]["Stop"]);
    assert_eq!(
        after_uninstall["hooks"]["PostToolUse"][0]["hooks"],
        serde_json::json!([{"type":"command","command":"echo keep"}])
    );
    Ok(())
}

#[test]
fn cursor_hooks_preserve_existing_entries_and_report_writes()
-> Result<(), Box<dyn std::error::Error>> {
    let repo = tempfile::tempdir()?;
    let result = Command::new("git")
        .arg("-C")
        .arg(repo.path())
        .args(["init", "-q"])
        .output()?;
    assert!(result.status.success());
    std::fs::create_dir_all(repo.path().join(".rosetta"))?;
    std::fs::create_dir_all(repo.path().join(".cursor"))?;
    std::fs::create_dir_all(repo.path().join("src"))?;
    std::fs::write(
        repo.path().join(".rosetta/glossary.json"),
        serde_json::to_vec(&serde_json::json!({
            "schema": "rosetta.glossary/v1", "context": "fixture", "purpose": "Shared vocabulary",
            "sections": [{"id":"terms","title":"Terms"}],
            "terms": [{"id":"term.group","name":"Group","section":"terms","definition":"A shared place.","status":"settled","avoid":[{"word":"workspace"}]}]
        }))?,
    )?;
    let edited = repo.path().join("src/Panel.tsx");
    std::fs::write(
        &edited,
        "export const Panel = () => <p>Workspace members</p>;\n",
    )?;
    let original = serde_json::json!({
        "version": 1,
        "hooks": {
            "postToolUse": [{"command": "bd cursor-hook postToolUse"}],
            "preCompact": [{"command": "bd cursor-hook preCompact"}]
        }
    });
    std::fs::write(
        repo.path().join(".cursor/hooks.json"),
        serde_json::to_vec_pretty(&original)?,
    )?;
    for _ in 0..2 {
        let result = run(repo.path(), &["hooks", "install", "cursor"], None)?;
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
    }
    let settings: serde_json::Value =
        serde_json::from_slice(&std::fs::read(repo.path().join(".cursor/hooks.json"))?)?;
    assert_eq!(
        settings["hooks"]["postToolUse"].as_array().map(Vec::len),
        Some(2)
    );
    assert_eq!(
        settings["hooks"]["postToolUse"][0],
        original["hooks"]["postToolUse"][0]
    );
    assert_eq!(
        settings["hooks"]["preCompact"],
        original["hooks"]["preCompact"]
    );
    assert_eq!(settings["hooks"]["postToolUse"][1]["matcher"], "Write|Edit");
    assert_eq!(
        settings["hooks"]["postToolUse"][1]["command"],
        "rosetta hooks run cursor post-edit"
    );

    let input = serde_json::json!({"cwd": repo.path(), "tool_name": "Write", "tool_input": {"file_path": edited}});
    let result = run(
        repo.path(),
        &["hooks", "run", "cursor", "post-edit"],
        Some(&input),
    )?;
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let response: serde_json::Value = serde_json::from_slice(&result.stdout)?;
    let context = response["additional_context"]
        .as_str()
        .ok_or("additional_context")?;
    assert!(
        context.contains("src/Panel.tsx:1: use 'Group' instead of 'workspace'"),
        "{context}"
    );

    let result = run(repo.path(), &["hooks", "uninstall", "cursor"], None)?;
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let restored: serde_json::Value =
        serde_json::from_slice(&std::fs::read(repo.path().join(".cursor/hooks.json"))?)?;
    assert_eq!(restored, original);
    Ok(())
}
