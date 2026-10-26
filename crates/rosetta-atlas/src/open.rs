//! A deliberately small loopback-only server for the bundled local viewer.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Component, Path};
use std::time::Duration;

use serde::Deserialize;

use crate::atlas::change;
use crate::{atlas, curation, json_text};

const INDEX: &[u8] = include_bytes!("../viewer-dist/index.html");
const VIEWER_JS: &[u8] = include_bytes!("../viewer-dist/atlas-viewer.js");
const VIEWER_CSS: &[u8] = include_bytes!("../viewer-dist/atlas-viewer.css");
const PELE_JS: &[u8] = include_bytes!("../viewer-dist/pele.js");

#[derive(Debug, thiserror::Error)]
pub enum OpenError {
    #[error("local viewer: {0}")]
    Io(#[from] std::io::Error),
    #[error("{path}: {source}")]
    BrandRead {
        path: String,
        source: std::io::Error,
    },
    #[error(
        "{path}: brand must be JSON with optional primary, secondary and tertiary #RRGGBB colours: {why}"
    )]
    Brand { path: String, why: String },
}

#[derive(Debug, Deserialize, serde::Serialize, Default)]
#[serde(deny_unknown_fields)]
pub struct Brand {
    #[serde(skip_serializing_if = "Option::is_none")]
    primary: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    secondary: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tertiary: Option<String>,
}

impl Brand {
    /// A colour file changes only the three family colours. The viewer fills
    /// absent entries with neutral defaults; arbitrary CSS is never served.
    pub fn from_file(path: &Path) -> Result<Self, OpenError> {
        let label = path.display().to_string();
        let bytes = std::fs::read(path).map_err(|source| OpenError::BrandRead {
            path: label.clone(),
            source,
        })?;
        if bytes.len() > 16 * 1024 {
            return Err(OpenError::Brand {
                path: label,
                why: "file exceeds 16 KiB".to_owned(),
            });
        }
        let brand: Self = serde_json::from_slice(&bytes).map_err(|error| OpenError::Brand {
            path: label.clone(),
            why: error.to_string(),
        })?;
        for value in [
            brand.primary.as_deref(),
            brand.secondary.as_deref(),
            brand.tertiary.as_deref(),
        ]
        .into_iter()
        .flatten()
        {
            if value.len() != 7
                || !value.starts_with('#')
                || !value[1..].bytes().all(|byte| byte.is_ascii_hexdigit())
            {
                return Err(OpenError::Brand {
                    path: label,
                    why: format!("{value:?} is not #RRGGBB"),
                });
            }
        }
        Ok(brand)
    }
}

pub fn serve(
    root: &Path,
    source_dir: &str,
    brand: &Brand,
    port: u16,
    view: Option<&str>,
    out: &mut dyn Write,
) -> Result<(), OpenError> {
    let listener = TcpListener::bind(("127.0.0.1", port))?;
    let address = listener.local_addr()?;
    writeln!(
        out,
        "http://{address}/{}",
        view.map_or(String::new(), |reference| format!("?view={reference}"))
    )?;
    out.flush()?;
    let mut secret = [0_u8; 32];
    getrandom::fill(&mut secret)
        .map_err(|error| OpenError::Io(std::io::Error::other(error.to_string())))?;
    let mut session = Session {
        token: secret.iter().map(|byte| format!("{byte:02x}")).collect(),
        port: address.port(),
        undo: None,
    };
    for incoming in listener.incoming() {
        let mut stream = incoming?;
        stream.set_read_timeout(Some(Duration::from_secs(3)))?;
        // Large repositories can produce multi-megabyte bundles. A browser
        // may pause its read while parsing and rendering another response.
        stream.set_write_timeout(Some(Duration::from_secs(30)))?;
        let _ = respond(&mut stream, root, source_dir, brand, &mut session);
    }
    Ok(())
}

struct Session {
    token: String,
    port: u16,
    undo: Option<curation::UndoRecord>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EvidenceRequest {
    path: String,
    fingerprint: String,
}

fn evidence_snapshot_matches(
    root: &Path,
    source_dir: &str,
    expected: &str,
) -> Result<(), &'static str> {
    let model = atlas::load_atlas_from(root, source_dir)
        .map_err(|_| "the Atlas model cannot be read; refresh the view")?;
    let current = rosetta_core::snapshot::capture(root, source_dir, &model.project.repo);
    if current.coverage != rosetta_core::snapshot::SnapshotCoverage::GitWorktree {
        return Err("source snapshot cannot be verified outside a Git worktree");
    }
    if current.fingerprint != expected {
        return Err(
            "source changed since this view was made; refresh the view before opening evidence",
        );
    }
    Ok(())
}

