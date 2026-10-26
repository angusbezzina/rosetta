use std::path::Path;
use std::process::{Command, Output};

use rosetta_glossary::{emit, model::Glossary, query::glossary_query};

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn git(root: &Path, args: &[&str]) -> TestResult {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()?;
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(())
}

fn rosetta(root: &Path, args: &[&str]) -> Result<Output, Box<dyn std::error::Error>> {
    Ok(Command::new(env!("CARGO_BIN_EXE_rosetta"))
        .current_dir(root)
        .args(args)
        .output()?)
}

#[test]
fn emitted_formats_are_deterministic_and_checked_when_staged() -> TestResult {
    let dir = tempfile::tempdir()?;
    let root = dir.path();
    git(root, &["init", "-q"])?;
    std::fs::create_dir_all(root.join(".rosetta"))?;
    let source = serde_json::json!({
        "schema":"rosetta.glossary/v1", "context":"fixture", "purpose":"Shared vocabulary.",
        "sections":[{"id":"product","title":"Product"}],
        "terms":[
            {"id":"term.group","name":"Group","section":"product","definition":"A shared place.","status":"settled","avoid":[{"word":"workspace","except":["Slack workspace"]}]},
            {"id":"term.future","name":"Future","section":"product","definition":"A proposed concept.","status":"proposed"}
        ]
    });
    std::fs::write(
        root.join(".rosetta/glossary.json"),
        serde_json::to_vec(&source)?,
    )?;
    let glossary: Glossary = serde_json::from_value(source)?;
    assert!(rosetta(root, &["glossary", "render"])?.status.success());

    for target in ["vale", "whetstone", "json"] {
        let first = rosetta(root, &["glossary", "emit", target])?;
        assert!(
            first.status.success(),
            "{}",
            String::from_utf8_lossy(&first.stderr)
        );
        let second = rosetta(root, &["glossary", "emit", target])?;
        assert_eq!(first.stdout, second.stdout);
    }
    assert_eq!(
        std::fs::read_to_string(root.join(emit::WHETSTONE_QUERY))?,
        glossary_query(&glossary).ok_or("generated query")?
    );
    let words: serde_json::Value =
        serde_json::from_slice(&std::fs::read(root.join(emit::WORD_LIST))?)?;
    assert_eq!(words["schema"], "rosetta.word-list/v1");
    assert_eq!(words["terms"].as_array().map(Vec::len), Some(1));
    assert_eq!(
        words["terms"][0]["avoid"][0]["except"][0],
        "Slack workspace"
    );
    let style = std::fs::read_to_string(root.join(".vale/styles/Rosetta/Term-group.yml"))?;
    assert!(style.contains("extends: substitution"));
    assert!(style.contains("Slack workspace"));
    assert!(std::fs::read_to_string(root.join(emit::VALE_ACCEPT))?.contains("Group"));

    git(root, &["add", "-A"])?;
    let check = rosetta(root, &["glossary", "check"])?;
    assert!(
        check.status.success(),
        "{}",
        String::from_utf8_lossy(&check.stderr)
    );

    std::fs::write(root.join(emit::WORD_LIST), "{}\n")?;
    let check = rosetta(root, &["glossary", "check"])?;
    assert_eq!(check.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&check.stderr).contains(emit::WORD_LIST));
    assert!(
        rosetta(root, &["glossary", "emit", "json"])?
            .status
            .success()
    );

    // A committed Vale adapter is complete only while every generated rule
    // remains staged. A newly added term must therefore add its rule too.
    let mut changed: serde_json::Value =
        serde_json::from_slice(&std::fs::read(root.join(".rosetta/glossary.json"))?)?;
    changed["terms"].as_array_mut().ok_or("terms")?.push(serde_json::json!({"id":"term.task","name":"Task","section":"product","definition":"A unit of work.","status":"settled","avoid":[{"word":"bead"}]}));
    std::fs::write(
        root.join(".rosetta/glossary.json"),
        serde_json::to_vec(&changed)?,
    )?;
    assert!(rosetta(root, &["glossary", "render"])?.status.success());
    let check = rosetta(root, &["glossary", "check"])?;
    assert_eq!(check.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&check.stderr).contains("Term-task.yml"));
    Ok(())
}

#[test]
fn commit_message_file_warns_and_strict_mode_blocks() -> TestResult {
    let dir = tempfile::tempdir()?;
    let root = dir.path();
    git(root, &["init", "-q"])?;
    std::fs::create_dir_all(root.join(".rosetta"))?;
    std::fs::write(
        root.join(".rosetta/glossary.json"),
        serde_json::to_vec(&serde_json::json!({
            "schema":"rosetta.glossary/v1", "context":"fixture", "purpose":"Shared vocabulary.",
            "sections":[{"id":"product","title":"Product"}],
            "terms":[{"id":"term.group","name":"Group","section":"product","definition":"A shared place.","status":"settled","avoid":[{"word":"workspace"}]}]
        }))?,
    )?;
    let message = root.join("COMMIT_EDITMSG");
    std::fs::write(&message, "Rename the workspace panel\n")?;
    let path = message.to_str().ok_or("commit path")?;
    let warn = rosetta(root, &["glossary", "lint", "--message-file", path])?;
    assert!(warn.status.success());
    assert!(String::from_utf8_lossy(&warn.stdout).contains("Use Group instead of workspace"));
    let strict = rosetta(
        root,
        &["glossary", "lint", "--message-file", path, "--strict"],
    )?;
    assert_eq!(strict.status.code(), Some(1));
    Ok(())
}
