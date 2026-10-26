//! A reproducible local revision for a focused view. Git's commit alone does
//! not identify uncommitted model or code edits, so those bytes are included.

use std::fs::File;
use std::io::Read;
use std::path::Path;
use std::process::Command;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Snapshot {
    pub repository: String,
    pub head: Option<String>,
    /// None means this directory is not the root of a Git checkout.
    pub dirty: Option<bool>,
    /// The model source, glossary, assertions and config bytes, independent of code state.
    pub model_revision: String,
    /// Git HEAD, status and the bytes of every changed tracked/untracked file.
    pub fingerprint: String,
    pub coverage: SnapshotCoverage,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SnapshotCoverage {
    GitWorktree,
    ModelOnly,
}

fn git(root: &Path, args: &[&str]) -> Option<Vec<u8>> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .ok()?;
    output.status.success().then_some(output.stdout)
}

fn hash_bytes(hasher: &mut blake3::Hasher, label: &[u8], bytes: &[u8]) {
    hasher.update(&(label.len() as u64).to_le_bytes());
    hasher.update(label);
    hasher.update(&(bytes.len() as u64).to_le_bytes());
    hasher.update(bytes);
}

fn hash_file(hasher: &mut blake3::Hasher, root: &Path, relative: &[u8]) {
    let Ok(name) = std::str::from_utf8(relative) else {
        hash_bytes(hasher, relative, b"non-utf8 path");
        return;
    };
    let path = root.join(name);
    let Ok(metadata) = std::fs::symlink_metadata(&path) else {
        hash_bytes(hasher, relative, b"missing");
        return;
    };
    if metadata.file_type().is_symlink() {
        let target = std::fs::read_link(&path).ok().map_or_else(
            || "unreadable symlink".to_owned(),
            |p| p.to_string_lossy().into_owned(),
        );
        hash_bytes(hasher, relative, target.as_bytes());
    } else if metadata.is_file() {
        hash_bytes(hasher, relative, &metadata.len().to_le_bytes());
        if let Ok(mut file) = File::open(path) {
            let mut buffer = [0_u8; 64 * 1024];
            loop {
                let Ok(read) = file.read(&mut buffer) else {
                    hasher.update(b"unreadable");
                    break;
                };
                if read == 0 {
                    break;
                }
                hasher.update(&buffer[..read]);
            }
        } else {
            hasher.update(b"unreadable");
        }
    } else {
        hash_bytes(hasher, relative, b"not a file");
    }
}

/// Capture the local model and checkout. A non-Git source reports model-only
/// coverage rather than claiming that an unknown code tree is clean.
pub fn model_revision(root: &Path, source_dir: &str) -> String {
    let mut model_hasher = blake3::Hasher::new();
    for name in [
        "atlas.json",
        "glossary.json",
        "assertions.json",
        "rosetta.toml",
    ] {
        let relative = format!("{source_dir}/{name}");
        hash_file(&mut model_hasher, root, relative.as_bytes());
    }
    model_hasher.finalize().to_hex().to_string()
}