fn evidence_file(root: &Path, requested: &str) -> Result<serde_json::Value, &'static str> {
    let relative = requested.split('#').next().unwrap_or_default();
    if relative.is_empty()
        || !Path::new(relative)
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
    {
        return Err("evidence path must be repository-relative");
    }
    let repo = root
        .canonicalize()
        .map_err(|_| "repository cannot be read")?;
    let file = root
        .join(relative)
        .canonicalize()
        .map_err(|_| "evidence file does not exist")?;
    if !file.starts_with(repo) || !file.is_file() {
        return Err("evidence file is outside the repository or is not a file");
    }
    const LIMIT: u64 = 128 * 1024;
    let mut bytes = Vec::new();
    std::fs::File::open(&file)
        .map_err(|_| "evidence file cannot be read")?
        .take(LIMIT + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "evidence file cannot be read")?;
    let truncated = bytes.len() as u64 > LIMIT;
    bytes.truncate(LIMIT as usize);
    while !bytes.is_empty()
        && std::str::from_utf8(&bytes).is_err_and(|error| error.error_len().is_none())
    {
        bytes.pop();
    }
    let content = std::str::from_utf8(&bytes).map_err(|_| "evidence is not UTF-8 text")?;
    Ok(serde_json::json!({"path": relative, "content": content, "truncated": truncated}))
}

