//! Runners that drive one analysis through each surface: the CLI binary, the
//! `fallow-mcp` server over stdio (typed path and CLI-fallback path), and
//! `fallow_api` in-process.

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::{Duration, SystemTime};

use serde_json::{Value, json};

use crate::common::{CommandOutput, fallow_bin};
use crate::keys::{KeySet, dead_code_keys, dupes_keys, health_keys, mcp_result_envelope};

/// How long one MCP request may take before the harness gives up.
const MCP_TIMEOUT: Duration = Duration::from_mins(3);

/// Environment variables the harness passes through to child processes even
/// though they start with `FALLOW_`.
const KEPT_FALLOW_VARS: &[&str] = &["FALLOW_BIN"];

/// The analyses the harness compares.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Analysis {
    DeadCode,
    Dupes,
    Health,
}

impl Analysis {
    pub const ALL: [Self; 3] = [Self::DeadCode, Self::Dupes, Self::Health];

    pub const fn cli_command(self) -> &'static str {
        match self {
            Self::DeadCode => "dead-code",
            Self::Dupes => "dupes",
            Self::Health => "health",
        }
    }

    /// The flag of bare `fallow` that loads a baseline of this analysis.
    pub const fn combined_baseline_flag(self) -> &'static str {
        match self {
            Self::DeadCode => "--baseline",
            Self::Dupes => "--dupes-baseline",
            Self::Health => "--health-baseline",
        }
    }

    /// Reduce an envelope of this analysis to its key set.
    pub fn keys(self, envelope: &Value) -> KeySet {
        match self {
            Self::DeadCode => dead_code_keys(envelope),
            Self::Dupes => dupes_keys(envelope),
            Self::Health => health_keys(envelope),
        }
    }
}

/// Scope flags of one run. Invariant I8 compares them across surfaces.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Scope {
    pub changed_since: Option<String>,
    pub workspace: Option<String>,
    pub production: bool,
}

impl Scope {
    fn cli_args(&self) -> Vec<String> {
        let mut args = Vec::new();
        if let Some(reference) = &self.changed_since {
            args.extend(["--changed-since".to_string(), reference.clone()]);
        }
        if let Some(workspace) = &self.workspace {
            args.extend(["--workspace".to_string(), workspace.clone()]);
        }
        if self.production {
            args.push("--production".to_string());
        }
        args
    }
}

/// A child process environment without the `FALLOW_*` variables of the
/// developer shell, so every surface reads the same inputs.
fn scrub_environment(command: &mut Command) {
    for (name, _) in std::env::vars_os() {
        let Some(name) = name.to_str() else {
            continue;
        };
        if name.starts_with("FALLOW_") && !KEPT_FALLOW_VARS.contains(&name) {
            command.env_remove(name);
        }
    }
    command
        .env("RUST_LOG", "")
        .env("NO_COLOR", "1")
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE");
}

/// Run the CLI with `args` against `root` and capture the output.
///
/// # Panics
///
/// Panics when the binary cannot start.
pub fn run_cli(root: &Path, args: &[String]) -> CommandOutput {
    run_cli_format(root, args, "json")
}

/// Run the CLI with `args` against `root` in the output `format`.
///
/// # Panics
///
/// Panics when the binary cannot start.
pub fn run_cli_format(root: &Path, args: &[String], format: &str) -> CommandOutput {
    let mut command = Command::new(fallow_bin());
    scrub_environment(&mut command);
    command
        .args(args)
        .arg("--root")
        .arg(root)
        .args(["--format", format, "--quiet", "--no-cache"]);
    let output = command.output().expect("run the fallow binary");
    CommandOutput {
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        code: output.status.code().unwrap_or(-1),
    }
}

/// Parse a CLI run as a JSON envelope, with the exit-code rule for findings.
///
/// # Panics
///
/// Panics on an exit code other than 0 or 1, or on output that is not JSON.
pub fn cli_envelope(output: &CommandOutput) -> Value {
    assert!(
        output.code == 0 || output.code == 1,
        "fallow exited with {} (0 or 1 expected)\nstdout:\n{}\nstderr:\n{}",
        output.code,
        output.stdout,
        output.stderr
    );
    crate::common::parse_json(output)
}

/// Run one analysis through the CLI and reduce it to keys.
pub fn cli_keys(analysis: Analysis, root: &Path, scope: &Scope, baseline: Option<&Path>) -> KeySet {
    let mut args = vec![analysis.cli_command().to_string()];
    args.extend(scope.cli_args());
    if let Some(baseline) = baseline {
        args.extend(["--baseline".to_string(), baseline.display().to_string()]);
    }
    analysis.keys(&cli_envelope(&run_cli(root, &args)))
}

