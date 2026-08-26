//! MCP step: register `fallow-mcp` with each harness.
//!
//! The launcher is resolved before anything is written: the project's own
//! `node_modules/.bin/fallow-mcp`, then `fallow-mcp` on `PATH`, then the
//! running binary when it answers `mcp-server --version` (the npm multicall
//! build). Only the last one is executed as a probe; the first two are
//! resolved on disk. A launcher that would only work on this machine is never
//! written into a file the team commits.
//!
//! JSON cannot carry a marker comment, so ownership of the `fallow` entry is
//! recognized by shape: an entry whose command and args match one of the
//! launchers fallow writes, with no extra keys, is fallow-managed. Anything
//! else is foreign and is neither replaced nor removed without `--force`.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde::Serialize;

use super::{Ctx, Harness, Mode, Reason, Scope, Step, StepReport, StepStatus};
use crate::setup_hooks::read_optional_text;

const SERVER_KEY: &str = "fallow";
const CLAUDE_PROJECT_FILE: &str = ".mcp.json";
const CLAUDE_LOCAL_SETTINGS: &str = ".claude/settings.local.json";
const CURSOR_FILE: &str = ".cursor/mcp.json";
const CODEX_FILE: &str = ".codex/config.toml";
const BACKUP_SUFFIX: &str = ".fallow-bak";
const SELF_PROBE_TIMEOUT: Duration = Duration::from_secs(5);

/// How the MCP server should be started.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct McpCommand {
    pub command: String,
    pub args: Vec<String>,
    pub source: McpSource,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum McpSource {
    /// `npx --no fallow-mcp` against the project's own `node_modules/.bin`.
    NodeModules,
    /// A `fallow-mcp` executable found on `PATH`.
    Path,
    /// The running binary answers `mcp-server --version` (npm multicall build).
    SelfBinary,
    /// Read back from a config file; provenance unknown.
    Registered,
}