fn respond(
    stream: &mut TcpStream,
    root: &Path,
    source_dir: &str,
    brand: &Brand,
    session: &mut Session,
) -> std::io::Result<()> {
    let mut request = [0_u8; 8192];
    let mut len = 0;
    let header_end = loop {
        if let Some(at) = request[..len]
            .windows(4)
            .position(|window| window == b"\r\n\r\n")
        {
            break at + 4;
        }
        if len == request.len() {
            return send(
                stream,
                400,
                "text/plain; charset=utf-8",
                b"headers too large\n",
            );
        }
        let received = stream.read(&mut request[len..])?;
        if received == 0 {
            return Ok(());
        }
        len += received;
    };
    let head = String::from_utf8_lossy(&request[..header_end]);
    let first = head.lines().next().unwrap_or_default();
    let mut parts = first.split_whitespace();
    let (method, path) = (
        parts.next().unwrap_or_default(),
        parts.next().unwrap_or_default(),
    );
    if method != "GET" && method != "POST" {
        return send(
            stream,
            405,
            "text/plain; charset=utf-8",
            b"GET or POST only\n",
        );
    }
    let (path, query) = path.split_once('?').unwrap_or((path, ""));
    let host_is_local = head.lines().any(|line| {
        line.get(..5)
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case("host:"))
            && matches!(
                line[5..].trim().split(':').next(),
                Some("127.0.0.1" | "localhost")
            )
    });
    if !host_is_local {
        return send(
            stream,
            403,
            "text/plain; charset=utf-8",
            b"local host only\n",
        );
    }
    if method == "POST" {
        if !path.starts_with("/curation/") && path != "/change/review" && path != "/evidence/read" {
            return send(stream, 405, "text/plain; charset=utf-8", b"GET only\n");
        }
        if !header(&head, "content-type").is_some_and(|kind| {
            kind.split(';')
                .next()
                .is_some_and(|kind| kind.trim() == "application/json")
        }) {
            return send(
                stream,
                400,
                "text/plain; charset=utf-8",
                b"JSON content type required\n",
            );
        }
        let Some(size) =
            header(&head, "content-length").and_then(|value| value.parse::<usize>().ok())
        else {
            return send(
                stream,
                400,
                "text/plain; charset=utf-8",
                b"content length required\n",
            );
        };
        if size > 64 * 1024 || len - header_end > size {
            return send(
                stream,
                400,
                "text/plain; charset=utf-8",
                b"proposal too large\n",
            );
        }
        let mut body = request[header_end..len].to_vec();
        body.resize(size, 0);
        stream.read_exact(&mut body[len - header_end..])?;
        let origin = header(&head, "origin");
        let allowed = [
            format!("http://127.0.0.1:{}", session.port),
            format!("http://localhost:{}", session.port),
        ];
        if !origin.is_some_and(|origin| allowed.iter().any(|candidate| candidate == origin))
            || header(&head, "x-rosetta-session") != Some(session.token.as_str())
        {
            return send(
                stream,
                403,
                "text/plain; charset=utf-8",
                b"local session required\n",
            );
        }
        return mutate(stream, root, source_dir, path, &body, session);
    }
    match path {
        "/" | "/index.html" => send(stream, 200, "text/html; charset=utf-8", INDEX),
        "/atlas-viewer.js" => send(stream, 200, "text/javascript; charset=utf-8", VIEWER_JS),
        "/atlas-viewer.css" => send(stream, 200, "text/css; charset=utf-8", VIEWER_CSS),
        "/pele.js" => send(stream, 200, "text/javascript; charset=utf-8", PELE_JS),
        "/brand.json" => {
            let body = serde_json::to_vec(brand).map_err(std::io::Error::other)?;
            send(stream, 200, "application/json; charset=utf-8", &body)
        }
        "/session.json" => {
            let body = serde_json::json!({"local": true, "token": session.token, "can_undo": session.undo.is_some()}).to_string();
            send(
                stream,
                200,
                "application/json; charset=utf-8",
                body.as_bytes(),
            )
        }
        "/bundle.json" => match atlas::bundle_repo_from(root, source_dir).and_then(|bundle| {
            json_text::to_pretty_string(&bundle).map_err(atlas::AtlasLoadError::Format)
        }) {
            Ok(body) => send(
                stream,
                200,
                "application/json; charset=utf-8",
                body.as_bytes(),
            ),
            Err(error) => send(
                stream,
                500,
                "text/plain; charset=utf-8",
                error.to_string().as_bytes(),
            ),
        },
        "/focus.json" => {
            let reference = query_value(query, "view");
            let capability = query_value(query, "capability");
            let root_id = query_value(query, "root");
            let level = query_value(query, "level").unwrap_or_else(|| "capability".to_owned());
            let refresh = query.split('&').any(|part| part == "refresh=1");
            match atlas::bundle_repo_from(root, source_dir) {
                Ok(bundle) => {
                    let selected = if let Some(reference) = reference.as_deref() {
                        if refresh {
                            atlas::focus::refresh(&bundle, reference)
                        } else {
                            atlas::focus::reopen(&bundle, reference)
                        }
                    } else if let Some(capability) = capability {
                        let level = match level.as_str() {
                            "capability" => atlas::focus::Level::Capability,
                            "component" => atlas::focus::Level::Component,
                            "implementation" => atlas::focus::Level::Implementation,
                            _ => {
                                return send(
                                    stream,
                                    400,
                                    "text/plain; charset=utf-8",
                                    b"unknown level\n",
                                );
                            }
                        };
                        atlas::focus::select(
                            &bundle,
                            atlas::focus::Query {
                                capability: Some(capability),
                                roots: root_id.into_iter().collect(),
                                level,
                                ..atlas::focus::Query::default()
                            },
                        )
                    } else {
                        return send(
                            stream,
                            400,
                            "text/plain; charset=utf-8",
                            b"view reference or capability required\n",
                        );
                    };
                    match selected {
                        Ok(view) => match json_text::to_pretty_string(&view) {
                            Ok(body) => send(
                                stream,
                                200,
                                "application/json; charset=utf-8",
                                body.as_bytes(),
                            ),
                            Err(error) => send(
                                stream,
                                500,
                                "text/plain; charset=utf-8",
                                error.to_string().as_bytes(),
                            ),
                        },
                        Err(error @ atlas::focus::FocusError::Stale { .. }) => send(
                            stream,
                            409,
                            "text/plain; charset=utf-8",
                            error.to_string().as_bytes(),
                        ),
                        Err(error) => send(
                            stream,
                            400,
                            "text/plain; charset=utf-8",
                            error.to_string().as_bytes(),
                        ),
                    }
                }
                Err(error) => send(
                    stream,
                    500,
                    "text/plain; charset=utf-8",
                    error.to_string().as_bytes(),
                ),
            }
        }
        _ => send(stream, 404, "text/plain; charset=utf-8", b"not found\n"),
    }
}

fn header<'a>(head: &'a str, key: &str) -> Option<&'a str> {
    head.lines().skip(1).find_map(|line| {
        let (name, value) = line.split_once(':')?;
        name.eq_ignore_ascii_case(key).then_some(value.trim())
    })
}

