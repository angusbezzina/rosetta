//! The files and directories below each workspace member, read live from the
//! git index whenever a bundle is built. They change on every PR, so they are
//! never committed to the model.

use std::collections::BTreeMap;
use std::path::Path;
use std::process::Command;

use super::model::{Family, NodeKind};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiveNode {
    /// `d:<path>` for a directory, `f:<path>` for a file.
    pub id: String,
    pub label: String,
    pub kind: NodeKind,
    pub family: Family,
    pub parent: String,
    pub path: String,
    /// Lines in a text file; `None` for a directory or a binary file.
    pub lines: Option<u64>,
    /// What a file says it does, in its own first sentence.
    pub summary: Option<String>,
    /// For a Rust module file (`door.rs` beside `door/`), the directory its
    /// submodules live in, which it stands for: a change there changes it.
    pub module_dir: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum WalkError {
    #[error("git ls-files: {0}")]
    Spawn(#[from] std::io::Error),
    #[error("git ls-files failed: {0}")]
    Git(String),
}

/// A member the walk descends into: its node id, path and family.
#[derive(Debug, Clone)]
pub struct WalkRoot {
    pub id: String,
    pub path: String,
    pub family: Family,
}

pub fn tracked_files(root: &Path) -> Result<Vec<String>, WalkError> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["ls-files", "-z", "--full-name", "--", "."])
        .output()?;
    if !output.status.success() {
        return Err(WalkError::Git(
            String::from_utf8_lossy(&output.stderr).trim().to_owned(),
        ));
    }
    // --full-name is relative to the git top level, which may sit above root.
    let prefix = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["rev-parse", "--show-prefix"])
        .output()?;
    let prefix = String::from_utf8_lossy(&prefix.stdout).trim().to_owned();
    let mut files: Vec<String> = output
        .stdout
        .split(|byte| *byte == 0)
        .filter(|entry| !entry.is_empty())
        .filter_map(|entry| {
            let path = String::from_utf8_lossy(entry).into_owned();
            path.strip_prefix(&prefix).map(str::to_owned)
        })
        .collect();
    files.sort();
    Ok(files)
}

fn count_lines(bytes: &[u8]) -> Option<u64> {
    if bytes.contains(&0) {
        return None;
    }
    let newlines = bytes.iter().filter(|byte| **byte == b'\n').count() as u64;
    Some(newlines + u64::from(!bytes.is_empty() && !bytes.ends_with(b"\n")))
}