impl McpCommand {
    /// Shell-quoted rendering for next actions. Words with whitespace or
    /// shell-significant characters are double-quoted with `"` and `\`
    /// escaped.
    pub fn shell_words(&self) -> String {
        std::iter::once(self.command.as_str())
            .chain(self.args.iter().map(String::as_str))
            .map(|word| {
                let needs_quotes = word
                    .chars()
                    .any(|c| c.is_whitespace() || matches!(c, '"' | '\\' | '$' | '`' | '\''));
                if needs_quotes {
                    format!("\"{}\"", word.replace('\\', "\\\\").replace('"', "\\\""))
                } else {
                    word.to_string()
                }
            })
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// True when the entry has one of the shapes fallow itself writes.
    pub fn is_fallow_shape(&self) -> bool {
        let base = self
            .command
            .rsplit(['/', '\\'])
            .next()
            .unwrap_or_default()
            .to_ascii_lowercase();
        let base = base
            .strip_suffix(".exe")
            .or_else(|| base.strip_suffix(".cmd"))
            .unwrap_or(&base);
        match (self.command.as_str(), self.args.as_slice()) {
            ("npx", [no, launcher]) => no == "--no" && launcher == "fallow-mcp",
            (_, []) => base == "fallow-mcp",
            (_, [sub]) => sub == "mcp-server" && base == "fallow",
            _ => false,
        }
    }
}

pub fn resolve_command(root: &Path) -> Option<McpCommand> {
    resolve_command_with(
        node_modules_launcher_present(root),
        || find_on_path("fallow-mcp"),
        self_supports_mcp_server,
    )
}

fn node_modules_launcher_present(root: &Path) -> bool {
    let bin = root.join("node_modules").join(".bin");
    let candidates: &[&str] = if cfg!(windows) {
        &["fallow-mcp.cmd", "fallow-mcp"]
    } else {
        &["fallow-mcp"]
    };
    candidates.iter().any(|name| bin.join(name).is_file())
}

pub fn resolve_command_with(
    node_modules_present: bool,
    path_lookup: impl FnOnce() -> Option<PathBuf>,
    self_probe: impl FnOnce() -> Option<PathBuf>,
) -> Option<McpCommand> {
    if node_modules_present {
        return Some(McpCommand {
            command: "npx".to_string(),
            args: vec!["--no".to_string(), "fallow-mcp".to_string()],
            source: McpSource::NodeModules,
        });
    }
    if let Some(found) = path_lookup() {
        let command = if cfg!(windows) {
            found.file_name().map_or_else(
                || "fallow-mcp".to_string(),
                |n| n.to_string_lossy().into_owned(),
            )
        } else {
            "fallow-mcp".to_string()
        };
        return Some(McpCommand {
            command,
            args: Vec::new(),
            source: McpSource::Path,
        });
    }
    self_probe().map(|exe| McpCommand {
        command: exe.display().to_string(),
        args: vec!["mcp-server".to_string()],
        source: McpSource::SelfBinary,
    })
}

fn find_on_path(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    let candidates: &[&str] = if cfg!(windows) {
        &["", ".exe", ".cmd"]
    } else {
        &[""]
    };
    std::env::split_paths(&path).find_map(|dir| {
        candidates
            .iter()
            .map(|ext| dir.join(format!("{name}{ext}")))
            .find(|candidate| candidate.is_file())
    })
}

/// Probe the running binary for the multicall `mcp-server` entry. Bounded by
/// [`SELF_PROBE_TIMEOUT`] so a misbehaving build cannot hang the install;
/// `crates/multicall/tests/server_dispatch.rs` pins that `--version` returns
/// before the stdio handshake.
fn self_supports_mcp_server() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let exe = dunce::canonicalize(&exe).unwrap_or(exe);
    let mut child = Command::new(&exe)
        .args(["mcp-server", "--version"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return status.success().then_some(exe),
            Ok(None) if started.elapsed() < SELF_PROBE_TIMEOUT => {
                std::thread::sleep(Duration::from_millis(20));
            }
            Ok(None) | Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    }
}

/// A machine-local launcher (absolute path to this binary) must not land in
/// a file the team commits. When the binary's directory is on `PATH` the bare
/// file name works for everyone on this machine; otherwise the step is
/// skipped.
fn shared_safe(command: &McpCommand) -> Result<McpCommand, StepReport> {
    if command.source != McpSource::SelfBinary {
        return Ok(command.clone());
    }
    let exe = Path::new(&command.command);
    let dir_on_path = exe.parent().is_some_and(|dir| {
        std::env::var_os("PATH").is_some_and(|path| {
            std::env::split_paths(&path)
                .any(|entry| dunce::canonicalize(&entry).ok().as_deref() == Some(dir))
        })
    });
    if dir_on_path && let Some(name) = exe.file_name() {
        return Ok(McpCommand {
            command: name.to_string_lossy().into_owned(),
            args: command.args.clone(),
            source: McpSource::SelfBinary,
        });
    }
    Err(StepReport::new(None, Step::Mcp, StepStatus::Skipped, Scope::Shared)
        .reason(Reason::MachineLocalLauncher)
        .detail(format!(
            "the only launcher is {}, which would not work for teammates; install fallow through npm (npm i -D fallow) or put fallow-mcp on PATH",
            command.command
        )))
}

pub fn install(ctx: &Ctx, harnesses: &[Harness], command: Option<&McpCommand>) -> Vec<StepReport> {
    harnesses
        .iter()
        .flat_map(|harness| install_for(ctx, *harness, command))
        .collect()
}

pub fn uninstall(ctx: &Ctx, harnesses: &[Harness]) -> Vec<StepReport> {
    harnesses
        .iter()
        .flat_map(|harness| uninstall_for(ctx, *harness))
        .collect()
}

fn target_file(ctx: &Ctx, harness: Harness) -> Result<PathBuf, String> {
    match harness {
        Harness::Claude => Ok(ctx.root.join(CLAUDE_PROJECT_FILE)),
        Harness::Cursor => Ok(ctx.scope_base()?.join(CURSOR_FILE)),
        Harness::Codex => {
            if ctx.user
                && let Some(codex_home) = std::env::var_os("CODEX_HOME")
            {
                return Ok(PathBuf::from(codex_home).join("config.toml"));
            }
            Ok(ctx.scope_base()?.join(CODEX_FILE))
        }
    }
}

fn install_for(ctx: &Ctx, harness: Harness, command: Option<&McpCommand>) -> Vec<StepReport> {
    let path = match target_file(ctx, harness) {
        Ok(path) => path,
        Err(message) => {
            return vec![StepReport::failed(
                Some(harness),
                Step::Mcp,
                Scope::Local,
                message,
            )];
        }
    };
    let Some(command) = command else {
        return vec![
            StepReport::new(Some(harness), Step::Mcp, StepStatus::Skipped, ctx.scope())
                .path(ctx, &path)
                .reason(Reason::McpEntryUnavailable)
                .detail(
                    "no fallow-mcp launcher found: install fallow through npm or put fallow-mcp on PATH",
                ),
        ];
    };
    if harness == Harness::Claude && ctx.user {
        return vec![manual_claude_user_step(ctx, Mode::Install).detail(format!(
            "fallow does not edit ~/.claude.json; run: claude mcp add --scope user {SERVER_KEY} -- {}",
            command.shell_words()
        ))];
    }
    let command = if ctx.scope() == Scope::Shared {
        match shared_safe(command) {
            Ok(command) => command,
            Err(skipped) => {
                let mut skipped = skipped.path(ctx, &path);
                skipped.harness = Some(harness);
                return vec![skipped];
            }
        }
    } else {
        command.clone()
    };
    match harness {
        Harness::Claude => {
            let entry = serde_json::json!({
                "type": "stdio",
                "command": command.command,
                "args": command.args,
            });
            vec![
                json_step(ctx, harness, &path, Scope::Shared, Some(&entry)),
                approval_step(ctx, Mode::Install),
            ]
        }
        Harness::Cursor => {
            let entry = serde_json::json!({
                "command": command.command,
                "args": command.args,
            });
            vec![json_step(ctx, harness, &path, ctx.scope(), Some(&entry))]
        }
        Harness::Codex => vec![codex_step(ctx, &path, Some(&command))],
    }
}

fn uninstall_for(ctx: &Ctx, harness: Harness) -> Vec<StepReport> {
    let path = match target_file(ctx, harness) {
        Ok(path) => path,
        Err(message) => {
            return vec![StepReport::failed(
                Some(harness),
                Step::Mcp,
                Scope::Local,
                message,
            )];
        }
    };
    match harness {
        Harness::Claude => {
            if ctx.user {
                return vec![manual_claude_user_step(ctx, Mode::Uninstall)];
            }
            vec![
                json_step(ctx, harness, &path, Scope::Shared, None),
                approval_step(ctx, Mode::Uninstall),
            ]
        }
        Harness::Cursor => vec![json_step(ctx, harness, &path, ctx.scope(), None)],
        Harness::Codex => vec![codex_step(ctx, &path, None)],
    }
}

fn manual_claude_user_step(ctx: &Ctx, mode: Mode) -> StepReport {
    let verb = match mode {
        Mode::Install => "add",
        Mode::Uninstall => "remove",
    };
    StepReport::new(
        Some(Harness::Claude),
        Step::Mcp,
        StepStatus::Skipped,
        Scope::Local,
    )
    .reason(Reason::ManualCommand)
    .detail(format!(
        "fallow does not edit ~/.claude.json; run `claude mcp {verb} --scope user {SERVER_KEY}`{}",
        if ctx.dry_run { " (dry run)" } else { "" }
    ))
}

fn outcome_report(
    base: StepReport,
    outcome: Result<FileOutcome, String>,
    ctx: &Ctx,
    path: &Path,
    invalid: Reason,
) -> StepReport {
    let harness = base.harness;
    let scope = base.scope;
    let format_name = if invalid == Reason::InvalidToml {
        "TOML"
    } else {
        "JSON"
    };
    match outcome {
        Ok(FileOutcome::Changed) => base,
        Ok(FileOutcome::ChangedAfterBackup(backup)) => base.detail(format!(
            "previous contents were not parseable and were saved to {}",
            super::display_path(&ctx.root, ctx.home.as_deref(), &backup)
        )),
        Ok(FileOutcome::Deleted) => base.detail("file removed (nothing else was left in it)"),
        Ok(FileOutcome::Unchanged) => base.with_status(StepStatus::Unchanged),
        Ok(FileOutcome::NotPresent) => base
            .with_status(StepStatus::Unchanged)
            .detail("not present"),
        Ok(FileOutcome::Foreign) => {
            let status = if base.status == StepStatus::Removed {
                StepStatus::Skipped
            } else {
                StepStatus::Refused
            };
            base.with_status(status)
                .reason(Reason::McpEntryForeign)
                .detail("the fallow entry here was not written by fallow; pass --force to replace it")
        }
        Ok(FileOutcome::InvalidPreserved) => base
            .with_status(StepStatus::Refused)
            .reason(invalid)
            .detail(format!(
                "existing file is not valid {format_name}; fix it or pass --force to rewrite it (the old contents are saved next to it)"
            )),
        Ok(FileOutcome::NotAnObject) => base
            .with_status(StepStatus::Refused)
            .reason(Reason::NotAnObject)
            .detail("top level of the file is not a JSON object; fix it or pass --force to rewrite it"),
        Err(message) => StepReport::failed(harness, Step::Mcp, scope, message).path(ctx, path),
    }
}

/// Merge or remove `mcpServers.fallow` in a JSON config file.
fn json_step(
    ctx: &Ctx,
    harness: Harness,
    path: &Path,
    scope: Scope,
    entry: Option<&serde_json::Value>,
) -> StepReport {
    let status = if entry.is_some() {
        StepStatus::Written
    } else {
        StepStatus::Removed
    };
    let base = StepReport::new(Some(harness), Step::Mcp, status, scope).path(ctx, path);
    let outcome = merge_json_server(path, "mcpServers", entry, ctx.dry_run, ctx.force);
    outcome_report(base, outcome, ctx, path, Reason::InvalidJson)
}

/// Opt-in pre-approval of the project-scoped server for this user.
fn approval_step(ctx: &Ctx, mode: Mode) -> StepReport {
    let path = ctx.root.join(CLAUDE_LOCAL_SETTINGS);
    let base = StepReport::new(
        Some(Harness::Claude),
        Step::Mcp,
        StepStatus::Written,
        Scope::Local,
    )
    .path(ctx, &path);
    if mode == Mode::Install && !ctx.approve {
        return base
            .with_status(StepStatus::Skipped)
            .reason(Reason::ApprovalNotRequested)
            .detail("pass --approve to pre-approve the project MCP server for yourself");
    }
    if is_git_tracked(&ctx.root, CLAUDE_LOCAL_SETTINGS) {
        return base
            .with_status(StepStatus::Skipped)
            .reason(Reason::SettingsLocalTracked)
            .detail("file is tracked by git, so an approval written there would be shared; untrack it first");
    }
    let base = if mode == Mode::Uninstall {
        base.with_status(StepStatus::Removed)
    } else {
        base
    };
    match merge_enabled_server(&path, mode == Mode::Install, ctx.dry_run, ctx.force) {
        Ok(ApprovalOutcome::File(FileOutcome::Changed)) => base.detail("enabledMcpjsonServers"),
        Ok(ApprovalOutcome::ClearedRejection) => base.detail(
            "enabledMcpjsonServers (also cleared an earlier rejection in disabledMcpjsonServers)",
        ),
        Ok(ApprovalOutcome::File(other)) => {
            outcome_report(base, Ok(other), ctx, &path, Reason::InvalidJson)
        }
        Err(message) => StepReport::failed(Some(Harness::Claude), Step::Mcp, Scope::Local, message)
            .path(ctx, &path),
    }
}

fn codex_step(ctx: &Ctx, path: &Path, command: Option<&McpCommand>) -> StepReport {
    let status = if command.is_some() {
        StepStatus::Written
    } else {
        StepStatus::Removed
    };
    let base =
        StepReport::new(Some(Harness::Codex), Step::Mcp, status, ctx.scope()).path(ctx, path);
    let base = if command.is_some() && !ctx.user {
        base.detail(
            "applies once Codex trusts this project; the codex mcp add next action works immediately",
        )
    } else {
        base
    };
    let outcome = merge_codex(path, command, ctx.dry_run, ctx.force);
    outcome_report(base, outcome, ctx, path, Reason::InvalidToml)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FileOutcome {
    Changed,
    /// The file was unparsable, `--force` was given, and the old bytes were
    /// saved to the returned path before the rewrite.
    ChangedAfterBackup(PathBuf),
    /// The removal left nothing behind, so the file itself was deleted.
    Deleted,
    Unchanged,
    NotPresent,
    /// A `fallow` entry exists that fallow did not write.
    Foreign,
    InvalidPreserved,
    NotAnObject,
}

enum ApprovalOutcome {
    File(FileOutcome),
    ClearedRejection,
}

/// Formatting of an existing JSON file, so an edit does not reflow the
/// whole document: indentation of the first indented line and whether the
/// file ended with a newline.
struct JsonStyle {
    indent: Vec<u8>,
    trailing_newline: bool,
}

impl JsonStyle {
    fn detect(text: &str) -> Self {
        let indent = text
            .lines()
            .find_map(|line| {
                let ws: String = line
                    .chars()
                    .take_while(|c| *c == ' ' || *c == '\t')
                    .collect();
                (!ws.is_empty() && ws.len() < line.len()).then_some(ws)
            })
            .unwrap_or_else(|| "  ".to_string());
        Self {
            indent: indent.into_bytes(),
            trailing_newline: text.is_empty() || text.ends_with('\n'),
        }
    }

    const fn default_style() -> Self {
        Self {
            indent: Vec::new(),
            trailing_newline: true,
        }
    }

    fn render(&self, value: &serde_json::Value) -> Result<String, String> {
        let indent: &[u8] = if self.indent.is_empty() {
            b"  "
        } else {
            &self.indent
        };
        let mut buf = Vec::new();
        let formatter = serde_json::ser::PrettyFormatter::with_indent(indent);
        let mut serializer = serde_json::Serializer::with_formatter(&mut buf, formatter);
        serde::Serialize::serialize(value, &mut serializer)
            .map_err(|e| format!("serialize JSON: {e}"))?;
        let mut text = String::from_utf8(buf).map_err(|e| format!("serialize JSON: {e}"))?;
        if self.trailing_newline {
            text.push('\n');
        }
        Ok(text)
    }
}

fn parse_json_object(
    path: &Path,
    existing: &str,
    force: bool,
    removing: bool,
    dry_run: bool,
) -> Result<Result<(serde_json::Value, Option<PathBuf>), FileOutcome>, String> {
    match serde_json::from_str::<serde_json::Value>(existing) {
        Ok(value) if value.is_object() => Ok(Ok((value, None))),
        Ok(_) if !force => Ok(Err(FileOutcome::NotAnObject)),
        Err(_) if !force => Ok(Err(FileOutcome::InvalidPreserved)),
        _ if removing => Ok(Err(FileOutcome::InvalidPreserved)),
        _ => {
            let backup = backup_path(path);
            if !dry_run {
                std::fs::write(&backup, existing)
                    .map_err(|e| format!("write backup {}: {e}", backup.display()))?;
            }
            Ok(Ok((serde_json::json!({}), Some(backup))))
        }
    }
}

fn backup_path(path: &Path) -> PathBuf {
    let mut name = path
        .file_name()
        .map(|n| n.to_os_string())
        .unwrap_or_default();
    name.push(BACKUP_SUFFIX);
    path.with_file_name(name)
}

fn entry_is_fallow_shape(entry: &serde_json::Value, allow_type: bool) -> bool {
    let Some(object) = entry.as_object() else {
        return false;
    };
    let known = ["command", "args", "type"];
    if object.keys().any(|key| !known.contains(&key.as_str())) {
        return false;
    }
    if let Some(kind) = object.get("type")
        && (!allow_type || kind.as_str() != Some("stdio"))
    {
        return false;
    }
    let Some(command) = object.get("command").and_then(serde_json::Value::as_str) else {
        return false;
    };
    let args: Vec<String> = object
        .get("args")
        .and_then(serde_json::Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(serde_json::Value::as_str)
                .map(ToOwned::to_owned)
                .collect()
        })
        .unwrap_or_default();
    McpCommand {
        command: command.to_string(),
        args,
        source: McpSource::Registered,
    }
    .is_fallow_shape()
}

/// Set or remove `<section>.fallow` in a JSON object file, preserving every
/// other key, the key order, and the file's indentation.
pub fn merge_json_server(
    path: &Path,
    section: &str,
    entry: Option<&serde_json::Value>,
    dry_run: bool,
    force: bool,
) -> Result<FileOutcome, String> {
    let existing = read_optional_text(path).map_err(|e| format!("read {}: {e}", path.display()))?;
    let Some(existing) = existing else {
        let Some(entry) = entry else {
            return Ok(FileOutcome::NotPresent);
        };
        let value = serde_json::json!({ section: { SERVER_KEY: entry } });
        return write_json(path, &value, &JsonStyle::default_style(), dry_run)
            .map(|()| FileOutcome::Changed);
    };
    let style = JsonStyle::detect(&existing);
    let (mut value, backup) =
        match parse_json_object(path, &existing, force, entry.is_none(), dry_run)? {
            Ok(parsed) => parsed,
            Err(outcome) => return Ok(outcome),
        };
    let before = style.render(&value)?;
    let Some(object) = value.as_object_mut() else {
        return Ok(FileOutcome::NotAnObject);
    };
    let current = object
        .get(section)
        .and_then(|servers| servers.get(SERVER_KEY));
    if let Some(current) = current
        && !force
        && !entry_is_fallow_shape(current, true)
    {
        return Ok(FileOutcome::Foreign);
    }
    match entry {
        Some(entry) => {
            let servers = object
                .entry(section)
                .or_insert_with(|| serde_json::Value::Object(serde_json::Map::new()));
            if !servers.is_object() {
                *servers = serde_json::Value::Object(serde_json::Map::new());
            }
            if let Some(servers) = servers.as_object_mut() {
                servers.insert(SERVER_KEY.to_string(), entry.clone());
            }
        }
        None => {
            let Some(servers) = object
                .get_mut(section)
                .and_then(serde_json::Value::as_object_mut)
            else {
                return Ok(FileOutcome::Unchanged);
            };
            if servers.remove(SERVER_KEY).is_none() {
                return Ok(FileOutcome::Unchanged);
            }
            let nothing_left = servers.is_empty() && object.len() == 1;
            if nothing_left {
                return delete_file(path, dry_run).map(|()| FileOutcome::Deleted);
            }
        }
    }
    let after = style.render(&value)?;
    if after == before && backup.is_none() {
        return Ok(FileOutcome::Unchanged);
    }
    write_json(path, &value, &style, dry_run)?;
    Ok(backup.map_or(FileOutcome::Changed, FileOutcome::ChangedAfterBackup))
}

/// Add or remove `fallow` in `enabledMcpjsonServers` of a Claude settings
/// file. Enabling also clears an earlier rejection from
/// `disabledMcpjsonServers`, which would otherwise outrank the approval.
fn merge_enabled_server(
    path: &Path,
    enable: bool,
    dry_run: bool,
    force: bool,
) -> Result<ApprovalOutcome, String> {
    let existing = read_optional_text(path).map_err(|e| format!("read {}: {e}", path.display()))?;
    let Some(existing) = existing else {
        if !enable {
            return Ok(ApprovalOutcome::File(FileOutcome::NotPresent));
        }
        let value = serde_json::json!({ "enabledMcpjsonServers": [SERVER_KEY] });
        return write_json(path, &value, &JsonStyle::default_style(), dry_run)
            .map(|()| ApprovalOutcome::File(FileOutcome::Changed));
    };
    let style = JsonStyle::detect(&existing);
    let (mut value, backup) = match parse_json_object(path, &existing, force, !enable, dry_run)? {
        Ok(parsed) => parsed,
        Err(outcome) => return Ok(ApprovalOutcome::File(outcome)),
    };
    let before = style.render(&value)?;
    let Some(object) = value.as_object_mut() else {
        return Ok(ApprovalOutcome::File(FileOutcome::NotAnObject));
    };
    let cleared_rejection = if enable
        && let Some(disabled) = object
            .get_mut("disabledMcpjsonServers")
            .and_then(serde_json::Value::as_array_mut)
    {
        let len = disabled.len();
        disabled.retain(|item| item.as_str() != Some(SERVER_KEY));
        disabled.len() != len
    } else {
        false
    };
    let list = object
        .entry("enabledMcpjsonServers")
        .or_insert_with(|| serde_json::Value::Array(Vec::new()));
    if !list.is_array() {
        *list = serde_json::Value::Array(Vec::new());
    }
    if let Some(items) = list.as_array_mut() {
        let present = items.iter().any(|item| item.as_str() == Some(SERVER_KEY));
        match (enable, present) {
            (true, false) => items.push(serde_json::Value::String(SERVER_KEY.to_string())),
            (false, true) => items.retain(|item| item.as_str() != Some(SERVER_KEY)),
            _ => {}
        }
    }
    if !enable {
        let only_empty_list = object.len() == 1
            && object
                .get("enabledMcpjsonServers")
                .and_then(serde_json::Value::as_array)
                .is_some_and(Vec::is_empty);
        if object.is_empty() || only_empty_list {
            return delete_file(path, dry_run)
                .map(|()| ApprovalOutcome::File(FileOutcome::Deleted));
        }
    }
    let after = style.render(&value)?;
    if after == before && backup.is_none() {
        return Ok(ApprovalOutcome::File(FileOutcome::Unchanged));
    }
    write_json(path, &value, &style, dry_run)?;
    if cleared_rejection {
        return Ok(ApprovalOutcome::ClearedRejection);
    }
    Ok(ApprovalOutcome::File(backup.map_or(
        FileOutcome::Changed,
        FileOutcome::ChangedAfterBackup,
    )))
}

fn write_json(
    path: &Path,
    value: &serde_json::Value,
    style: &JsonStyle,
    dry_run: bool,
) -> Result<(), String> {
    let text = style.render(value)?;
    if dry_run {
        return Ok(());
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("create {}: {e}", parent.display()))?;
    }
    std::fs::write(path, text).map_err(|e| format!("write {}: {e}", path.display()))
}

