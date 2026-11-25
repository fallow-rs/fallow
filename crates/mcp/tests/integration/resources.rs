#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests use unwrap and expect to keep fixture setup concise"
)]

//! Protocol-level coverage of the resource surface: the built `fallow-mcp`
//! binary is driven over stdio JSON-RPC through `initialize`,
//! `resources/list`, `resources/templates/list`, and `resources/read`, so the
//! capability declaration and the wire shape of every response are pinned
//! end to end rather than through the in-process free functions alone.

use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::Duration;

const RESPONSE_TIMEOUT: Duration = Duration::from_mins(1);

#[test]
fn resources_are_listed_and_read_over_stdio() {
    let mut server = McpServer::start();

    let capabilities = &server.initialize_result["capabilities"];
    assert!(
        capabilities["resources"].is_object(),
        "initialize must advertise resources: {capabilities}"
    );
    assert!(
        capabilities["resources"].get("subscribe").is_none()
            && capabilities["resources"].get("listChanged").is_none(),
        "static catalogue must not advertise subscribe or listChanged: {capabilities}"
    );

    let listed = server.request(2, "resources/list", &serde_json::json!({}));
    let resources = listed["result"]["resources"]
        .as_array()
        .unwrap_or_else(|| panic!("resources array: {listed}"));
    let uris: Vec<&str> = resources.iter().filter_map(|r| r["uri"].as_str()).collect();
    assert_eq!(
        uris,
        [
            "fallow://tools",
            "fallow://issue-types",
            "fallow://explain",
            "fallow://task-matrix",
            "fallow://schema/config",
            "fallow://schema/plugin",
            "fallow://schema/rule-pack",
            "fallow://schema/similar-code-snapshot",
        ]
    );
    for resource in resources {
        assert_eq!(resource["mimeType"], "application/json", "{resource}");
        assert!(
            resource["size"].as_u64().is_some_and(|s| s > 0),
            "{resource}"
        );
        assert_eq!(
            resource["annotations"]["audience"],
            serde_json::json!(["assistant"])
        );
    }

    let templates = server.request(3, "resources/templates/list", &serde_json::json!({}));
    let template_uris: Vec<&str> = templates["result"]["resourceTemplates"]
        .as_array()
        .unwrap_or_else(|| panic!("resourceTemplates array: {templates}"))
        .iter()
        .filter_map(|t| t["uriTemplate"].as_str())
        .collect();
    assert_eq!(
        template_uris,
        ["fallow://tools/{name}", "fallow://explain/{issue_type}"]
    );

    let task_matrix = server.read(4, "fallow://task-matrix");
    let rows = task_matrix["rows"].as_array().expect("rows");
    assert!(
        rows.iter()
            .any(|row| row["command"] == "fallow audit --base <ref>"),
        "task matrix must carry the audit row: {task_matrix}"
    );

    let explain = server.read(5, "fallow://explain/unused-export");
    assert_eq!(explain["kind"], "explain");
    assert_eq!(explain["id"], "fallow/unused-export");

    let error = server.request(
        6,
        "resources/read",
        &serde_json::json!({ "uri": "fallow://explain/not-a-rule" }),
    );
    assert!(
        error["result"].is_null(),
        "unknown issue type must not succeed: {error}"
    );
    assert!(
        error["error"]["data"]["nearest_matches"].is_array(),
        "structured error must list nearest matches: {error}"
    );
    assert_eq!(error["error"]["data"]["index"], "fallow://explain");

    let unknown = server.request(
        7,
        "resources/read",
        &serde_json::json!({ "uri": "fallow://nope" }),
    );
    assert!(
        unknown["error"]["data"]["known_uris"].is_array(),
        "unknown uri error must list the catalogue: {unknown}"
    );
}

struct McpServer {
    child: Child,
    stdin: ChildStdin,
    lines: Receiver<String>,
    initialize_result: serde_json::Value,
}

impl Drop for McpServer {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl McpServer {
    fn start() -> Self {
        Self::start_with("2024-11-05")
    }

    fn start_with(protocol: &str) -> Self {
        let mut server = Self::spawn();
        server.send(&format!(
            r#"{{"jsonrpc":"2.0","id":1,"method":"initialize","params":{{"protocolVersion":"{protocol}","capabilities":{{}},"clientInfo":{{"name":"resources-test","version":"0"}}}}}}"#
        ));
        let response = server.response(1);
        assert!(
            response["result"]["serverInfo"].is_object(),
            "initialize must return server info: {response}"
        );
        server.initialize_result = response["result"].clone();
        server.send(r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#);
        server
    }

    fn spawn() -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_fallow-mcp"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn fallow-mcp");

