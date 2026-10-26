use std::path::Path;
use std::process::Command;

#[test]
fn atlas_check_json_and_strict_are_stable_and_opt_in() -> Result<(), Box<dyn std::error::Error>> {
    let fixture =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../rosetta-atlas/tests/fixtures/repo");
    let run = |args: &[&str]| -> Result<std::process::Output, std::io::Error> {
        Command::new(env!("CARGO_BIN_EXE_rosetta"))
            .current_dir(&fixture)
            .args(args)
            .output()
    };
    let normal = run(&["atlas", "check", "--json"])?;
    assert_eq!(normal.status.code(), Some(1));
    let normal: serde_json::Value = serde_json::from_slice(&normal.stdout)?;
    assert_eq!(normal["schema"], "rosetta.atlas-check/v1");
    assert_eq!(normal["strict"], false);
    assert_eq!(normal["passed"], false);
    let normal_findings = normal["findings"]
        .as_array()
        .ok_or("normal findings array")?;
    assert!(
        normal_findings
            .iter()
            .any(|item| item["path"] == "nodes[infra].derived")
    );
    assert!(
        !normal_findings
            .iter()
            .any(|item| item["path"] == "nodes[web].refs.term")
    );

    let strict = run(&["atlas", "check", "--strict", "--json"])?;
    assert_eq!(
        strict.status.code(),
        Some(1),
        "{}",
        String::from_utf8_lossy(&strict.stderr)
    );
    let strict: serde_json::Value = serde_json::from_slice(&strict.stdout)?;
    assert_eq!(strict["schema"], "rosetta.atlas-check/v1");
    assert_eq!(strict["strict"], true);
    assert_eq!(strict["passed"], false);
    let findings = strict["findings"].as_array().ok_or("findings array")?;
    assert_eq!(findings.len(), normal_findings.len() + 2);
    assert!(findings.iter().any(|item| {
        item["path"] == "nodes[web].refs.term"
            && item["message"] == "a named part needs a glossary term"
    }));
    Ok(())
}
