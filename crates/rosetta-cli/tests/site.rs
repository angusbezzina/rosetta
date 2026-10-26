use std::path::Path;
use std::process::{Command, Output};

fn run(root: &Path, args: &[&str]) -> Result<Output, Box<dyn std::error::Error>> {
    Ok(Command::new(env!("CARGO_BIN_EXE_rosetta"))
        .current_dir(root)
        .args(args)
        .output()?)
}

#[test]
fn site_exports_a_read_only_file_url_page_and_escapes_model_text()
-> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let git = Command::new("git")
        .arg("init")
        .arg("-q")
        .current_dir(dir.path())
        .output()?;
    assert!(git.status.success());
    std::fs::write(dir.path().join("package.json"), r#"{"name":"sample"}"#)?;
    let initialized = run(dir.path(), &["init"])?;
    assert!(
        initialized.status.success(),
        "{}",
        String::from_utf8_lossy(&initialized.stderr)
    );

    let source = dir.path().join(".rosetta/atlas.json");
    let mut atlas: serde_json::Value = serde_json::from_slice(&std::fs::read(&source)?)?;
    atlas["project"]["name"] =
        serde_json::Value::String("</script><script>alert(1)</script>".into());
    std::fs::write(&source, serde_json::to_vec_pretty(&atlas)?)?;
    let brand_path = dir.path().join("brand.json");
    std::fs::write(&brand_path, r##"{"primary":"#123456"}"##)?;
    let out = dir.path().join("site");
    let exported = run(
        dir.path(),
        &[
            "atlas",
            "site",
            "--out",
            out.to_str().ok_or("site path")?,
            "--brand",
            brand_path.to_str().ok_or("brand path")?,
        ],
    )?;
    assert!(
        exported.status.success(),
        "{}",
        String::from_utf8_lossy(&exported.stderr)
    );

    let page = std::fs::read_to_string(out.join("index.html"))?;
    assert!(page.contains("window.__ATLAS__={bundle:"));
    assert!(page.contains("staticSite:true"));
    assert!(page.contains("\\u003c/script>"));
    assert!(!page.contains("</script><script>alert(1)</script>"));
    assert!(page.contains("<script src=\"./site-viewer.js\" defer></script>"));
    assert!(!page.contains("type=\"module\""));
    assert!(!page.contains("__ROSETTA_INLINE_DATA__"));
    assert!(std::fs::metadata(out.join("site-viewer.js"))?.len() > 100_000);
    assert!(std::fs::metadata(out.join("atlas-viewer.css"))?.len() > 100);
    let bundle: serde_json::Value =
        serde_json::from_slice(&std::fs::read(out.join("bundle.json"))?)?;
    assert_eq!(
        bundle["project"]["name"],
        "</script><script>alert(1)</script>"
    );
    let brand: serde_json::Value = serde_json::from_slice(&std::fs::read(out.join("brand.json"))?)?;
    assert_eq!(brand["primary"], "#123456");
    Ok(())
}
