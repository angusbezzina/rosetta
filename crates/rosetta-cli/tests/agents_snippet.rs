use std::process::Command;

#[test]
fn agents_snippet_is_short_and_works_without_a_repository() -> Result<(), Box<dyn std::error::Error>>
{
    let dir = tempfile::tempdir()?;
    let output = Command::new(env!("CARGO_BIN_EXE_rosetta"))
        .current_dir(dir.path())
        .arg("agents-snippet")
        .output()?;
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let snippet = String::from_utf8(output.stdout)?;
    assert_eq!(snippet.lines().count(), 3);
    assert!(snippet.ends_with('\n'));
    assert!(snippet.contains("skills/rosetta/SKILL.md"));
    assert!(snippet.contains("rosetta glossary context"));
    assert!(snippet.contains("rosetta atlas context"));
    assert!(snippet.contains(".rosetta/glossary.json"));
    assert!(snippet.contains(".rosetta/atlas.json"));
    Ok(())
}