        let stdin = child.stdin.take().expect("child stdin");
        let stdout = child.stdout.take().expect("child stdout");
        let (sender, lines) = mpsc::channel();
        thread::spawn(move || {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                if sender.send(line).is_err() {
                    return;
                }
            }
        });

        Self {
            child,
            stdin,
            lines,
            initialize_result: serde_json::Value::Null,
        }
    }

    fn request(&mut self, id: u64, method: &str, params: &serde_json::Value) -> serde_json::Value {
        let request = serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        });
        self.send(&serde_json::to_string(&request).expect("serialize request"));
        self.response(id)
    }

    /// Read a resource and parse its single JSON text content.
    fn read(&mut self, id: u64, uri: &str) -> serde_json::Value {
        let response = self.request(id, "resources/read", &serde_json::json!({ "uri": uri }));
        let contents = response["result"]["contents"]
            .as_array()
            .unwrap_or_else(|| panic!("contents array for {uri}: {response}"));
        assert_eq!(contents.len(), 1, "{uri} returns one content item");
        assert_eq!(contents[0]["uri"], uri);
        assert_eq!(contents[0]["mimeType"], "application/json");
        assert_eq!(
            contents[0]["_meta"]["fallow_version"],
            env!("CARGO_PKG_VERSION"),
            "{uri} carries the server version in _meta"
        );
        let text = contents[0]["text"]
            .as_str()
            .unwrap_or_else(|| panic!("text content for {uri}: {response}"));
        serde_json::from_str(text).unwrap_or_else(|err| panic!("{uri} payload is JSON: {err}"))
    }

    fn send(&mut self, message: &str) {
        writeln!(self.stdin, "{message}").expect("write message");
        self.stdin.flush().expect("flush message");
    }

    fn response(&self, id: u64) -> serde_json::Value {
        loop {
            let line = self
                .lines
                .recv_timeout(RESPONSE_TIMEOUT)
                .unwrap_or_else(|err| panic!("no response for id {id}: {err}"));
            let Ok(message) = serde_json::from_str::<serde_json::Value>(&line) else {
                continue;
            };
            if message["id"].as_u64() == Some(id) {
                return message;
            }
        }
    }
}

/// Initialize negotiation currently selects the legacy resource error code,
/// even when a newer protocol is requested.
#[test]
fn unknown_uri_error_code_follows_initialize_negotiation() {
    let mut server = McpServer::start_with("2026-07-28");
    assert_eq!(server.initialize_result["protocolVersion"], "2025-11-25");
    let error = server.request(
        2,
        "resources/read",
        &serde_json::json!({ "uri": "fallow://nope" }),
    );
    assert_eq!(error["error"]["code"], -32002, "{error}");
    assert!(
        error["error"]["data"]["known_uris"].is_array(),
        "structured data must survive legacy error classification: {error}"
    );
}

#[test]
fn modern_request_metadata_uses_invalid_params_for_unknown_uri() {
    let mut server = McpServer::spawn();
    let error = server.request(
        2,
        "resources/read",
        &serde_json::json!({
            "uri": "fallow://nope",
            "_meta": {
                "io.modelcontextprotocol/protocolVersion": "2026-07-28",
                "io.modelcontextprotocol/clientInfo": {"name": "resources-test", "version": "0"},
                "io.modelcontextprotocol/clientCapabilities": {}
            }
        }),
    );
    assert_eq!(error["error"]["code"], -32602, "{error}");
    assert!(
        error["error"]["data"]["known_uris"].is_array(),
        "structured data must survive modern error classification: {error}"
    );
}

const SKILLS_EXTENSION: &str = "io.modelcontextprotocol/skills";
const RELEASED_SKILLS: &[&str] = &["fallow", "fallow-setup"];

fn released_skill_dir(name: &str) -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../npm/fallow/skills")
        .join(name)
}

/// Every file below `dir`, as a path relative to `dir` with forward slashes.
fn files_below(dir: &std::path::Path) -> Vec<String> {
    let mut found = Vec::new();
    let mut pending = vec![dir.to_path_buf()];
    while let Some(current) = pending.pop() {
        for entry in std::fs::read_dir(&current).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                pending.push(path);
            } else {
                let relative = path.strip_prefix(dir).unwrap();
                found.push(relative.to_string_lossy().replace('\\', "/"));
            }
        }
    }
    found.sort();
    found
}

