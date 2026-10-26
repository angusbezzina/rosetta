//! Finding the repo a command acts on.

use std::path::{Path, PathBuf};
use std::process::Command;

/// Default directory holding a repository's glossary and atlas.
pub const SOURCE_DIR: &str = ".rosetta";
/// A source directory that an embedding product can select explicitly.
pub const VIRGIL_DIR: &str = ".virgil";
const MARKERS: [&str; 4] = ["rosetta.toml", "repo.toml", "glossary.json", "atlas.json"];

/// Walks up from `start` to the closest repository using Rosetta's source directory.
pub fn find_root(start: &Path) -> Option<PathBuf> {
    find_root_in(start, SOURCE_DIR)
}

/// Finds the closest repository with a source marker in a selected directory.
/// The directory must be relative to the repository, so a caller cannot
/// accidentally discover a source outside the checkout.
pub fn find_root_in(start: &Path, source_dir: &str) -> Option<PathBuf> {
    let source = Path::new(source_dir);
    if source.is_absolute()
        || source.components().any(|component| {
            matches!(
                component,
                std::path::Component::ParentDir | std::path::Component::Prefix(_)
            )
        })
    {
        return None;
    }
    start.ancestors().find_map(|dir| {
        let source = dir.join(source);
        MARKERS
            .iter()
            .any(|marker| source.join(marker).is_file())
            .then(|| dir.to_path_buf())
    })
}

/// The top of the git work tree holding `start`, for a repo that has no
/// `.virgil/` yet.
pub fn git_toplevel(start: &Path) -> Option<PathBuf> {
    let output = Command::new("git")
        .arg("-C")
        .arg(start)
        .args(["rev-parse", "--show-toplevel"])
        .output()
        .ok()?;
    let path = String::from_utf8(output.stdout).ok()?;
    let path = path.trim();
    (output.status.success() && !path.is_empty()).then(|| PathBuf::from(path))
}

/// `owner/name` of the repository the `origin` remote points at.
pub fn origin_repository(root: &Path) -> Option<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["remote", "get-url", "origin"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    repository_of_url(String::from_utf8(output.stdout).ok()?.trim())
}

/// `owner/name` from a remote URL in its https, ssh or scp-like form.
pub fn repository_of_url(url: &str) -> Option<String> {
    let path = url.trim_end_matches('/');
    let path = path.strip_suffix(".git").unwrap_or(path);
    let mut segments = path.rsplit(['/', ':']);
    let name = segments.next().filter(|s| !s.is_empty())?;
    let owner = segments
        .next()
        .filter(|s| !s.is_empty() && !s.contains('@'))?;
    Some(format!("{owner}/{name}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_the_nearest_rosetta_directory_and_an_explicit_legacy_directory()
    -> Result<(), Box<dyn std::error::Error>> {
        let dir = tempfile::tempdir()?;
        std::fs::create_dir_all(dir.path().join(".rosetta"))?;
        std::fs::write(dir.path().join(".rosetta/rosetta.toml"), "")?;
        let nested = dir.path().join("a/b");
        std::fs::create_dir_all(&nested)?;
        assert_eq!(find_root(&nested), Some(dir.path().to_path_buf()));
        std::fs::create_dir_all(dir.path().join("a/.rosetta"))?;
        std::fs::write(dir.path().join("a/.rosetta/atlas.json"), "{}")?;
        assert_eq!(find_root(&nested), Some(dir.path().join("a")));
        std::fs::create_dir_all(dir.path().join("a/.virgil"))?;
        std::fs::write(dir.path().join("a/.virgil/repo.toml"), "")?;
        assert_eq!(
            find_root_in(&nested, VIRGIL_DIR),
            Some(dir.path().join("a"))
        );
        assert_eq!(find_root_in(&nested, "../elsewhere"), None);
        Ok(())
    }

    #[test]
    fn git_checkout_without_sources_is_still_discoverable() -> Result<(), Box<dyn std::error::Error>>
    {
        let dir = tempfile::tempdir()?;
        let result = Command::new("git")
            .arg("-C")
            .arg(dir.path())
            .args(["init", "-q"])
            .output()?;
        assert!(result.status.success());
        let nested = dir.path().join("a/b");
        std::fs::create_dir_all(&nested)?;
        assert_eq!(find_root(&nested), None);
        assert_eq!(
            git_toplevel(&nested),
            Some(std::fs::canonicalize(dir.path())?)
        );
        Ok(())
    }

    #[test]
    fn reads_owner_and_name_from_each_remote_form() {
        for url in [
            "https://github.com/acme/widgets.git",
            "https://github.com/acme/widgets",
            "git@github.com:acme/widgets.git",
            "ssh://git@github.com/acme/widgets.git",
        ] {
            assert_eq!(
                repository_of_url(url).as_deref(),
                Some("acme/widgets"),
                "{url}"
            );
        }
        assert_eq!(repository_of_url("git@github.com:widgets.git"), None);
        assert_eq!(repository_of_url(""), None);
    }
}