/// Delete a file that an uninstall emptied, then the `.cursor` or `.codex`
/// directory when nothing else is left in it (both are detection evidence).
fn delete_file(path: &Path, dry_run: bool) -> Result<(), String> {
    if dry_run {
        return Ok(());
    }
    std::fs::remove_file(path).map_err(|e| format!("remove {}: {e}", path.display()))?;
    if let Some(parent) = path.parent()
        && parent
            .file_name()
            .is_some_and(|name| name == ".cursor" || name == ".codex")
    {
        let _ = std::fs::remove_dir(parent);
    }
    Ok(())
}

fn codex_entry_is_fallow_shape(entry: &toml_edit::Item) -> bool {
    let Some(table) = entry.as_table_like() else {
        return false;
    };
    if table
        .iter()
        .any(|(key, _)| key != "command" && key != "args")
    {
        return false;
    }
    let Some(command) = table.get("command").and_then(toml_edit::Item::as_str) else {
        return false;
    };
    let args: Vec<String> = table
        .get("args")
        .and_then(toml_edit::Item::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(toml_edit::Value::as_str)
                .map(ToOwned::to_owned)
                .collect()
        })
        .unwrap_or_default();
    McpCommand {
        command: command.to_string(),
        args,
        source: McpSource::Registered,
    }
    .is_fallow_shape()
}