pub fn capture(root: &Path, source_dir: &str, repository: &str) -> Snapshot {
    let model_revision = model_revision(root, source_dir);
    let mut hasher = blake3::Hasher::new();
    hash_bytes(&mut hasher, b"repository", repository.as_bytes());
    hash_bytes(&mut hasher, b"model", model_revision.as_bytes());

    let same_checkout = git(root, &["rev-parse", "--show-toplevel"])
        .and_then(|bytes| String::from_utf8(bytes).ok())
        .and_then(|path| std::fs::canonicalize(path.trim()).ok())
        .zip(std::fs::canonicalize(root).ok())
        .is_some_and(|(git_root, requested)| git_root == requested);
    if !same_checkout {
        return Snapshot {
            repository: repository.to_owned(),
            head: None,
            dirty: None,
            model_revision,
            fingerprint: hasher.finalize().to_hex().to_string(),
            coverage: SnapshotCoverage::ModelOnly,
        };
    }

    let head = git(root, &["rev-parse", "HEAD"])
        .and_then(|bytes| String::from_utf8(bytes).ok())
        .map(|value| value.trim().to_owned());
    hash_bytes(
        &mut hasher,
        b"head",
        head.as_deref().unwrap_or("").as_bytes(),
    );
    let Some(status) = git(
        root,
        &["status", "--porcelain=v1", "-z", "--untracked-files=all"],
    ) else {
        return Snapshot {
            repository: repository.to_owned(),
            head,
            dirty: None,
            model_revision,
            fingerprint: hasher.finalize().to_hex().to_string(),
            coverage: SnapshotCoverage::ModelOnly,
        };
    };
    hash_bytes(&mut hasher, b"status", &status);
    let mut entries = status.split(|byte| *byte == 0);
    while let Some(entry) = entries.next() {
        if entry.len() < 4 {
            continue;
        }
        hash_file(&mut hasher, root, &entry[3..]);
        if matches!(entry[0], b'R' | b'C') || matches!(entry[1], b'R' | b'C') {
            let _ = entries.next();
        }
    }
    Snapshot {
        repository: repository.to_owned(),
        head,
        dirty: Some(!status.is_empty()),
        model_revision,
        fingerprint: hasher.finalize().to_hex().to_string(),
        coverage: SnapshotCoverage::GitWorktree,
    }
}

#[cfg(test)]
mod tests {
    use super::{SnapshotCoverage, capture};

    #[test]
    fn model_edits_change_a_model_only_snapshot() -> std::io::Result<()> {
        let dir = tempfile::tempdir()?;
        std::fs::create_dir(dir.path().join(".rosetta"))?;
        std::fs::write(dir.path().join(".rosetta/atlas.json"), "one")?;
        let before = capture(dir.path(), ".rosetta", "example/repo");
        assert_eq!(before.coverage, SnapshotCoverage::ModelOnly);
        assert_eq!(before.dirty, None);
        std::fs::write(dir.path().join(".rosetta/atlas.json"), "two")?;
        let after = capture(dir.path(), ".rosetta", "example/repo");
        assert_ne!(before.fingerprint, after.fingerprint);
        assert_ne!(before.model_revision, after.model_revision);
        Ok(())
    }

    #[test]
    fn assertion_edits_change_the_shared_model_revision() -> std::io::Result<()> {
        let dir = tempfile::tempdir()?;
        std::fs::create_dir(dir.path().join(".rosetta"))?;
        let before = capture(dir.path(), ".rosetta", "example/repo");
        std::fs::write(
            dir.path().join(".rosetta/assertions.json"),
            "{\"rules\":[]}",
        )?;
        let after = capture(dir.path(), ".rosetta", "example/repo");
        assert_ne!(before.model_revision, after.model_revision);
        assert_ne!(before.fingerprint, after.fingerprint);
        Ok(())
    }

    #[test]
    fn dirty_code_edits_change_a_git_snapshot() -> std::io::Result<()> {
        let dir = tempfile::tempdir()?;
        let git = std::process::Command::new("git")
            .arg("-C")
            .arg(dir.path())
            .args(["init", "-q"])
            .output()?;
        assert!(git.status.success());
        std::fs::create_dir(dir.path().join(".rosetta"))?;
        std::fs::write(dir.path().join(".rosetta/atlas.json"), "model")?;
        std::fs::write(dir.path().join("code.rs"), "one")?;
        let before = capture(dir.path(), ".rosetta", "example/repo");
        assert_eq!(before.coverage, SnapshotCoverage::GitWorktree);
        assert_eq!(before.dirty, Some(true));
        std::fs::write(dir.path().join("code.rs"), "two")?;
        let after = capture(dir.path(), ".rosetta", "example/repo");
        assert_eq!(before.model_revision, after.model_revision);
        assert_ne!(before.fingerprint, after.fingerprint);
        Ok(())
    }
}