/// Run bare `fallow` through the CLI with one baseline per analysis, and
/// return its envelope.
pub fn cli_combined(root: &Path, baselines: Option<&[PathBuf; 3]>) -> Value {
    let mut args = Vec::new();
    if let Some(baselines) = baselines {
        for (analysis, path) in Analysis::ALL.into_iter().zip(baselines) {
            args.extend([
                analysis.combined_baseline_flag().to_string(),
                path.display().to_string(),
            ]);
        }
    }
    cli_envelope(&run_cli(root, &args))
}

/// Run one analysis through the CLI with `--save-baseline` into `target`.
pub fn cli_save_baseline(analysis: Analysis, root: &Path, target: &Path) {
    let args = vec![
        analysis.cli_command().to_string(),
        "--save-baseline".to_string(),
        target.display().to_string(),
    ];
    cli_envelope(&run_cli(root, &args));
    assert!(
        target.is_file(),
        "--save-baseline wrote no file at {}",
        target.display()
    );
}

/// The `fallow-mcp` binary next to the `fallow` binary under test.
///
/// # Panics
///
/// Panics with a build instruction when the binary is missing. The harness
/// never skips the MCP surface.
pub fn mcp_bin() -> PathBuf {
    let path = fallow_bin().with_file_name(format!("fallow-mcp{}", std::env::consts::EXE_SUFFIX));
    assert!(
        path.is_file(),
        "the drift harness needs the fallow-mcp binary at {}. Run `cargo build -p fallow-mcp` first, \
         then run the harness again.",
        path.display()
    );
    assert_mcp_bin_current(&path);
    path
}

/// Fail when a source file of `fallow-mcp` is newer than the binary.
///
/// `cargo test -p fallow-cli` does not rebuild `fallow-mcp`, so an old binary
/// would run old `fallow_api` code on the MCP typed path. The list of sources
/// comes from the dep-info file that cargo writes next to the binary. It is the
/// list cargo itself compares against, so a rebuild always clears the failure.
/// A crate outside the dependency graph of `fallow-mcp` never triggers it.
fn assert_mcp_bin_current(binary: &Path) {
    let dep_info = binary.with_extension("d");
    let listing = std::fs::read_to_string(&dep_info).unwrap_or_else(|err| {
        panic!(
            "cannot read {} ({err}). Run `cargo build -p fallow-mcp`, then run the harness again.",
            dep_info.display()
        )
    });
    let built = modified(binary).expect("read the fallow-mcp modification time");
    let newest = dep_info_sources(&listing)
        .into_iter()
        .filter_map(|source| modified(&source).map(|time| (time, source)))
        .max();
    if let Some((time, source)) = newest
        && time > built
    {
        panic!(
            "fallow-mcp at {} is older than {}. Run `cargo build -p fallow-mcp`, then run the \
             harness again.",
            binary.display(),
            source.display()
        );
    }
}

fn modified(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path)
        .and_then(|meta| meta.modified())
        .ok()
}

/// The prerequisites of a make-style dep-info file. Each line has the form
/// `target: source source`, and a backslash escapes a space inside a path.
fn dep_info_sources(listing: &str) -> Vec<PathBuf> {
    let mut sources = Vec::new();
    for line in listing.lines() {
        let Some((_, prerequisites)) = line.split_once(": ") else {
            continue;
        };
        let mut current = String::new();
        let mut chars = prerequisites.chars().peekable();
        while let Some(ch) = chars.next() {
            match ch {
                '\\' if chars.peek() == Some(&' ') => {
                    current.push(' ');
                    chars.next();
                }
                ' ' | '\t' => {
                    if !current.is_empty() {
                        sources.push(PathBuf::from(std::mem::take(&mut current)));
                    }
                }
                _ => current.push(ch),
            }
        }
        if !current.is_empty() {
            sources.push(PathBuf::from(current));
        }
    }
    sources
}

/// A `fallow-mcp` server driven over stdio JSON-RPC.
pub struct McpServer {
    child: Child,
    stdin: ChildStdin,
    lines: Receiver<String>,
    next_id: u64,
}

impl McpServer {
    /// Start the server with `FALLOW_BIN` set to the CLI binary under test, so
    /// the CLI-fallback path runs the same build.
    ///
    /// # Panics
    ///
    /// Panics when the binary is missing or the handshake fails.
    pub fn start() -> Self {
        Self::start_with_cli(&fallow_bin())
    }