/// Set or remove `[mcp_servers.fallow]` in a Codex `config.toml`, keeping
/// formatting and comments of everything else.
pub fn merge_codex(
    path: &Path,
    command: Option<&McpCommand>,
    dry_run: bool,
    force: bool,
) -> Result<FileOutcome, String> {
    let existing = read_optional_text(path).map_err(|e| format!("read {}: {e}", path.display()))?;
    let (mut doc, before, backup) = match existing {
        None => {
            if command.is_none() {
                return Ok(FileOutcome::NotPresent);
            }
            (toml_edit::DocumentMut::new(), String::new(), None)
        }
        Some(text) => match text.parse::<toml_edit::DocumentMut>() {
            Ok(doc) => (doc, text, None),
            Err(_) if force && command.is_some() => {
                let backup = backup_path(path);
                if !dry_run {
                    std::fs::write(&backup, &text)
                        .map_err(|e| format!("write backup {}: {e}", backup.display()))?;
                }
                (toml_edit::DocumentMut::new(), text, Some(backup))
            }
            Err(_) => return Ok(FileOutcome::InvalidPreserved),
        },
    };

    if let Some(current) = doc
        .get("mcp_servers")
        .and_then(|servers| servers.get(SERVER_KEY))
        && !force
        && !codex_entry_is_fallow_shape(current)
    {
        return Ok(FileOutcome::Foreign);
    }

    match command {
        Some(command) => {
            let servers = doc
                .entry("mcp_servers")
                .or_insert_with(|| toml_edit::Item::Table(toml_edit::Table::new()));
            if !servers.is_table() {
                *servers = toml_edit::Item::Table(toml_edit::Table::new());
            }
            let Some(servers) = servers.as_table_mut() else {
                return Err(format!("{}: mcp_servers is not a table", path.display()));
            };
            servers.set_implicit(true);
            let mut entry = toml_edit::Table::new();
            entry.insert("command", toml_edit::value(command.command.as_str()));
            let mut args = toml_edit::Array::new();
            for arg in &command.args {
                args.push(arg.as_str());
            }
            entry.insert("args", toml_edit::value(args));
            servers.insert(SERVER_KEY, toml_edit::Item::Table(entry));
        }
        None => {
            let Some(servers) = doc
                .get_mut("mcp_servers")
                .and_then(toml_edit::Item::as_table_mut)
            else {
                return Ok(FileOutcome::Unchanged);
            };
            if servers.remove(SERVER_KEY).is_none() {
                return Ok(FileOutcome::Unchanged);
            }
            if servers.is_empty() {
                doc.remove("mcp_servers");
            }
            if doc.to_string().trim().is_empty() {
                return delete_file(path, dry_run).map(|()| FileOutcome::Deleted);
            }
        }
    }

    let after = doc.to_string();
    if after == before && backup.is_none() {
        return Ok(FileOutcome::Unchanged);
    }
    if dry_run {
        return Ok(backup.map_or(FileOutcome::Changed, FileOutcome::ChangedAfterBackup));
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("create {}: {e}", parent.display()))?;
    }
    std::fs::write(path, after).map_err(|e| format!("write {}: {e}", path.display()))?;
    Ok(backup.map_or(FileOutcome::Changed, FileOutcome::ChangedAfterBackup))
}

