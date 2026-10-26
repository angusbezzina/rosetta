#[cfg(unix)]
#[test]
fn glossary_check_does_not_launch_a_tracker_found_on_path() -> Result<(), Box<dyn std::error::Error>>
{
    use std::os::unix::fs::PermissionsExt;
    use std::process::Command;

    let dir = tempfile::tempdir()?;
    let git = Command::new("git")
        .arg("init")
        .arg("-q")
        .current_dir(dir.path())
        .output()?;
    assert!(git.status.success());
    std::fs::create_dir(dir.path().join(".rosetta"))?;
    std::fs::write(
        dir.path().join(".rosetta/glossary.json"),
        r#"{
  "schema": "rosetta.glossary/v1",
  "context": "a local test",
  "purpose": "share one name",
  "sections": [{"id": "core", "title": "Core"}],
  "terms": [{
    "id": "term.workbench",
    "name": "Workbench",
    "section": "core",
    "definition": "A place to inspect work.",
    "status": "proposed",
    "epic": "ENG-12"
  }]
}"#,
    )?;
    let binary = env!("CARGO_BIN_EXE_rosetta");
    let render = Command::new(binary)
        .current_dir(dir.path())
        .args(["glossary", "render"])
        .output()?;
    assert!(
        render.status.success(),
        "{}",
        String::from_utf8_lossy(&render.stderr)
    );

    let bin = dir.path().join("bin");
    std::fs::create_dir(&bin)?;
    let tracker = bin.join("virgil");
    std::fs::write(
        &tracker,
        "#!/bin/sh\nprintf called > \"$ROSETTA_TRACKER_MARKER\"\n",
    )?;
    std::fs::set_permissions(&tracker, std::fs::Permissions::from_mode(0o755))?;
    let marker = dir.path().join("tracker-called");
    let path = std::env::join_paths(
        std::iter::once(bin.as_os_str().to_os_string()).chain(
            std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
                .map(|path| path.into_os_string()),
        ),
    )?;
    let check = Command::new(binary)
        .current_dir(dir.path())
        .env("PATH", path)
        .env("ROSETTA_TRACKER_MARKER", &marker)
        .args(["glossary", "check"])
        .output()?;
    assert!(
        check.status.success(),
        "{}",
        String::from_utf8_lossy(&check.stderr)
    );
    assert!(
        !marker.exists(),
        "a local check launched an external tracker"
    );
    Ok(())
}
