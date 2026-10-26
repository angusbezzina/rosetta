//! When each part last changed, from one newest-first pass over `git log`:
//! the first commit that touches a path dates it and every directory above
//! it that no newer commit has dated. The pass stops as soon as every part
//! with a path has a date, or after [`MAX_COMMITS`], beyond which the rest
//! stay undated.

use std::collections::{BTreeMap, BTreeSet};
use std::io::{BufRead, BufReader};
use std::path::Path;
use std::process::{Command, Stdio};

use serde::{Deserialize, Serialize};

/// How far back the pass reads before it leaves the rest undated. A commit
/// costs about 35 µs to read (this repo's 10,000 take 0.35 s), so the bound
/// keeps a far longer history under two seconds.
pub const MAX_COMMITS: usize = 50_000;

/// Starts each commit in the log stream; the date and subject follow it.
const RECORD: u8 = 0x1e;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Updated {
    /// The commit time, RFC 3339.
    pub at: String,
    /// The commit's subject line.
    pub subject: String,
}

#[derive(Debug, thiserror::Error)]
pub enum HistoryError {
    #[error("git log: {0}")]
    Spawn(#[from] std::io::Error),
    #[error("git log failed: {0}")]
    Git(String),
}

/// The newest commit under `root` for each of `paths` (repo-relative, `.`
/// for the root itself). A checkout with no commit yet dates nothing.
pub fn last_changed(
    root: &Path,
    paths: &BTreeSet<&str>,
) -> Result<BTreeMap<String, Updated>, HistoryError> {
    last_changed_covering(root, paths, &BTreeMap::new())
}

/// As [`last_changed`], where a change below a key of `covers` also dates
/// the path it maps to: a Rust module file is dated by its submodules too.
pub fn last_changed_covering(
    root: &Path,
    paths: &BTreeSet<&str>,
    covers: &BTreeMap<&str, &str>,
) -> Result<BTreeMap<String, Updated>, HistoryError> {
    let mut found = BTreeMap::new();
    if paths.is_empty() || !has_commit(root)? {
        return Ok(found);
    }
    let mut pending: BTreeSet<&str> = paths.iter().copied().collect();
    let mut child = Command::new("git")
        .arg("-C")
        .arg(root)
        // A merge lists only the paths its own resolution changed, so a file
        // a merge brought in is dated by the commit that wrote it.
        .args([
            "log",
            "-z",
            "--name-only",
            "--relative",
            "--no-renames",
            "--diff-merges=dense-combined",
        ])
        .arg(format!("--max-count={MAX_COMMITS}"))
        // Epoch seconds, formatted here: git's own ISO form writes UTC as `Z`
        // in some versions and `+00:00` in others.
        .arg("--format=%x1e%ct%x00%s")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| HistoryError::Spawn(std::io::Error::other("git log has no stdout")))?;
    let mut reader = BufReader::new(stdout);
    let mut record = Vec::new();
    loop {
        record.clear();
        if reader.read_until(RECORD, &mut record)? == 0 {
            break;
        }
        if record.last() == Some(&RECORD) {
            record.pop();
        }
        if record.is_empty() {
            continue;
        }
        assign(&record, covers, &mut pending, &mut found);
        if pending.is_empty() {
            break;
        }
    }
    drop(reader);
    if pending.is_empty() {
        // Every part is dated: the rest of the log is not needed.
        let _ = child.kill();
        let _ = child.wait();
        return Ok(found);
    }
    let output = child.wait_with_output()?;
    if !output.status.success() {
        return Err(HistoryError::Git(
            String::from_utf8_lossy(&output.stderr).trim().to_owned(),
        ));
    }
    Ok(found)
}

fn has_commit(root: &Path) -> Result<bool, HistoryError> {
    let status = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["rev-parse", "--verify", "--quiet", "HEAD"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()?;
    Ok(status.success())
}

/// One commit's record: `<date>\0<subject>\0`, then for a commit with a
/// diff a newline and each path NUL-terminated.
/// A commit's epoch seconds as RFC 3339 in UTC, `2026-10-01T00:00:00Z`.
fn utc_rfc3339(seconds: &[u8]) -> Option<String> {
    let seconds: i64 = std::str::from_utf8(seconds).ok()?.trim().parse().ok()?;
    time::OffsetDateTime::from_unix_timestamp(seconds)
        .ok()?
        .format(&time::format_description::well_known::Rfc3339)
        .ok()
}

fn assign(
    record: &[u8],
    covers: &BTreeMap<&str, &str>,
    pending: &mut BTreeSet<&str>,
    found: &mut BTreeMap<String, Updated>,
) {
    let mut fields = record.split(|byte| *byte == 0);
    let (Some(at), Some(subject)) = (fields.next(), fields.next()) else {
        return;
    };
    let Some(at) = utc_rfc3339(at) else {
        return;
    };
    let updated = Updated {
        at,
        subject: String::from_utf8_lossy(subject).into_owned(),
    };
    for file in fields {
        let file = file.strip_prefix(b"\n").unwrap_or(file);
        if file.is_empty() {
            continue;
        }
        let file = String::from_utf8_lossy(file);
        for path in std::iter::once(file.as_ref())
            .chain(ancestors(&file))
            .chain(std::iter::once("."))
        {
            for dated in std::iter::once(path).chain(covers.get(path).copied()) {
                if let Some(dated) = pending.take(dated) {
                    found.insert(dated.to_owned(), updated.clone());
                }
            }
        }
    }
}

/// `a/b/c.rs` as `a/b`, then `a`.
fn ancestors(path: &str) -> impl Iterator<Item = &str> {
    let mut at = path;
    std::iter::from_fn(move || {
        let (parent, _) = at.rsplit_once('/')?;
        at = parent;
        Some(parent)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_file_dates_itself_and_every_directory_above_it_once() {
        let mut pending = BTreeSet::from(["a/b/c.rs", "a/b", "a", ".", "z"]);
        let mut found = BTreeMap::new();
        assign(
            b"1790899200\0newer\0\na/b/c.rs\0",
            &BTreeMap::new(),
            &mut pending,
            &mut found,
        );
        assign(
            b"1790812800\0older\0\na/x.rs\0z\0",
            &BTreeMap::new(),
            &mut pending,
            &mut found,
        );
        assert_eq!(found["a/b/c.rs"].subject, "newer");
        assert_eq!(found["a"].subject, "newer");
        assert_eq!(found["."].subject, "newer");
        assert_eq!(found["z"].subject, "older");
        assert!(pending.is_empty());
    }

    #[test]
    fn a_record_without_paths_dates_nothing() {
        let mut pending = BTreeSet::from(["a"]);
        let mut found = BTreeMap::new();
        assign(
            b"1790899200\0Merge\0",
            &BTreeMap::new(),
            &mut pending,
            &mut found,
        );
        assert!(found.is_empty());
    }

    #[test]
    fn a_change_in_a_module_directory_dates_its_module_file() {
        let mut pending = BTreeSet::from(["src/door.rs", "src/door/a.rs"]);
        let mut found = BTreeMap::new();
        let covers = BTreeMap::from([("src/door", "src/door.rs")]);
        assign(
            b"1790899200\0inside\0\nsrc/door/a.rs\0",
            &covers,
            &mut pending,
            &mut found,
        );
        assert_eq!(found["src/door.rs"].subject, "inside");
        assert_eq!(found["src/door/a.rs"].subject, "inside");
    }
}