fn is_git_tracked(root: &Path, relative: &str) -> bool {
    Command::new("git")
        .args(["ls-files", "--error-unmatch", relative])
        .current_dir(root)
        .output()
        .is_ok_and(|output| output.status.success())
}

/// True when a `.mcp.json` declares at least one server, so an emptied file
/// left by an uninstall does not count as harness evidence.
pub fn mcp_json_has_servers(path: &Path) -> bool {
    read_optional_text(path)
        .ok()
        .flatten()
        .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
        .and_then(|value| {
            value
                .get("mcpServers")
                .and_then(serde_json::Value::as_object)
                .map(|servers| !servers.is_empty())
        })
        .unwrap_or(false)
}

/// Read the registered command for a harness, if any, for `agent status`.
pub fn registered_command(path: &Path, harness: Harness) -> Option<McpCommand> {
    let text = read_optional_text(path).ok().flatten()?;
    match harness {
        Harness::Claude | Harness::Cursor => {
            let value: serde_json::Value = serde_json::from_str(&text).ok()?;
            let entry = value.get("mcpServers")?.get(SERVER_KEY)?;
            let command = entry.get("command")?.as_str()?.to_string();
            let args = entry
                .get("args")
                .and_then(serde_json::Value::as_array)
                .map(|items| {
                    items
                        .iter()
                        .filter_map(serde_json::Value::as_str)
                        .map(ToOwned::to_owned)
                        .collect()
                })
                .unwrap_or_default();
            let source = if entry_is_fallow_shape(entry, harness == Harness::Claude) {
                McpSource::Registered
            } else {
                McpSource::Path
            };
            Some(McpCommand {
                command,
                args,
                source,
            })
        }
        Harness::Codex => {
            let doc = text.parse::<toml_edit::DocumentMut>().ok()?;
            let entry = doc.get("mcp_servers")?.get(SERVER_KEY)?;
            let command = entry.get("command")?.as_str()?.to_string();
            let args = entry
                .get("args")
                .and_then(toml_edit::Item::as_array)
                .map(|items| {
                    items
                        .iter()
                        .filter_map(toml_edit::Value::as_str)
                        .map(ToOwned::to_owned)
                        .collect()
                })
                .unwrap_or_default();
            let source = if codex_entry_is_fallow_shape(entry) {
                McpSource::Registered
            } else {
                McpSource::Path
            };
            Some(McpCommand {
                command,
                args,
                source,
            })
        }
    }
}

/// Whether a registered entry read by [`registered_command`] has a shape
/// fallow writes (`true`) or was written by someone else (`false`).
pub fn registered_is_managed(command: &McpCommand) -> bool {
    command.source == McpSource::Registered
}

pub const fn claude_project_file() -> &'static str {
    CLAUDE_PROJECT_FILE
}

pub const fn cursor_file() -> &'static str {
    CURSOR_FILE
}

pub const fn codex_file() -> &'static str {
    CODEX_FILE
}
