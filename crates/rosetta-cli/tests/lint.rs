use std::io::Write;
use std::path::Path;
use std::process::{Command, Output, Stdio};

fn command(root: &Path, args: &[&str]) -> Result<Output, Box<dyn std::error::Error>> {
    Ok(Command::new(env!("CARGO_BIN_EXE_rosetta"))
        .current_dir(root)
        .args(args)
        .output()?)
}

#[test]
fn glossary_lint_text_json_sarif_strict_and_changed_only_agree()
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
    std::fs::create_dir(repo.path().join(".rosetta"))?;
    std::fs::write(
        repo.path().join(".rosetta/glossary.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "schema": "rosetta.glossary/v1", "context": "fixture", "purpose": "Shared words",
            "sections": [{ "id": "terms", "title": "Terms" }],
            "terms": [{ "id": "term.group", "name": "Group", "section": "terms", "definition": "A shared place.", "status": "settled", "avoid": [{ "word": "workspace", "except": ["Linear workspace"] }] }]
        }))?,
    )?;
    std::fs::write(repo.path().join("view.tsx"), "<p>Group</p>\n")?;
    std::fs::write(repo.path().join("guide.md"), "Linear workspace is fine.\n")?;
    git(&["add", "-A"])?;
    git(&[
        "-c",
        "user.name=Test",
        "-c",
        "user.email=test@example.invalid",
        "commit",
        "-qm",
        "base",
    ])?;
    std::fs::write(
        repo.path().join("view.tsx"),
        "<p>Use the workspace here.</p>\n",
    )?;
    let text = command(repo.path(), &["glossary", "lint", "--base", "HEAD"])?;
    assert!(text.status.success());
    let message = String::from_utf8(text.stdout)?;
    assert!(
        message.contains("view.tsx:1:12: warning: Use Group instead of workspace"),
        "{message}"
    );
    assert!(!message.contains("guide.md"));

    let json = command(
        repo.path(),
        &[
            "glossary", "lint", "--files", "view.tsx", "guide.md", "--format", "json",
        ],
    )?;
    assert!(
        json.status.success(),
        "{}",
        String::from_utf8_lossy(&json.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&json.stdout)?;
    assert_eq!(report["schema"], "rosetta.lint/v1");
    assert_eq!(report["findings"].as_array().map(Vec::len), Some(1));
    assert_eq!(report["findings"][0]["term_id"], "term.group");
    assert_eq!(report["files_checked"], 2);

    let sarif = command(
        repo.path(),
        &[
            "glossary", "lint", "--files", "view.tsx", "--format", "sarif", "--strict",
        ],
    )?;
    assert_eq!(sarif.status.code(), Some(1));
    let report: serde_json::Value = serde_json::from_slice(&sarif.stdout)?;
    assert_eq!(report["version"], "2.1.0");
    assert_eq!(report["runs"][0]["results"][0]["ruleId"], "term.group");
    assert_eq!(report["runs"][0]["results"][0]["level"], "error");
    assert_eq!(
        report["runs"][0]["results"][0]["locations"][0]["physicalLocation"]["region"]["startColumn"],
        12
    );

    let mut child = Command::new(env!("CARGO_BIN_EXE_rosetta"))
        .current_dir(repo.path())
        .args(["glossary", "lint", "--stdin", "--format", "json"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()?;
    if let Some(input) = child.stdin.as_mut() {
        input.write_all(b"A workspace for people.")?;
    }
    let output = child.wait_with_output()?;
    assert!(output.status.success());
    let report: serde_json::Value = serde_json::from_slice(&output.stdout)?;
    assert_eq!(report["findings"][0]["path"], "<stdin>");
    Ok(())
}