fn mutate(
    stream: &mut TcpStream,
    root: &Path,
    source_dir: &str,
    path: &str,
    body: &[u8],
    session: &mut Session,
) -> std::io::Result<()> {
    if path == "/evidence/read" {
        let requested: EvidenceRequest = match serde_json::from_slice(body) {
            Ok(requested) => requested,
            Err(error) => {
                return send(
                    stream,
                    400,
                    "text/plain; charset=utf-8",
                    error.to_string().as_bytes(),
                );
            }
        };
        if let Err(why) = evidence_snapshot_matches(root, source_dir, &requested.fingerprint) {
            return send(stream, 409, "text/plain; charset=utf-8", why.as_bytes());
        }
        return match evidence_file(root, &requested.path) {
            Ok(_)
                if evidence_snapshot_matches(root, source_dir, &requested.fingerprint).is_err() =>
            {
                send(
                    stream,
                    409,
                    "text/plain; charset=utf-8",
                    b"source changed while reading evidence; refresh the view",
                )
            }
            Ok(evidence) => send(
                stream,
                200,
                "application/json; charset=utf-8",
                evidence.to_string().as_bytes(),
            ),
            Err(why) => send(stream, 400, "text/plain; charset=utf-8", why.as_bytes()),
        };
    }
    if path == "/change/review" {
        let proposal: change::IntentProposal = match serde_json::from_slice(body) {
            Ok(proposal) => proposal,
            Err(error) => {
                return send(
                    stream,
                    400,
                    "text/plain; charset=utf-8",
                    error.to_string().as_bytes(),
                );
            }
        };
        return match change::changed(root, source_dir, &proposal.base_commit, Some(&proposal)) {
            Ok(report) => match json_text::to_pretty_string(&report) {
                Ok(body) => send(
                    stream,
                    200,
                    "application/json; charset=utf-8",
                    body.as_bytes(),
                ),
                Err(error) => send(
                    stream,
                    500,
                    "text/plain; charset=utf-8",
                    error.to_string().as_bytes(),
                ),
            },
            Err(
                error @ (change::ChangeError::StaleBaseline { .. }
                | change::ChangeError::StaleModel { .. }),
            ) => send(
                stream,
                409,
                "text/plain; charset=utf-8",
                error.to_string().as_bytes(),
            ),
            Err(
                error @ (change::ChangeError::ProposalSchema | change::ChangeError::NoBaseline(_)),
            ) => send(
                stream,
                400,
                "text/plain; charset=utf-8",
                error.to_string().as_bytes(),
            ),
            Err(error) => send(
                stream,
                500,
                "text/plain; charset=utf-8",
                error.to_string().as_bytes(),
            ),
        };
    }
    if path == "/curation/undo" {
        let Some(record) = session.undo.take() else {
            return send(
                stream,
                400,
                "text/plain; charset=utf-8",
                b"nothing to undo\n",
            );
        };
        return match curation::undo(root, record) {
            Ok(()) => send(
                stream,
                200,
                "application/json; charset=utf-8",
                b"{\"undone\":true}",
            ),
            Err((error, record)) => {
                session.undo = Some(*record);
                send(
                    stream,
                    409,
                    "text/plain; charset=utf-8",
                    error.to_string().as_bytes(),
                )
            }
        };
    }
    if path != "/curation/preview" && path != "/curation/apply" {
        return send(stream, 404, "text/plain; charset=utf-8", b"not found\n");
    }
    let proposal: curation::Proposal = match serde_json::from_slice(body) {
        Ok(proposal) => proposal,
        Err(error) => {
            return send(
                stream,
                400,
                "text/plain; charset=utf-8",
                error.to_string().as_bytes(),
            );
        }
    };
    let result = if path == "/curation/preview" {
        curation::preview(root, source_dir, &proposal).map(|preview| (preview, None))
    } else {
        curation::apply(root, source_dir, &proposal)
            .map(|(preview, record)| (preview, Some(record)))
    };
    match result {
        Ok((preview, record)) => {
            if let Some(record) = record {
                session.undo = Some(record);
            }
            match json_text::to_pretty_string(&preview) {
                Ok(body) => send(
                    stream,
                    200,
                    "application/json; charset=utf-8",
                    body.as_bytes(),
                ),
                Err(error) => send(
                    stream,
                    500,
                    "text/plain; charset=utf-8",
                    error.to_string().as_bytes(),
                ),
            }
        }
        Err(error @ curation::CurationError::Conflict { .. }) => send(
            stream,
            409,
            "text/plain; charset=utf-8",
            error.to_string().as_bytes(),
        ),
        Err(error @ (curation::CurationError::Invalid(_) | curation::CurationError::Schema)) => {
            send(
                stream,
                400,
                "text/plain; charset=utf-8",
                error.to_string().as_bytes(),
            )
        }
        Err(error) => send(
            stream,
            500,
            "text/plain; charset=utf-8",
            error.to_string().as_bytes(),
        ),
    }
}