    /// Start a server whose `FALLOW_BIN` names a file that does not exist.
    /// Every call it answers took the typed path: a CLI fallback cannot start.
    pub fn start_typed_only() -> Self {
        let missing = std::env::temp_dir().join("fallow-drift-no-cli-fallback");
        assert!(
            !missing.exists(),
            "{} must not exist, so a CLI fallback fails",
            missing.display()
        );
        Self::start_with_cli(&missing)
    }

    fn start_with_cli(cli: &Path) -> Self {
        let mut command = Command::new(mcp_bin());
        scrub_environment(&mut command);
        let mut child = command
            .env("FALLOW_BIN", cli)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn fallow-mcp");
        let stdin = child.stdin.take().expect("fallow-mcp stdin");
        let stdout = child.stdout.take().expect("fallow-mcp stdout");
        let (sender, lines) = mpsc::channel();
        thread::spawn(move || {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                if sender.send(line).is_err() {
                    return;
                }
            }
        });
        let mut server = Self {
            child,
            stdin,
            lines,
            next_id: 1,
        };
        let response = server.request(
            "initialize",
            &json!({
                "protocolVersion": "2024-11-05",
                "capabilities": {},
                "clientInfo": {"name": "fallow-drift-harness", "version": "0"}
            }),
        );
        assert!(
            response["result"]["serverInfo"].is_object(),
            "initialize must return server info: {response}"
        );
        server.send(&json!({"jsonrpc": "2.0", "method": "notifications/initialized"}));
        server
    }

    fn send(&mut self, message: &Value) {
        let line = serde_json::to_string(message).expect("serialize JSON-RPC message");
        writeln!(self.stdin, "{line}").expect("write to fallow-mcp");
        self.stdin.flush().expect("flush fallow-mcp stdin");
    }

    fn request(&mut self, method: &str, params: &Value) -> Value {
        let id = self.next_id;
        self.next_id += 1;
        self.send(&json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}));
        loop {
            let line = self
                .lines
                .recv_timeout(MCP_TIMEOUT)
                .unwrap_or_else(|err| panic!("no fallow-mcp response to {method} ({err})"));
            let Ok(message) = serde_json::from_str::<Value>(&line) else {
                continue;
            };
            if message["id"] == id {
                return message;
            }
        }
    }

    /// Call one tool and return its parsed envelope.
    pub fn call_tool(&mut self, tool: &str, arguments: &Value) -> Value {
        let response = self.request("tools/call", &json!({"name": tool, "arguments": arguments}));
        mcp_result_envelope(&response["result"])
    }
}

