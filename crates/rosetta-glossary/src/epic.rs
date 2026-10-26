//! A proposed term can be compared with its epic by an explicit tracker
//! adapter. The local glossary check does not launch a tracker by default.

use std::ffi::OsString;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde::Deserialize;

use super::model::{Glossary, Problem, TermStatus};
use super::render::GLOSSARY_SOURCE;

/// How long the check waits on one epic before it gives up on the warning.
pub const EPIC_LOOKUP_TIMEOUT: Duration = Duration::from_secs(2);

/// Answers whether an epic is closed; `None` when it cannot tell.
pub trait EpicStatus {
    fn is_closed(&self, epic: &str) -> Option<bool>;
}

/// Offline default: no external tracker was explicitly requested.
pub struct NoEpicStatus;

impl EpicStatus for NoEpicStatus {
    fn is_closed(&self, _epic: &str) -> Option<bool> {
        None
    }
}

/// Legacy opt-in adapter for `virgil tasks show <epic> --json`.
pub struct VirgilTasksOnPath {
    virgil: Option<PathBuf>,
    timeout: Duration,
}

/// The parts of `virgil tasks show --json` the warning reads.
#[derive(Debug, Deserialize)]
struct ShowEnvelope {
    result: ShowResult,
}

#[derive(Debug, Deserialize)]
struct ShowResult {
    task: ShownTask,
}

#[derive(Debug, Deserialize)]
struct ShownTask {
    state: String,
    #[serde(default)]
    close_as: String,
}

impl VirgilTasksOnPath {
    pub fn new(path: Option<OsString>) -> Self {
        Self::with_timeout(path, EPIC_LOOKUP_TIMEOUT)
    }

    pub fn with_timeout(path: Option<OsString>, timeout: Duration) -> Self {
        let virgil = path.and_then(|path| {
            std::env::split_paths(&path)
                .map(|dir| dir.join("virgil"))
                .find(|candidate| candidate.is_file())
        });
        Self { virgil, timeout }
    }
}

impl EpicStatus for VirgilTasksOnPath {
    fn is_closed(&self, epic: &str) -> Option<bool> {
        let mut child = Command::new(self.virgil.as_ref()?)
            .args(["tasks", "show", epic, "--json"])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .ok()?;
        let started = Instant::now();
        loop {
            match child.try_wait().ok()? {
                Some(status) if status.success() => break,
                Some(_) => return None,
                None if started.elapsed() >= self.timeout => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return None;
                }
                None => std::thread::sleep(Duration::from_millis(20)),
            }
        }
        let output = child.wait_with_output().ok()?;
        let shown: ShowEnvelope = serde_json::from_slice(&output.stdout).ok()?;
        let task = shown.result.task;
        Some(task.state == "closed" || task.state == "done" || !task.close_as.is_empty())
    }
}

pub fn closed_epic_warnings(glossary: &Glossary, status: &dyn EpicStatus) -> Vec<Problem> {
    closed_epic_warnings_for(glossary, GLOSSARY_SOURCE, status)
}

pub fn closed_epic_warnings_for(
    glossary: &Glossary,
    source: &str,
    status: &dyn EpicStatus,
) -> Vec<Problem> {
    glossary
        .terms
        .iter()
        .enumerate()
        .filter(|(_, term)| term.status == TermStatus::Proposed)
        .filter_map(|(index, term)| {
            let epic = term.epic.as_deref()?;
            status.is_closed(epic)?.then(|| {
                Problem::new(
                    format!("{source}#terms[{index}].epic"),
                    format!(
                        "{} is proposed but its epic {epic} is closed; the PR that lands the epic sets the term settled",
                        term.name
                    ),
                )
            })
        })
        .collect()
}

/// Whether `text` names an epic the way `virgil tasks show` resolves one: a
/// native reference (the epic prefix and a ULID) or an imported key (a team
/// prefix, a dash, then letters and digits).
pub fn is_epic_ref(text: &str) -> bool {
    if let Some(ulid) = text.strip_prefix("epic:") {
        return ulid.len() == 26
            && ulid
                .bytes()
                .all(|b| b.is_ascii_digit() || (b.is_ascii_uppercase() && !b"ILOU".contains(&b)));
    }
    let Some((prefix, key)) = text.split_once('-') else {
        return false;
    };
    let mut parts = key.split('.');
    let head_ok = parts.next().is_some_and(|head| {
        !head.is_empty()
            && head
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
    });
    !prefix.is_empty()
        && prefix
            .bytes()
            .next()
            .is_some_and(|b| b.is_ascii_uppercase())
        && prefix
            .bytes()
            .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit())
        && head_ok
        && parts.all(|part| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit()))
}

#[cfg(test)]
mod tests {
    use super::is_epic_ref;

    #[test]
    fn epic_refs() {
        assert!(is_epic_ref("epic:01ARZ3NDEKTSV4RRFFQ69G5FAA"));
        assert!(is_epic_ref("ABC-1a2b3"));
        assert!(is_epic_ref("ABC-1a2b3.1"));
        assert!(is_epic_ref("ENG-12"));
        assert!(!is_epic_ref("epic:01ARZ3NDEKTSV4RRFFQ69G5FA"));
        assert!(!is_epic_ref("epic:01ARZ3NDEKTSV4RRFFQ69G5FAI"));
        assert!(!is_epic_ref("task:01ARZ3NDEKTSV4RRFFQ69G5FAA"));
        assert!(!is_epic_ref("abc-1a2b3"));
        assert!(!is_epic_ref("ABC-"));
        assert!(!is_epic_ref("ABC-a..1"));
        assert!(!is_epic_ref("an epic"));
    }
}