#[test]
fn released_skills_are_served_byte_for_byte() {
    let mut server = McpServer::start_with("2025-11-25");
    let extensions = &server.initialize_result["capabilities"]["extensions"];
    assert!(
        extensions[SKILLS_EXTENSION].is_object(),
        "initialize must declare the skills extension: {extensions}"
    );
    assert!(
        extensions[SKILLS_EXTENSION]["directoryRead"] != true,
        "the manifest is complete, so directory reads are not offered: {extensions}"
    );

    let listing = server.request(2, "skills/list", &serde_json::json!({}));
    let skills = listing["result"]["skills"]
        .as_array()
        .unwrap_or_else(|| panic!("skills array: {listing}"));
    let names: Vec<&str> = skills
        .iter()
        .map(|skill| skill["frontmatter"]["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, RELEASED_SKILLS, "{listing}");

    let mut id = 10;
    for skill in skills {
        let name = skill["frontmatter"]["name"].as_str().unwrap();
        let uri = format!("skill://{name}/SKILL.md");
        assert_eq!(skill["uri"], uri.as_str());
        assert_eq!(skill["frontmatter"]["license"], "MIT", "{skill}");
        assert!(skill["frontmatter"]["description"].is_string(), "{skill}");

        let dir = released_skill_dir(name);
        let manifest = skill["resources"].as_array().unwrap();
        let mut served: Vec<String> = manifest
            .iter()
            .map(|file| {
                file["uri"]
                    .as_str()
                    .unwrap()
                    .strip_prefix(&format!("skill://{name}/"))
                    .unwrap()
                    .to_string()
            })
            .collect();
        served.sort();
        assert_eq!(
            served,
            files_below(&dir),
            "manifest of {name} lists every released file"
        );

        for file in manifest {
            let file_uri = file["uri"].as_str().unwrap();
            let relative = file_uri.strip_prefix(&format!("skill://{name}/")).unwrap();
            let on_disk = std::fs::read(dir.join(relative)).unwrap();
            assert_eq!(file["size"], on_disk.len(), "{file_uri}");
            assert_eq!(
                file["digest"],
                rmcp::model::sha256_digest(&on_disk),
                "{file_uri}"
            );

            id += 1;
            let read = server.request(
                id,
                "resources/read",
                &serde_json::json!({ "uri": file_uri }),
            );
            let contents = &read["result"]["contents"][0];
            assert_eq!(contents["uri"], file_uri, "{read}");
            assert_eq!(
                contents["text"].as_str().map(str::as_bytes),
                Some(on_disk.as_slice()),
                "{file_uri} is served byte for byte"
            );
        }

        id += 1;
        let got = server.request(id, "skills/get", &serde_json::json!({ "uri": uri }));
        assert_eq!(
            &got["result"]["skill"], skill,
            "skills/get matches skills/list"
        );
    }
}

#[test]
fn unknown_skill_uri_is_invalid_params() {
    let mut server = McpServer::start_with("2025-11-25");
    let error = server.request(
        2,
        "skills/get",
        &serde_json::json!({ "uri": "skill://nope/SKILL.md" }),
    );
    assert_eq!(error["error"]["code"], -32602, "{error}");
    let missing = server.request(
        3,
        "resources/read",
        &serde_json::json!({ "uri": "skill://fallow/references/nope.md" }),
    );
    assert!(missing["error"].is_object(), "{missing}");
}

#[test]
fn skills_are_listed_with_modern_request_metadata() {
    let mut server = McpServer::spawn();
    let listing = server.request(
        2,
        "skills/list",
        &serde_json::json!({
            "_meta": {
                "io.modelcontextprotocol/protocolVersion": "2026-07-28",
                "io.modelcontextprotocol/clientInfo": {"name": "resources-test", "version": "0"},
                "io.modelcontextprotocol/clientCapabilities": {}
            }
        }),
    );
    assert_eq!(listing["result"]["resultType"], "complete", "{listing}");
    assert!(listing["result"]["ttlMs"].is_u64(), "{listing}");
    assert!(listing["result"]["cacheScope"].is_string(), "{listing}");
    assert_eq!(
        listing["result"]["skills"].as_array().map(Vec::len),
        Some(2)
    );
}
