//! Shared by the integration tests: the fixture repo, copied to a temp
//! directory and committed to its own git index so the live walk sees it.

use std::path::{Path, PathBuf};
use std::process::Command;

pub type TestResult = Result<(), Box<dyn std::error::Error>>;

pub fn fixture_repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/repo")
}

pub fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

/// The repository this crate lives in.
pub fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn copy_dir(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir(&entry.path(), &target)?;
        } else {
            std::fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
}

fn git(dir: &Path, args: &[&str]) -> std::io::Result<()> {
    let status = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args([
            "-c",
            "user.name=fixture",
            "-c",
            "user.email=fixture@example.com",
        ])
        .args(args)
        .output()?;
    if status.status.success() {
        Ok(())
    } else {
        Err(std::io::Error::other(
            String::from_utf8_lossy(&status.stderr).into_owned(),
        ))
    }
}

/// A private copy of the fixture repo with every file tracked.
pub fn fixture_copy() -> std::io::Result<tempfile::TempDir> {
    fixture_copy_of(&fixture_repo())
}

/// A private copy of a fixture directory with every file tracked.
pub fn fixture_copy_of(fixture: &Path) -> std::io::Result<tempfile::TempDir> {
    let dir = tempfile::tempdir()?;
    copy_dir(fixture, dir.path())?;
    git(dir.path(), &["init", "-q"])?;
    git(dir.path(), &["add", "-A"])?;
    Ok(dir)
}

pub fn edit(path: &Path, from: &str, to: &str) -> std::io::Result<()> {
    let text = std::fs::read_to_string(path)?;
    if !text.contains(from) {
        return Err(std::io::Error::other(format!(
            "{} does not contain {from:?}",
            path.display()
        )));
    }
    std::fs::write(path, text.replacen(from, to, 1))
}