fn name_of(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

/// Directory and file nodes below each member. A file inside a member nested
/// in another belongs to the nested one.
pub fn walk(root: &Path, members: &[WalkRoot], files: &[String]) -> Vec<LiveNode> {
    let mut nodes: BTreeMap<String, LiveNode> = BTreeMap::new();
    for file in files {
        // The deepest member whose directory holds the file.
        let Some(member) = members
            .iter()
            .filter(|m| m.path.is_empty() || file.starts_with(&format!("{}/", m.path)))
            .max_by_key(|m| m.path.len())
        else {
            continue;
        };
        let relative = if member.path.is_empty() {
            file.as_str()
        } else {
            &file[member.path.len() + 1..]
        };
        let mut parent = member.id.clone();
        let mut dir = member.path.clone();
        let mut segments: Vec<&str> = relative.split('/').collect();
        let Some(file_name) = segments.pop() else {
            continue;
        };
        for segment in segments {
            dir = if dir.is_empty() {
                segment.to_owned()
            } else {
                format!("{dir}/{segment}")
            };
            let id = format!("d:{dir}");
            nodes.entry(id.clone()).or_insert_with(|| LiveNode {
                id: id.clone(),
                label: segment.to_owned(),
                kind: NodeKind::Directory,
                family: member.family,
                parent: parent.clone(),
                path: dir.clone(),
                lines: None,
                summary: None,
                module_dir: None,
            });
            parent = id;
        }
        let bytes = std::fs::read(root.join(file)).ok();
        let lines = bytes.as_deref().and_then(count_lines);
        let summary = bytes
            .as_deref()
            .filter(|_| lines.is_some())
            .and_then(|bytes| super::blurb::of_file(file, &String::from_utf8_lossy(bytes)));
        let id = format!("f:{file}");
        nodes.insert(
            id.clone(),
            LiveNode {
                id,
                label: name_of(file_name).to_owned(),
                kind: NodeKind::File,
                family: member.family,
                parent,
                path: file.clone(),
                lines,
                summary,
                module_dir: None,
            },
        );
    }
    let crates: Vec<&WalkRoot> = members
        .iter()
        .filter(|member| root.join(&member.path).join("Cargo.toml").is_file())
        .collect();
    fold_modules(&mut nodes, &crates);
    nodes.into_values().collect()
}

/// In a crate, `door.rs` and `door/` are one module (the 2018 layout): the
/// directory's entries become the file's children and the directory is not
/// listed beside it. Elsewhere a file and a directory of one stem are two
/// things.
fn fold_modules(nodes: &mut BTreeMap<String, LiveNode>, crates: &[&WalkRoot]) {
    let in_a_crate = |path: &str| {
        crates
            .iter()
            .any(|member| member.path.is_empty() || path.starts_with(&format!("{}/", member.path)))
    };
    let modules: Vec<(String, String)> = nodes
        .values()
        .filter(|node| node.kind == NodeKind::Directory && in_a_crate(&node.path))
        .filter_map(|dir| {
            let file = format!("f:{}.rs", dir.path);
            nodes.contains_key(&file).then(|| (dir.id.clone(), file))
        })
        .collect();
    for (dir_id, file_id) in modules {
        let Some(dir) = nodes.remove(&dir_id) else {
            continue;
        };
        for node in nodes.values_mut().filter(|node| node.parent == dir_id) {
            node.parent = file_id.clone();
        }
        if let Some(file) = nodes.get_mut(&file_id) {
            file.module_dir = Some(dir.path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    type TestResult = Result<(), Box<dyn std::error::Error>>;

    fn tree(files: &[(&str, &str)]) -> std::io::Result<tempfile::TempDir> {
        let dir = tempfile::tempdir()?;
        for (path, text) in files {
            let full = dir.path().join(path);
            if let Some(parent) = full.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(full, text)?;
        }
        Ok(dir)
    }

    fn walked(dir: &Path, member: &str, files: &[&str]) -> BTreeMap<String, LiveNode> {
        let roots = [WalkRoot {
            id: "member".to_owned(),
            path: member.to_owned(),
            family: Family::Service,
        }];
        let files: Vec<String> = files.iter().map(|f| (*f).to_owned()).collect();
        walk(dir, &roots, &files)
            .into_iter()
            .map(|node| (node.id.clone(), node))
            .collect()
    }

    #[test]
    fn a_rust_module_file_holds_its_directory() -> TestResult {
        let files = [
            "c/Cargo.toml",
            "c/src/lib.rs",
            "c/src/door.rs",
            "c/src/door/a.rs",
        ];
        let dir = tree(&[
            ("c/Cargo.toml", "[package]\n"),
            ("c/src/lib.rs", "mod door;\n"),
            ("c/src/door.rs", "mod a;\npub fn x() {}\n"),
            ("c/src/door/a.rs", "\n"),
        ])?;
        let nodes = walked(dir.path(), "c", &files);
        assert!(!nodes.contains_key("d:c/src/door"));
        let door = nodes.get("f:c/src/door.rs").ok_or("door.rs")?;
        assert_eq!(door.parent, "d:c/src");
        assert_eq!(door.lines, Some(2));
        assert_eq!(door.module_dir.as_deref(), Some("c/src/door"));
        let a = nodes.get("f:c/src/door/a.rs").ok_or("a.rs")?;
        assert_eq!(a.parent, "f:c/src/door.rs");
        Ok(())
    }

    #[test]
    fn outside_a_crate_a_file_and_a_directory_stay_two_things() -> TestResult {
        let files = ["p/package.json", "p/src/door.rs", "p/src/door/a.rs"];
        let dir = tree(&[
            ("p/package.json", "{}\n"),
            ("p/src/door.rs", "\n"),
            ("p/src/door/a.rs", "\n"),
        ])?;
        let nodes = walked(dir.path(), "p", &files);
        let a = nodes.get("f:p/src/door/a.rs").ok_or("a.rs")?;
        assert_eq!(a.parent, "d:p/src/door");
        let door = nodes.get("f:p/src/door.rs").ok_or("door.rs")?;
        assert_eq!(door.module_dir, None);
        Ok(())
    }

    #[test]
    fn counts_lines_like_wc_plus_an_unterminated_last_line() {
        assert_eq!(count_lines(b""), Some(0));
        assert_eq!(count_lines(b"a\nb\n"), Some(2));
        assert_eq!(count_lines(b"a\nb"), Some(2));
        assert_eq!(count_lines(b"\x00png"), None);
    }
}
