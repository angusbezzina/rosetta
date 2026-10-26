use std::process::Command;

#[test]
fn glossary_schema_prints_without_a_repository() -> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let output = Command::new(env!("CARGO_BIN_EXE_rosetta"))
        .current_dir(dir.path())
        .args(["glossary", "schema"])
        .output()?;
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let schema: serde_json::Value = serde_json::from_slice(&output.stdout)?;
    assert_eq!(schema["title"], "Glossary");
    assert!(schema["properties"]["terms"].is_object());
    assert!(schema["properties"]["schema"].is_object());
    Ok(())
}

#[test]
fn atlas_schema_prints_without_a_repository() -> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let output = Command::new(env!("CARGO_BIN_EXE_rosetta"))
        .current_dir(dir.path())
        .args(["atlas", "schema"])
        .output()?;
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let schema: serde_json::Value = serde_json::from_slice(&output.stdout)?;
    assert_eq!(schema["title"], "Atlas");
    assert!(schema["properties"]["nodes"].is_object());
    assert!(schema["properties"]["edges"].is_object());
    Ok(())
}