fn query_value(query: &str, key: &str) -> Option<String> {
    let raw = query.split('&').find_map(|part| {
        part.split_once('=')
            .and_then(|(name, value)| (name == key).then_some(value))
    })?;
    if raw.len() > 8192 {
        return None;
    }
    let mut bytes = Vec::with_capacity(raw.len());
    let mut input = raw.bytes();
    while let Some(byte) = input.next() {
        match byte {
            b'%' => {
                let first = (input.next()? as char).to_digit(16)?;
                let second = (input.next()? as char).to_digit(16)?;
                bytes.push((first * 16 + second) as u8);
            }
            b'+' => bytes.push(b' '),
            other => bytes.push(other),
        }
    }
    String::from_utf8(bytes).ok()
}

fn send(
    stream: &mut TcpStream,
    status: u16,
    content_type: &str,
    body: &[u8],
) -> std::io::Result<()> {
    let reason = match status {
        200 => "OK",
        400 => "Bad Request",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        409 => "Conflict",
        _ => "Internal Server Error",
    };
    write!(
        stream,
        "HTTP/1.1 {status} {reason}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\nX-Frame-Options: DENY\r\nContent-Security-Policy: default-src 'none'; script-src 'self' 'unsafe-inline'; style-src 'self' 'unsafe-inline'; connect-src 'self'; img-src 'self' data:; base-uri 'none'; frame-ancestors 'none'\r\nConnection: close\r\n\r\n",
        body.len()
    )?;
    stream.write_all(body)
}

#[cfg(test)]
mod tests {
    use super::{Brand, evidence_file, evidence_snapshot_matches};

    #[test]
    fn evidence_requires_the_viewed_git_snapshot() -> Result<(), Box<dyn std::error::Error>> {
        let repo = tempfile::tempdir()?;
        let git = std::process::Command::new("git")
            .arg("-C")
            .arg(repo.path())
            .args(["init", "-q"])
            .status()?;
        assert!(git.success());
        std::fs::create_dir(repo.path().join(".rosetta"))?;
        let model = crate::atlas::starter_atlas(repo.path())?;
        std::fs::write(
            repo.path().join(".rosetta/atlas.json"),
            crate::json_text::to_pretty_string(&model)?,
        )?;
        std::fs::write(repo.path().join("source.rs"), "before")?;
        let viewed = rosetta_core::snapshot::capture(repo.path(), ".rosetta", &model.project.repo);
        assert!(evidence_snapshot_matches(repo.path(), ".rosetta", &viewed.fingerprint).is_ok());
        std::fs::write(repo.path().join("source.rs"), "after")?;
        assert!(evidence_snapshot_matches(repo.path(), ".rosetta", &viewed.fingerprint).is_err());
        Ok(())
    }

    #[test]
    fn brand_is_bounded_and_validated() -> Result<(), Box<dyn std::error::Error>> {
        let temp = tempfile::tempdir()?;
        let path = temp.path().join("brand.json");
        std::fs::write(&path, r##"{"primary":"#123abc"}"##)?;
        assert!(Brand::from_file(&path).is_ok());
        std::fs::write(&path, r#"{"primary":"red"}"#)?;
        assert!(Brand::from_file(&path).is_err());
        std::fs::write(&path, r##"{"unknown":"#123abc"}"##)?;
        assert!(Brand::from_file(&path).is_err());
        Ok(())
    }

    #[test]
    fn evidence_stays_inside_the_repo_and_preserves_utf8() -> Result<(), Box<dyn std::error::Error>>
    {
        let repo = tempfile::tempdir()?;
        let outside = tempfile::tempdir()?;
        std::fs::write(
            repo.path().join("source.rs"),
            format!("{}é", "a".repeat(128 * 1024 - 1)),
        )?;
        std::fs::write(outside.path().join("secret"), "outside")?;
        let excerpt = evidence_file(repo.path(), "source.rs#symbol")?;
        assert_eq!(excerpt["path"], "source.rs");
        assert_eq!(excerpt["truncated"], true);
        assert_eq!(
            excerpt["content"].as_str().map(str::len),
            Some(128 * 1024 - 1)
        );
        assert!(evidence_file(repo.path(), "../secret").is_err());
        assert!(evidence_file(repo.path(), "/etc/passwd").is_err());
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(outside.path().join("secret"), repo.path().join("linked"))?;
            assert!(evidence_file(repo.path(), "linked").is_err());
        }
        Ok(())
    }
}
