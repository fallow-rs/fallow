//! The language server surface of the drift harness.
//!
//! The VS Code extension shows two counts of one project: the sidebar counts
//! the arrays of a bare CLI run (`fallow --format json --skip health`), and
//! the status bar counts the fields of the `fallow/analysisComplete`
//! notification of `fallow-lsp`. Invariant I15 checks that both give the
//! same count for each category.

use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use crate::surfaces::{scrub_environment, sibling_bin};

const ANALYSIS_COMPLETE: &str = "fallow/analysisComplete";
const LSP_TIMEOUT: Duration = Duration::from_mins(3);

/// Fields of the notification that are no finding count of one category.
const NOT_CATEGORY_FIELDS: &[&str] = &[
    "totalIssues",
    "duplicationPercentage",
    "changedSinceScope",
    "packageBaselines",
];

/// Arrays of the CLI `check` section that the notification has no field for.
/// The extension does not count them in its total (`COUNTED_FINDING_KEYS`
/// in `editors/vscode/src/analysis-utils.ts`).
const NOT_SENT_ARRAYS: &[&str] = &["deprecated_exports_in_use"];

/// The prefix of a field that counts only the findings of its array at
/// `error` severity: the component health arrays, as `total_issues` counts
/// them.
const ENFORCED_PREFIX: &str = "enforced";

/// The CLI arrays that one notification field counts. The notification sends
/// one count for the three boundary arrays; each other field counts the array
/// of the same name in snake case.
fn cli_arrays(field: &str) -> Vec<String> {
    if field == "boundaryViolations" {
        return [
            "boundary_violations",
            "boundary_coverage_violations",
            "boundary_call_violations",
        ]
        .map(str::to_string)
        .to_vec();
    }
    let mut snake = String::new();
    for ch in field.chars() {
        if ch.is_ascii_uppercase() {
            snake.push('_');
            snake.push(ch.to_ascii_lowercase());
        } else {
            snake.push(ch);
        }
    }
    vec![snake]
}

/// The `fallow-lsp` binary next to the `fallow` binary under test.
///
/// # Panics
///
/// Panics with a build instruction when the binary is missing or older than
/// one of its sources.
pub fn lsp_bin() -> PathBuf {
    sibling_bin("fallow-lsp")
}

/// The count of each category of the notification fields, by field name.
pub fn lsp_category_counts(params: &Value) -> BTreeMap<String, u64> {
    params
        .as_object()
        .expect("the notification params are an object")
        .iter()
        .filter(|(field, _)| !NOT_CATEGORY_FIELDS.contains(&field.as_str()))
        .map(|(field, value)| {
            let count = value
                .as_u64()
                .unwrap_or_else(|| panic!("{field} is not a count: {value}"));
            (field.clone(), count)
        })
        .collect()
}

/// The count of each category of a bare CLI run, by notification field name,
/// for the fields of `lsp`. An array of the run that no field counts is an
/// error, so a new category that the server does not send fails the check.
pub fn cli_category_counts(
    envelope: &Value,
    lsp: &BTreeMap<String, u64>,
) -> Result<BTreeMap<String, u64>, String> {
    let check = &envelope["check"];
    let length = |array: &str| check[array].as_array().map_or(0, Vec::len) as u64;
    let mut counted = Vec::new();
    let mut counts = BTreeMap::new();
    for field in lsp.keys() {
        let count = if field == "cloneGroups" {
            envelope["dupes"]["clone_groups"]
                .as_array()
                .map_or(0, Vec::len) as u64
        } else if let Some(rest) = field.strip_prefix(ENFORCED_PREFIX) {
            let mut chars = rest.chars();
            let first = chars
                .next()
                .unwrap_or_else(|| panic!("{field} names no component array"));
            let lowered = format!("{}{}", first.to_ascii_lowercase(), chars.as_str());
            let array = cli_arrays(&lowered).remove(0);
            let enforced = check[array.as_str()].as_array().map_or(0, |findings| {
                findings
                    .iter()
                    .filter(|finding| finding["effective_severity"] == "error")
                    .count()
            }) as u64;
            counted.push(array);
            enforced
        } else {
            let arrays = cli_arrays(field);
            let count = arrays.iter().map(|array| length(array)).sum();
            counted.extend(arrays);
            count
        };
        counts.insert(field.clone(), count);
    }
    let unsent: Vec<&str> = check
        .as_object()
        .into_iter()
        .flatten()
        .filter(|(array, value)| {
            value.as_array().is_some_and(|items| !items.is_empty())
                && !counted.contains(array)
                && !NOT_SENT_ARRAYS.contains(&array.as_str())
        })
        .map(|(array, _)| array.as_str())
        .collect();
    if unsent.is_empty() {
        Ok(counts)
    } else {
        Err(format!(
            "the CLI reports {unsent:?}, and fallow/analysisComplete has no field for them"
        ))
    }
}

