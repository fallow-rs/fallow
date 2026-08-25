//! MCP step: register `fallow-mcp` with each harness.
//!
//! The command written is probed first so a config that cannot start is
//! never written: the project-pinned npm launcher, then `fallow-mcp` on
//! `PATH`, then the running binary when it is the multicall build.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Serialize;

use super::{Ctx, Harness, Mode, Scope, Step, StepReport, StepStatus};
use crate::setup_hooks::read_optional_text;

const SERVER_KEY: &str = "fallow";
const CLAUDE_PROJECT_FILE: &str = ".mcp.json";
const CLAUDE_LOCAL_SETTINGS: &str = ".claude/settings.local.json";
const CURSOR_FILE: &str = ".cursor/mcp.json";
const CODEX_FILE: &str = ".codex/config.toml";

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
    /// `npx --no fallow-mcp` against the project's own `node_modules/fallow`.
    NodeModules,
    /// A `fallow-mcp` executable found on `PATH`.
    Path,
    /// The running binary answers `mcp-server --version` (npm multicall build).
    SelfBinary,
}

impl McpCommand {
    pub fn shell_words(&self) -> String {
        std::iter::once(self.command.as_str())
            .chain(self.args.iter().map(String::as_str))
            .map(|word| {
                if word.chars().any(char::is_whitespace) {
                    format!("\"{word}\"")
                } else {
                    word.to_string()
                }
            })
            .collect::<Vec<_>>()
            .join(" ")
    }
}

