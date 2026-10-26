//! Exercise the public stdio protocol, including a stable focused-view round trip.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::path::Path;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

use serde_json::{Value, json};

struct Session {
    repo: tempfile::TempDir,
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    next_id: u64,
}

struct Viewer(Child);

impl Drop for Viewer {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

impl Session {
    fn start() -> Result<Self, Box<dyn std::error::Error>> {
        let fixture = format!(
            "{}/../rosetta-atlas/tests/fixtures/repo",
            env!("CARGO_MANIFEST_DIR")
        );
        let repo = tempfile::tempdir()?;
        copy_tree(Path::new(&fixture), repo.path())?;
        for args in [
            vec!["init", "-q"],
            vec!["add", "-A"],
            vec![
                "-c",
                "user.name=fixture",
                "-c",
                "user.email=fixture@example.com",
                "commit",
                "-qm",
                "fixture",
            ],
        ] {
            let result = Command::new("git")
                .arg("-C")
                .arg(repo.path())
                .args(args)
                .status()?;
            if !result.success() {
                return Err("could not prepare isolated MCP fixture".into());
            }
        }
        rosetta_atlas::atlas::sync_atlas(repo.path())?;
        let mut child = Command::new(env!("CARGO_BIN_EXE_rosetta"))
            .arg("mcp")
            .arg("--repo")
            .arg(repo.path())
            .current_dir(env!("CARGO_MANIFEST_DIR"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        let stdin = child.stdin.take().ok_or("missing stdin")?;
        let stdout = BufReader::new(child.stdout.take().ok_or("missing stdout")?);
        Ok(Self {
            repo,
            child,
            stdin,
            stdout,
            next_id: 0,
        })
    }

    fn request(
        &mut self,
        method: &str,
        params: Value,
    ) -> Result<Value, Box<dyn std::error::Error>> {
        self.next_id += 1;
        writeln!(
            self.stdin,
            "{}",
            json!({"jsonrpc":"2.0","id":self.next_id,"method":method,"params":params})
        )?;
        self.stdin.flush()?;
        let mut line = String::new();
        if self.stdout.read_line(&mut line)? == 0 {
            return Err("server closed stdout".into());
        }
        let reply: Value = serde_json::from_str(&line)?;
        if reply.get("error").is_some() {
            return Err(format!("{method}: {reply}").into());
        }
        Ok(reply["result"].clone())
    }

    fn call(&mut self, name: &str, arguments: Value) -> Result<Value, Box<dyn std::error::Error>> {
        let result = self.request("tools/call", json!({"name":name,"arguments":arguments}))?;
        if result["isError"] == true {
            return Err(format!("{name}: {result}").into());
        }
        Ok(result["structuredContent"].clone())
    }
}

fn copy_tree(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_tree(&entry.path(), &target)?;
        } else if entry.file_type()?.is_file() {
            std::fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
}

impl Drop for Session {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[test]
fn tools_resources_and_shared_view_round_trip() -> Result<(), Box<dyn std::error::Error>> {
    let mut session = Session::start()?;
    session.request("initialize", json!({"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"test","version":"1"}}))?;
    writeln!(
        session.stdin,
        "{}",
        json!({"jsonrpc":"2.0","method":"notifications/initialized"})
    )?;
    session.stdin.flush()?;

    let tools = session.request("tools/list", json!({}))?;
    let names: Vec<_> = tools["tools"]
        .as_array()
        .ok_or("tools missing")?
        .iter()
        .filter_map(|t| t["name"].as_str())
        .collect();
    for expected in [
        "define-term",
        "lint-text",
        "terms-for-paths",
        "list-parts",
        "read-part",
        "incomers",
        "outgoers",
        "paths-between",
        "impact",
        "render-view",
        "open-view",
        "read-view",
        "propose-change",
    ] {
        assert!(names.contains(&expected), "missing {expected}");
    }
    let resources = session.request("resources/list", json!({}))?;
    assert_eq!(
        resources["resources"]
            .as_array()
            .ok_or("resources missing")?
            .len(),
        2
    );
    let glossary = session.request("resources/read", json!({"uri":"rosetta://GLOSSARY.md"}))?;
    assert!(
        glossary["contents"][0]["text"]
            .as_str()
            .ok_or("text missing")?
            .contains("Door")
    );

    let parts = session.call("list-parts", json!({"query":"door"}))?;
    assert!(
        parts["parts"]
            .as_array()
            .ok_or("parts missing")?
            .iter()
            .any(|p| p["id"] == "door")
    );
    let incoming = session.call("incomers", json!({"key":"store"}))?;
    assert!(
        incoming["edges"]
            .as_array()
            .ok_or("edges missing")?
            .iter()
            .any(|e| e["from"] == "door" && e["to"] == "store")
    );
    let route = session.call("paths-between", json!({"from":"web","to":"store"}))?;
    assert!(
        route["paths"]
            .as_array()
            .ok_or("paths missing")?
            .iter()
            .any(|path| path["nodes"] == json!(["web", "door", "store"])
                && path["edges"][0]["kind"] == "calls"
                && path["edges"][1]["kind"] == "writes")
    );
    let view = session.call("open-view", json!({"roots":["store"],"max_chars":4000}))?;
    assert!(view.to_string().chars().count() <= 4000);
    let reference = view["reference"].as_str().ok_or("reference missing")?;
    assert!(reference.starts_with("rv1."));
    assert!(view["snapshot"]["fingerprint"].as_str().is_some());
    assert!(view["omissions"].is_object());
    let reopened = session.call("read-view", json!({"reference":reference,"max_chars":4000}))?;
    assert_eq!(view["id"], reopened["id"]);
    assert_eq!(view["reference"], reopened["reference"]);
    assert_eq!(view["snapshot"], reopened["snapshot"]);
    let complete = session.call(
        "read-view",
        json!({"reference":reference,"max_chars":100000}),
    )?;
    let cli = Command::new(env!("CARGO_BIN_EXE_rosetta"))
        .args(["atlas", "context", "--view", reference, "--json"])
        .current_dir(session.repo.path())
        .output()?;
    assert!(
        cli.status.success(),
        "{}",
        String::from_utf8_lossy(&cli.stderr)
    );
    let cli_view: Value = serde_json::from_slice(&cli.stdout)?;
    for key in [
        "reference",
        "snapshot",
        "query",
        "nodes",
        "edges",
        "terms",
        "omissions",
    ] {
        assert_eq!(complete[key], cli_view[key], "CLI/MCP mismatch for {key}");
    }
    let terms = session.call("terms-for-paths", json!({"reference":reference}))?;
    assert_eq!(terms["view_reference"], reference);
    let proposal = json!({
        "schema": "rosetta.impact-proposal/v1",
        "base_commit": view["snapshot"]["head"].as_str().unwrap_or(""),
        "base_model_revision": view["snapshot"]["model_revision"],
        "intents": [{"kind":"claim","text":"The store is fast.","rationale":"Review this claim."}]
    });
    let reviewed = session.call(
        "propose-change",
        json!({"reference":reference,"proposal":proposal}),
    )?;
    assert_eq!(reviewed["status"], "proposed_unverified");
    assert_eq!(reviewed["reference"], reference);

    let mut viewer = Viewer(
        Command::new(env!("CARGO_BIN_EXE_rosetta"))
            .args(["atlas", "open", "--port", "0", "--view", reference])
            .current_dir(session.repo.path())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?,
    );
    let stdout = viewer.0.stdout.take().ok_or("viewer stdout missing")?;
    let mut stdout = BufReader::new(stdout);
    let mut url = String::new();
    stdout.read_line(&mut url)?;
    if url.is_empty() {
        let mut diagnostics = String::new();
        if let Some(mut stderr) = viewer.0.stderr.take() {
            stderr.read_to_string(&mut diagnostics)?;
        }
        // Some sandboxes deny loopback bind. The CLI/MCP parity above still runs.
        if diagnostics.contains("Operation not permitted")
            && std::env::var_os("ROSETTA_REQUIRE_VIEWER_BIND").is_none()
        {
            return Ok(());
        }
        return Err(format!("viewer did not print a URL: {diagnostics}").into());
    }
    let authority = url
        .trim()
        .strip_prefix("http://")
        .ok_or("viewer URL missing")?
        .split('/')
        .next()
        .ok_or("viewer address missing")?;
    let mut stream = TcpStream::connect(authority)?;
    stream.set_read_timeout(Some(std::time::Duration::from_secs(5)))?;
    write!(
        stream,
        "GET /focus.json?view={reference} HTTP/1.1\r\nHost: {authority}\r\nConnection: close\r\n\r\n"
    )?;
    let mut response = String::new();
    stream.read_to_string(&mut response)?;
    assert!(response.starts_with("HTTP/1.1 200"), "{response}");
    let body = response
        .split_once("\r\n\r\n")
        .ok_or("viewer response body missing")?
        .1;
    let viewer_view: Value = serde_json::from_str(body)?;
    for key in [
        "reference",
        "snapshot",
        "query",
        "nodes",
        "edges",
        "terms",
        "omissions",
    ] {
        assert_eq!(
            complete[key], viewer_view[key],
            "viewer/MCP mismatch for {key}"
        );
    }
    Ok(())
}
