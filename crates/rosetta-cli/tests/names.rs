use std::path::Path;
use std::process::{Command, Output};

fn command(root: &Path, args: &[&str]) -> Result<Output, Box<dyn std::error::Error>> {
    Ok(Command::new(env!("CARGO_BIN_EXE_rosetta"))
        .current_dir(root)
        .args(args)
        .output()?)
}

#[test]
fn naming_boundaries_warn_and_changed_only_limits_the_check()
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
    for directory in [
        ".rosetta",
        "crates/cli/src",
        "crates/feature",
        "apps/web",
        "schema",
        "tools",
    ] {
        std::fs::create_dir_all(repo.path().join(directory))?;
    }
    std::fs::write(
        repo.path().join(".rosetta/glossary.json"),
        serde_json::to_vec(&serde_json::json!({
            "schema": "rosetta.glossary/v1", "context": "fixture", "purpose": "Shared names",
            "sections": [{ "id": "terms", "title": "Terms" }],
            "terms": [{ "id": "term.group", "name": "Group", "section": "terms", "definition": "A shared place.", "status": "settled", "avoid": [{ "word": "workspace" }] }]
        }))?,
    )?;
    std::fs::write(
        repo.path().join(".rosetta/rosetta.toml"),
        "[names]\nrust_cli = ['crates/cli/src/main.rs']\ntypescript_cli = ['tools/cli.ts']\ntypescript_ids = ['schema/**/*.ts']\n",
    )?;
    std::fs::write(
        repo.path().join("crates/feature/Cargo.toml"),
        "[package]\nname = \"group-core\"\nversion = \"0.1.0\"\n",
    )?;
    std::fs::write(
        repo.path().join("apps/web/package.json"),
        "{\"name\":\"@scope/workspace-ui\"}\n",
    )?;
    std::fs::write(
        repo.path().join(".rosetta/atlas.json"),
        "{\"nodes\":[{\"id\":\"part\",\"label\":\"Workspace store\"}]}\n",
    )?;
    std::fs::write(
        repo.path().join("schema/collections.ts"),
        "const collection = { id: 'workspace_draft' };\n",
    )?;
    std::fs::write(
        repo.path().join("tools/cli.ts"),
        "program.command('workspace-list <name>');\n",
    )?;
    std::fs::write(
        repo.path().join("crates/cli/src/main.rs"),
        "#[derive(Parser)]\nstruct Cli { #[command(subcommand)] command: Command }\n#[derive(Subcommand)]\nenum Command { WorkspaceSwitch }\n",
    )?;
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
        repo.path().join("crates/feature/Cargo.toml"),
        "[package]\nname = \"workspace-core\"\nversion = \"0.1.0\"\n",
    )?;

    let changed = command(
        repo.path(),
        &["glossary", "names", "--base", "HEAD", "--format", "json"],
    )?;
    assert!(
        changed.status.success(),
        "{}",
        String::from_utf8_lossy(&changed.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&changed.stdout)?;
    assert_eq!(report["schema"], "rosetta.names/v1");
    assert_eq!(report["files_checked"], 1);
    assert_eq!(report["findings"].as_array().map(Vec::len), Some(1));
    assert_eq!(report["findings"][0]["name"], "workspace-core");
    assert_eq!(report["findings"][0]["line"], 2);

    let all = command(repo.path(), &["glossary", "names", "--format", "json"])?;
    assert!(
        all.status.success(),
        "{}",
        String::from_utf8_lossy(&all.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&all.stdout)?;
    let names = report["findings"]
        .as_array()
        .unwrap_or(&Vec::new())
        .iter()
        .filter_map(|item| item["name"].as_str().map(str::to_owned))
        .collect::<Vec<_>>();
    assert_eq!(
        names,
        [
            "Workspace store",
            "@scope/workspace-ui",
            "workspace-switch",
            "workspace-core",
            "workspace_draft",
            "workspace-list"
        ]
    );

    let sarif = command(
        repo.path(),
        &[
            "glossary",
            "names",
            "--files",
            "crates/feature/Cargo.toml",
            "--format",
            "sarif",
            "--strict",
        ],
    )?;
    assert_eq!(sarif.status.code(), Some(1));
    let report: serde_json::Value = serde_json::from_slice(&sarif.stdout)?;
    assert_eq!(report["runs"][0]["results"][0]["ruleId"], "term.group");
    assert_eq!(report["runs"][0]["results"][0]["level"], "error");
    Ok(())
}
