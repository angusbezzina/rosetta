use std::process::Command;

#[test]
fn config_show_reports_effective_local_config() -> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let init = Command::new("git")
        .arg("-C")
        .arg(dir.path())
        .args(["init", "-q"])
        .output()?;
    assert!(init.status.success());
    std::fs::create_dir(dir.path().join(".rosetta"))?;
    std::fs::write(
        dir.path().join(".rosetta/rosetta.toml"),
        "[refs]\nrulebook = 'whetstone/rulebook.json'\n[[name_sources]]\nglob = 'schemas/bundle.json'\npointer = '/x-virgil-collections'\n[records]\ncollections_key = 'x-virgil-collections'\nscope_key = 'x-virgil-scope'\nref_to_key = 'x-virgil-ref-to'\n[derivation]\niac = false\n",
    )?;
    let output = Command::new(env!("CARGO_BIN_EXE_rosetta"))
        .current_dir(dir.path())
        .args(["config", "show", "--json"])
        .output()?;
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let config: serde_json::Value = serde_json::from_slice(&output.stdout)?;
    assert_eq!(config["source"]["directory"], ".rosetta");
    assert_eq!(config["derivation"]["iac"], false);
    assert_eq!(config["derivation"]["drivers"], true);
    assert_eq!(config["refs"]["rulebook"], "whetstone/rulebook.json");
    assert_eq!(
        config["name_sources"][0]["pointer"],
        "/x-virgil-collections"
    );
    assert_eq!(config["records"]["collections_key"], "x-virgil-collections");
    assert_eq!(config["records"]["scope_key"], "x-virgil-scope");
    assert_eq!(config["records"]["ref_to_key"], "x-virgil-ref-to");
    Ok(())
}