pub fn resolve_command(root: &Path) -> Option<McpCommand> {
    resolve_command_with(
        root.join("node_modules")
            .join("fallow")
            .join("package.json")
            .is_file(),
        || find_on_path("fallow-mcp"),
        self_supports_mcp_server,
    )
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
    if path_lookup().is_some() {
        return Some(McpCommand {
            command: "fallow-mcp".to_string(),
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

fn self_supports_mcp_server() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let exe = dunce::canonicalize(&exe).unwrap_or(exe);
    let output = Command::new(&exe)
        .args(["mcp-server", "--version"])
        .output()
        .ok()?;
    output.status.success().then_some(exe)
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

fn install_for(ctx: &Ctx, harness: Harness, command: Option<&McpCommand>) -> Vec<StepReport> {
    let Some(command) = command else {
        let relative = match harness {
            Harness::Claude => CLAUDE_PROJECT_FILE,
            Harness::Codex => CODEX_FILE,
            Harness::Cursor => CURSOR_FILE,
        };
        let base = if harness == Harness::Claude {
            ctx.root.clone()
        } else {
            ctx.scope_base()
                .map_or_else(|_| ctx.root.clone(), Path::to_path_buf)
        };
        return vec![
            StepReport::new(Some(harness), Step::Mcp, StepStatus::Skipped, ctx.scope())
                .path(&ctx.root, &base.join(relative))
                .reason("mcp_entry_unavailable")
                .detail(
                    "no fallow-mcp launcher found: install fallow through npm or put fallow-mcp on PATH",
                ),
        ];
    };
    match harness {
        Harness::Claude => install_claude(ctx, command),
        Harness::Codex => vec![install_codex(ctx, command)],
        Harness::Cursor => vec![install_cursor(ctx, command)],
    }
}

fn uninstall_for(ctx: &Ctx, harness: Harness) -> Vec<StepReport> {
    match harness {
        Harness::Claude => {
            if ctx.user {
                return vec![manual_claude_user_step(ctx, Mode::Uninstall)];
            }
            let mut steps = vec![json_step(
                ctx,
                harness,
                &ctx.root.join(CLAUDE_PROJECT_FILE),
                Scope::Shared,
                None,
            )];
            steps.push(approval_step(ctx, Mode::Uninstall));
            steps
        }
        Harness::Codex => vec![codex_step(ctx, None)],
        Harness::Cursor => {
            let base = match ctx.scope_base() {
                Ok(base) => base.to_path_buf(),
                Err(message) => {
                    return vec![StepReport::failed(
                        Some(harness),
                        Step::Mcp,
                        Scope::Local,
                        message,
                    )];
                }
            };
            vec![json_step(
                ctx,
                harness,
                &base.join(CURSOR_FILE),
                ctx.scope(),
                None,
            )]
        }
    }
}

fn install_claude(ctx: &Ctx, command: &McpCommand) -> Vec<StepReport> {
    if ctx.user {
        return vec![manual_claude_user_step(ctx, Mode::Install).detail(format!(
            "run: claude mcp add --scope user {SERVER_KEY} -- {}",
            command.shell_words()
        ))];
    }
    let entry = serde_json::json!({
        "type": "stdio",
        "command": command.command,
        "args": command.args,
    });
    let mut steps = vec![json_step(
        ctx,
        Harness::Claude,
        &ctx.root.join(CLAUDE_PROJECT_FILE),
        Scope::Shared,
        Some(&entry),
    )];
    steps.push(approval_step(ctx, Mode::Install));
    steps
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
    .reason("manual_command")
    .detail(format!(
        "fallow does not edit ~/.claude.json; run `claude mcp {verb} --scope user {SERVER_KEY}`{}",
        if ctx.dry_run { " (dry run)" } else { "" }
    ))
}

fn install_cursor(ctx: &Ctx, command: &McpCommand) -> StepReport {
    let base = match ctx.scope_base() {
        Ok(base) => base.to_path_buf(),
        Err(message) => {
            return StepReport::failed(Some(Harness::Cursor), Step::Mcp, Scope::Local, message);
        }
    };
    let entry = serde_json::json!({
        "command": command.command,
        "args": command.args,
    });
    json_step(
        ctx,
        Harness::Cursor,
        &base.join(CURSOR_FILE),
        ctx.scope(),
        Some(&entry),
    )
}

fn install_codex(ctx: &Ctx, command: &McpCommand) -> StepReport {
    codex_step(ctx, Some(command))
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
    let base = StepReport::new(Some(harness), Step::Mcp, status, scope).path(&ctx.root, path);
    match merge_json_server(path, "mcpServers", entry, ctx.dry_run, ctx.force) {
        Ok(FileOutcome::Changed) => base,
        Ok(FileOutcome::Unchanged) => base.with_status(StepStatus::Unchanged),
        Ok(FileOutcome::NotPresent) => base
            .with_status(StepStatus::Unchanged)
            .detail("not present"),
        Ok(FileOutcome::InvalidPreserved) => base
            .with_status(StepStatus::Refused)
            .reason("invalid_json")
            .detail("existing file is not valid JSON; fix it or pass --force to rewrite it"),
        Err(message) => {
            StepReport::failed(Some(harness), Step::Mcp, scope, message).path(&ctx.root, path)
        }
    }
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
    .path(&ctx.root, &path);
    if mode == Mode::Install && !ctx.approve {
        return base
            .with_status(StepStatus::Skipped)
            .reason("approval_not_requested")
            .detail("pass --approve to pre-approve the project MCP server for yourself");
    }
    if is_git_tracked(&ctx.root, CLAUDE_LOCAL_SETTINGS) {
        return base
            .with_status(StepStatus::Skipped)
            .reason("settings_local_tracked")
            .detail("file is tracked by git, so an approval written there would be shared; untrack it first");
    }
    let outcome = match mode {
        Mode::Install => merge_enabled_server(&path, true, ctx.dry_run, ctx.force),
        Mode::Uninstall => merge_enabled_server(&path, false, ctx.dry_run, ctx.force),
    };
    let base = if mode == Mode::Uninstall {
        base.with_status(StepStatus::Removed)
    } else {
        base
    };
    match outcome {
        Ok(FileOutcome::Changed) => base.detail("enabledMcpjsonServers"),
        Ok(FileOutcome::Unchanged) => base.with_status(StepStatus::Unchanged),
        Ok(FileOutcome::NotPresent) => base
            .with_status(StepStatus::Unchanged)
            .detail("not present"),
        Ok(FileOutcome::InvalidPreserved) => base
            .with_status(StepStatus::Refused)
            .reason("invalid_json")
            .detail("existing file is not valid JSON; fix it or pass --force to rewrite it"),
        Err(message) => StepReport::failed(Some(Harness::Claude), Step::Mcp, Scope::Local, message)
            .path(&ctx.root, &path),
    }
}

fn codex_step(ctx: &Ctx, command: Option<&McpCommand>) -> StepReport {
    let base_dir = match ctx.scope_base() {
        Ok(base) => base.to_path_buf(),
        Err(message) => {
            return StepReport::failed(Some(Harness::Codex), Step::Mcp, Scope::Local, message);
        }
    };
    let path = base_dir.join(CODEX_FILE);
    let status = if command.is_some() {
        StepStatus::Written
    } else {
        StepStatus::Removed
    };
    let base = StepReport::new(Some(Harness::Codex), Step::Mcp, status, ctx.scope())
        .path(&ctx.root, &path);
    let base = if command.is_some() && !ctx.user {
        base.detail(
            "applies once Codex trusts this project; the codex mcp add next step works immediately",
        )
    } else {
        base
    };
    match merge_codex(&path, command, ctx.dry_run, ctx.force) {
        Ok(FileOutcome::Changed) => base,
        Ok(FileOutcome::Unchanged) => base.with_status(StepStatus::Unchanged),
        Ok(FileOutcome::NotPresent) => base
            .with_status(StepStatus::Unchanged)
            .detail("not present"),
        Ok(FileOutcome::InvalidPreserved) => base
            .with_status(StepStatus::Refused)
            .reason("invalid_toml")
            .detail("existing file is not valid TOML; fix it or pass --force to rewrite it"),
        Err(message) => StepReport::failed(Some(Harness::Codex), Step::Mcp, ctx.scope(), message)
            .path(&ctx.root, &path),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileOutcome {
    Changed,
    Unchanged,
    NotPresent,
    InvalidPreserved,
}

/// Set or remove `<section>.fallow` in a JSON object file, preserving every
/// other key and the key order.
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
        return write_json(path, &value, dry_run).map(|()| FileOutcome::Changed);
    };
    let mut value: serde_json::Value = match serde_json::from_str(&existing) {
        Ok(value) => value,
        Err(_) if force && entry.is_some() => serde_json::json!({}),
        Err(_) if entry.is_none() => return Ok(FileOutcome::Unchanged),
        Err(_) => return Ok(FileOutcome::InvalidPreserved),
    };
    let Some(object) = value.as_object_mut() else {
        if !force {
            return Ok(FileOutcome::InvalidPreserved);
        }
        value = serde_json::json!({});
        return merge_json_into(path, value, section, entry, dry_run);
    };
    let _ = object;
    merge_json_into(path, value, section, entry, dry_run)
}

fn merge_json_into(
    path: &Path,
    mut value: serde_json::Value,
    section: &str,
    entry: Option<&serde_json::Value>,
    dry_run: bool,
) -> Result<FileOutcome, String> {
    let before = serialize_json(&value)?;
    let object = value
        .as_object_mut()
        .ok_or_else(|| format!("{}: top level is not an object", path.display()))?;
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
        }
    }
    let after = serialize_json(&value)?;
    if after == before {
        return Ok(FileOutcome::Unchanged);
    }
    write_json(path, &value, dry_run).map(|()| FileOutcome::Changed)
}

/// Add or remove `fallow` in `enabledMcpjsonServers` of a Claude settings file.
fn merge_enabled_server(
    path: &Path,
    enable: bool,
    dry_run: bool,
    force: bool,
) -> Result<FileOutcome, String> {
    let existing = read_optional_text(path).map_err(|e| format!("read {}: {e}", path.display()))?;
    let Some(existing) = existing else {
        if !enable {
            return Ok(FileOutcome::NotPresent);
        }
        let value = serde_json::json!({ "enabledMcpjsonServers": [SERVER_KEY] });
        return write_json(path, &value, dry_run).map(|()| FileOutcome::Changed);
    };
    let mut value: serde_json::Value = match serde_json::from_str(&existing) {
        Ok(value) => value,
        Err(_) if force && enable => serde_json::json!({}),
        Err(_) if !enable => return Ok(FileOutcome::Unchanged),
        Err(_) => return Ok(FileOutcome::InvalidPreserved),
    };
    if !value.is_object() {
        if !force {
            return Ok(FileOutcome::InvalidPreserved);
        }
        value = serde_json::json!({});
    }
    let before = serialize_json(&value)?;
    let Some(object) = value.as_object_mut() else {
        return Ok(FileOutcome::InvalidPreserved);
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
    let after = serialize_json(&value)?;
    if after == before {
        return Ok(FileOutcome::Unchanged);
    }
    write_json(path, &value, dry_run).map(|()| FileOutcome::Changed)
}

fn serialize_json(value: &serde_json::Value) -> Result<String, String> {
    serde_json::to_string_pretty(value)
        .map(|text| format!("{text}\n"))
        .map_err(|e| format!("serialize JSON: {e}"))
}

fn write_json(path: &Path, value: &serde_json::Value, dry_run: bool) -> Result<(), String> {
    let text = serialize_json(value)?;
    if dry_run {
        return Ok(());
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("create {}: {e}", parent.display()))?;
    }
    std::fs::write(path, text).map_err(|e| format!("write {}: {e}", path.display()))
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
    let (mut doc, before) = match existing {
        None => {
            if command.is_none() {
                return Ok(FileOutcome::NotPresent);
            }
            (toml_edit::DocumentMut::new(), String::new())
        }
        Some(text) => match text.parse::<toml_edit::DocumentMut>() {
            Ok(doc) => (doc, text),
            Err(_) if force && command.is_some() => (toml_edit::DocumentMut::new(), text),
            Err(_) if command.is_none() => return Ok(FileOutcome::Unchanged),
            Err(_) => return Ok(FileOutcome::InvalidPreserved),
        },
    };

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
        }
    }

    let after = doc.to_string();
    if after == before {
        return Ok(FileOutcome::Unchanged);
    }
    if dry_run {
        return Ok(FileOutcome::Changed);
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("create {}: {e}", parent.display()))?;
    }
    std::fs::write(path, after).map_err(|e| format!("write {}: {e}", path.display()))?;
    Ok(FileOutcome::Changed)
}

fn is_git_tracked(root: &Path, relative: &str) -> bool {
    Command::new("git")
        .args(["ls-files", "--error-unmatch", relative])
        .current_dir(root)
        .output()
        .is_ok_and(|output| output.status.success())
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
            Some(McpCommand {
                command,
                args,
                source: McpSource::Path,
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
            Some(McpCommand {
                command,
                args,
                source: McpSource::Path,
            })
        }
    }
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