fn send(stdin: &mut ChildStdin, message: &Value) {
    let body = message.to_string();
    write!(stdin, "Content-Length: {}\r\n\r\n{body}", body.len()).expect("write to fallow-lsp");
    stdin.flush().expect("flush fallow-lsp stdin");
}

fn read_messages(stdout: impl Read + Send + 'static) -> mpsc::Receiver<Value> {
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        let mut reader = BufReader::new(stdout);
        loop {
            let mut length = None;
            loop {
                let mut line = String::new();
                if reader.read_line(&mut line).unwrap_or(0) == 0 {
                    return;
                }
                let line = line.trim_end();
                if line.is_empty() {
                    break;
                }
                if let Some(value) = line.strip_prefix("Content-Length: ") {
                    length = value.parse::<usize>().ok();
                }
            }
            let Some(length) = length else { return };
            let mut body = vec![0; length];
            if reader.read_exact(&mut body).is_err() {
                return;
            }
            let Ok(message) = serde_json::from_slice::<Value>(&body) else {
                return;
            };
            if sender.send(message).is_err() {
                return;
            }
        }
    });
    receiver
}

fn file_uri(path: &Path) -> String {
    url::Url::from_file_path(path)
        .unwrap_or_else(|()| panic!("{} is not an absolute path", path.display()))
        .to_string()
}

/// A `fallow-lsp` child process that is killed when the case ends, also when
/// the case panics while it waits for a message.
struct Server(Child);

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// The first source file of the project, which the session opens: the
/// server starts its first run when a document opens.
fn first_source(root: &Path) -> PathBuf {
    let mut stack = vec![root.to_path_buf()];
    let mut sources = Vec::new();
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).expect("read project dir").flatten() {
            let path = entry.path();
            let name = entry.file_name();
            if path.is_dir() {
                if name != "node_modules" && name != ".git" {
                    stack.push(path);
                }
            } else if path
                .extension()
                .is_some_and(|ext| ["ts", "tsx", "js", "jsx", "mjs"].iter().any(|e| ext == *e))
            {
                sources.push(path);
            }
        }
    }
    sources.sort();
    sources
        .into_iter()
        .next()
        .expect("the project has a source file")
}

/// Run one editor session of `fallow-lsp` on `root` with the given
/// initialization options, and return the params of its first
/// `fallow/analysisComplete` notification. The session writes its cache to
/// `cache_dir`, so the project tree stays as the CLI run sees it.
///
/// # Panics
///
/// Panics when the server does not answer or sends no notification in time.
pub fn lsp_analysis_complete(root: &Path, cache_dir: &Path, options: &Value) -> Value {
    let mut command = Command::new(lsp_bin());
    scrub_environment(&mut command);
    let mut server = Server(
        command
            .current_dir(root)
            .env("FALLOW_CACHE_DIR", cache_dir)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn fallow-lsp"),
    );
    let mut stdin = server.0.stdin.take().expect("fallow-lsp stdin");
    let messages = read_messages(server.0.stdout.take().expect("fallow-lsp stdout"));
    send(
        &mut stdin,
        &json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "processId": null,
                "rootUri": file_uri(root),
                "capabilities": {},
                "initializationOptions": options,
            },
        }),
    );
    let deadline = Instant::now() + LSP_TIMEOUT;
    let next = || {
        let remaining = deadline.saturating_duration_since(Instant::now());
        messages
            .recv_timeout(remaining)
            .unwrap_or_else(|err| panic!("fallow-lsp sent no {ANALYSIS_COMPLETE} ({err})"))
    };
    // The server drops notifications that arrive before it answers
    // `initialize`.
    while next().get("id") != Some(&json!(1)) {}
    send(
        &mut stdin,
        &json!({"jsonrpc": "2.0", "method": "initialized", "params": {}}),
    );
    let opened = first_source(root);
    send(
        &mut stdin,
        &json!({
            "jsonrpc": "2.0",
            "method": "textDocument/didOpen",
            "params": {
                "textDocument": {
                    "uri": file_uri(&opened),
                    "languageId": "typescript",
                    "version": 1,
                    "text": std::fs::read_to_string(&opened).expect("read the opened source"),
                },
            },
        }),
    );
    let params = loop {
        let message = next();
        let method = message.get("method").and_then(Value::as_str);
        if method == Some(ANALYSIS_COMPLETE) {
            break message["params"].clone();
        }
        if let (Some(_), Some(id)) = (method, message.get("id")) {
            send(
                &mut stdin,
                &json!({"jsonrpc": "2.0", "id": id, "result": null}),
            );
        }
    };
    send(
        &mut stdin,
        &json!({"jsonrpc": "2.0", "id": 2, "method": "shutdown", "params": null}),
    );
    send(
        &mut stdin,
        &json!({"jsonrpc": "2.0", "method": "exit", "params": null}),
    );
    drop(stdin);
    drop(server);
    params
}