impl Drop for McpServer {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Which MCP code path a call must take.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum McpPath {
    /// In-process `fallow_api` inside the server.
    Typed,
    /// A `fallow` subprocess. `save_baseline` forces it: every analysis tool
    /// routes a baseline parameter to the CLI, and saving a baseline does not
    /// change the findings of the run.
    CliFallback,
}

/// Whether the MCP tool for `analysis` accepts every flag in `scope`.
pub const fn mcp_supports(analysis: Analysis, scope: &Scope) -> bool {
    !(matches!(analysis, Analysis::Dupes) && scope.production)
}

/// Run one analysis through the MCP server and reduce it to keys.
///
/// `scratch` receives the baseline file that proves the fallback path ran.
///
/// # Panics
///
/// Panics when the tool fails, or when the fallback path wrote no baseline.
pub fn mcp_keys(
    server: &mut McpServer,
    path: McpPath,
    analysis: Analysis,
    root: &Path,
    scope: &Scope,
    scratch: &Path,
) -> KeySet {
    analysis.keys(&mcp_envelope(server, path, analysis, root, scope, scratch))
}

/// Run one analysis through the MCP server and return its envelope. See
/// [`mcp_keys`].
pub fn mcp_envelope(
    server: &mut McpServer,
    path: McpPath,
    analysis: Analysis,
    root: &Path,
    scope: &Scope,
    scratch: &Path,
) -> Value {
    let mut arguments = json!({"root": root.display().to_string(), "no_cache": true});
    let tool = match analysis {
        Analysis::DeadCode if scope.changed_since.is_some() => "check_changed",
        Analysis::DeadCode => "analyze",
        Analysis::Dupes => "find_dupes",
        Analysis::Health => "check_health",
    };
    if let Some(reference) = &scope.changed_since {
        let key = if tool == "check_changed" {
            "since"
        } else {
            "changed_since"
        };
        arguments[key] = json!(reference);
    }
    if let Some(workspace) = &scope.workspace {
        arguments["workspace"] = json!(workspace);
    }
    if scope.production {
        arguments["production"] = json!(true);
    }
    let proof = scratch.join(format!("mcp-fallback-{tool}.json"));
    let _ = std::fs::remove_file(&proof);
    if path == McpPath::CliFallback {
        arguments["save_baseline"] = json!(proof.display().to_string());
    }
    let envelope = server.call_tool(tool, &arguments);
    assert_eq!(
        proof.is_file(),
        path == McpPath::CliFallback,
        "the MCP {tool} call did not take the {path:?} path (baseline proof file at {})",
        proof.display()
    );
    envelope
}

/// Run one analysis through `fallow_api` in this process and reduce it to keys.
///
/// # Panics
///
/// Panics when the programmatic run fails.
pub fn api_keys(analysis: Analysis, root: &Path, scope: &Scope) -> KeySet {
    let options = fallow_api::AnalysisOptions {
        root: Some(root.to_path_buf()),
        no_cache: true,
        production: scope.production,
        production_override: scope.production.then_some(true),
        changed_since: scope.changed_since.clone(),
        workspace: scope.workspace.clone().map(|workspace| vec![workspace]),
        explain: true,
        ..fallow_api::AnalysisOptions::default()
    };
    let envelope = match analysis {
        Analysis::DeadCode => fallow_api::run_dead_code(&fallow_api::DeadCodeOptions {
            analysis: options,
            ..fallow_api::DeadCodeOptions::default()
        })
        .and_then(fallow_api::serialize_dead_code_programmatic_json),
        Analysis::Dupes => fallow_api::run_duplication(&fallow_api::DuplicationOptions {
            analysis: options,
            ..fallow_api::DuplicationOptions::default()
        })
        .and_then(fallow_api::serialize_duplication_programmatic_json),
        Analysis::Health => fallow_api::run_health(&fallow_api::ComplexityOptions {
            analysis: options,
            ..fallow_api::ComplexityOptions::default()
        })
        .and_then(fallow_api::serialize_health_programmatic_json),
    }
    .unwrap_or_else(|err| panic!("fallow_api {analysis:?} failed: {err:?}"));
    analysis.keys(&envelope)
}

/// Run `fallow_api` dead-code analysis with a saved dead-code baseline.
///
/// # Panics
///
/// Panics when the programmatic run fails.
pub fn api_dead_code_keys_with_baseline(root: &Path, baseline: &Path) -> KeySet {
    let options = fallow_api::DeadCodeOptions {
        analysis: fallow_api::AnalysisOptions {
            root: Some(root.to_path_buf()),
            no_cache: true,
            explain: true,
            ..fallow_api::AnalysisOptions::default()
        },
        ..fallow_api::DeadCodeOptions::default()
    };
    let envelope = fallow_api::run_dead_code_with_baseline(&options, Some(baseline))
        .and_then(fallow_api::serialize_dead_code_programmatic_json)
        .unwrap_or_else(|err| panic!("fallow_api dead-code with a baseline failed: {err:?}"));
    Analysis::DeadCode.keys(&envelope)
}

/// The base ref of every audit run: the base commit of a generated project.
pub const AUDIT_BASE_REF: &str = "HEAD~1";

/// Run `fallow audit` against the base commit and return its envelope.
pub fn cli_audit(root: &Path) -> Value {
    cli_envelope(&run_cli(
        root,
        &[
            "audit".to_string(),
            "--base".to_string(),
            AUDIT_BASE_REF.to_string(),
        ],
    ))
}

/// Run the MCP `audit` tool against the base commit and return its envelope.
/// `server` must be a [`McpServer::start_typed_only`] server, so the result
/// comes from the typed path.
pub fn mcp_audit(server: &mut McpServer, root: &Path) -> Value {
    server.call_tool(
        "audit",
        &json!({
            "root": root.display().to_string(),
            "base": AUDIT_BASE_REF,
            "no_cache": true,
        }),
    )
}

/// Run `fallow_api::run_audit` in this process against the base commit.
///
/// # Panics
///
/// Panics when the programmatic run fails.
pub fn api_audit(root: &Path) -> Value {
    let options = fallow_api::AuditOptions {
        analysis: fallow_api::AnalysisOptions {
            root: Some(root.to_path_buf()),
            no_cache: true,
            explain: true,
            ..fallow_api::AnalysisOptions::default()
        },
        base: Some(AUDIT_BASE_REF.to_string()),
        gate: fallow_api::AuditGate::NewOnly,
        min_invocations_hot: 100,
        ..fallow_api::AuditOptions::default()
    };
    fallow_api::run_audit(&options)
        .and_then(fallow_api::serialize_audit_programmatic_json)
        .unwrap_or_else(|err| panic!("fallow_api audit failed: {err:?}"))
}
