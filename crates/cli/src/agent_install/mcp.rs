//! MCP step: register `fallow-mcp` with each harness.
//!
//! The launcher is resolved before anything is written: the project's own
//! `node_modules/.bin/fallow-mcp`, then `fallow-mcp` on `PATH`, then the
//! running binary when `mcp-server --version` prints the MCP server version
//! (the npm multicall build). Only the last one is executed as a probe; the
//! first two are resolved on disk. A launcher that would only work on this machine is never
//! written into a file the team commits.
//!
//! A user-level entry applies in every project. It never uses `npx`, which
//! runs the `fallow-mcp` of the project that the harness opens. It uses the
//! absolute path of the `fallow-mcp` on `PATH`, as found, or of the running
//! binary when that binary serves MCP. A launcher of the current project
//! never counts, see [`crate::setup_hooks::user_launcher`].
//!
//! JSON cannot carry a marker comment, so ownership of the `fallow` entry is
//! recognized by shape: an entry whose command and args match one of the
//! launchers fallow writes, with no extra keys, is fallow-managed. The one
//! extra key fallow writes is an `env` that holds only `FALLOW_MCP_SKILLS=off`.
//! Anything else is foreign and is neither replaced nor removed without
//! `--force`.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde::Serialize;

use super::{Ctx, Harness, Mode, Reason, Scope, Step, StepReport, StepStatus};
use crate::state_file::{Existing, load_existing, read_optional_text};
use fallow_types::mcp_manifest::{MCP_SKILLS_ENV, MCP_SKILLS_OFF};

const SERVER_KEY: &str = "fallow";
const CLAUDE_PROJECT_FILE: &str = ".mcp.json";
const CLAUDE_LOCAL_SETTINGS: &str = ".claude/settings.local.json";
const CURSOR_FILE: &str = ".cursor/mcp.json";
const CODEX_FILE: &str = ".codex/config.toml";
const BACKUP_SUFFIX: &str = ".fallow-bak";
const SELF_PROBE_TIMEOUT: Duration = Duration::from_secs(5);
/// The first word of the `--version` line of the MCP server entry.
const MCP_VERSION_PREFIX: &str = "fallow-mcp ";
/// The version line is short; the probe reads no more than this many bytes.
const PROBE_OUTPUT_LIMIT: u64 = 4096;
const NO_LAUNCHER_DETAIL: &str = "no fallow-mcp launcher found. Install fallow through npm (npm i -D fallow), or put fallow-mcp on PATH. cargo install fallow-mcp and the GitHub release archive both supply fallow-mcp";
/// A user-level entry starts in every project, so it needs a launcher outside
/// each project: a global install.
const NO_USER_LAUNCHER_DETAIL: &str = "no fallow-mcp launcher outside the project found. A user-level entry needs a global install: npm install -g fallow, or cargo install fallow-mcp";

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
    /// The running binary serves `mcp-server` (npm multicall build).
    SelfBinary,
    /// Read back from a config file; provenance unknown.
    Registered,
}

impl McpCommand {
    /// Shell-quoted rendering for next actions. Each word stays one argument
    /// in the shell of this platform, whatever characters its path holds.
    pub fn shell_words(&self) -> String {
        std::iter::once(self.command.as_str())
            .chain(self.args.iter().map(String::as_str))
            .map(fallow_engine::shell_quote::shell_quote)
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

/// The launcher for a project-level entry (`user` false) or a user-level
/// entry (`user` true).
pub fn resolve_command(root: &Path, user: bool) -> Option<McpCommand> {
    if user {
        return resolve_user_command(root);
    }
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

/// The launcher for a user-level entry and for a printed user-scope command
/// of the project at `root`.
pub fn resolve_user_command(root: &Path) -> Option<McpCommand> {
    resolve_user_command_with(
        crate::setup_hooks::user_launcher::search_dirs(),
        &crate::setup_hooks::user_launcher::project_dirs(root),
        || {
            let exe = std::env::current_exe().ok()?;
            serves_mcp(&exe).then_some(exe)
        },
    )
}

/// The user-level launcher: the first `fallow-mcp` in `search_dirs` that
/// [`crate::setup_hooks::user_launcher::is_user_launcher`] accepts, as found, else the
/// running binary when it serves MCP and is accepted too.
pub fn resolve_user_command_with(
    search_dirs: impl IntoIterator<Item = PathBuf>,
    project_dirs: &[PathBuf],
    self_probe: impl FnOnce() -> Option<PathBuf>,
) -> Option<McpCommand> {
    if let Some(found) = crate::setup_hooks::user_launcher::find_user_launcher(
        "fallow-mcp",
        search_dirs,
        project_dirs,
    ) {
        return Some(McpCommand {
            command: found.display().to_string(),
            args: Vec::new(),
            source: McpSource::Path,
        });
    }
    self_probe()
        .filter(|exe| crate::setup_hooks::user_launcher::is_user_launcher(exe, project_dirs))
        .map(|exe| McpCommand {
            command: exe.display().to_string(),
            args: vec!["mcp-server".to_string()],
            source: McpSource::SelfBinary,
        })
}

/// Resolve a bare command name against `PATH`, the way the agent gate does.
pub(super) fn find_on_path(name: &str) -> Option<PathBuf> {
    let candidates: &[&str] = if cfg!(windows) {
        &["", ".exe", ".cmd"]
    } else {
        &[""]
    };
    fallow_process::HostPaths::from_env()
        .search_dirs()
        .find_map(|dir| {
            candidates
                .iter()
                .map(|ext| dir.join(format!("{name}{ext}")))
                .find(|candidate| candidate.is_file())
        })
}

fn self_supports_mcp_server() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let exe = dunce::canonicalize(&exe).unwrap_or(exe);
    serves_mcp(&exe).then_some(exe)
}

/// True when `exe mcp-server --version` reaches the MCP server entry of the
/// multicall build. That entry prints [`MCP_VERSION_PREFIX`]. The plain CLI
/// build has no `mcp-server` subcommand, but clap still answers `--version`
/// with `fallow <version>` and exit code 0, so the exit code alone proves
/// nothing. Bounded by [`SELF_PROBE_TIMEOUT`] so a misbehaving build cannot
/// hang the install; `crates/multicall/tests/integration/server_dispatch.rs`
/// pins that `--version` returns before the stdio handshake.
fn serves_mcp(exe: &Path) -> bool {
    let Ok(mut child) = Command::new(exe)
        .args(["mcp-server", "--version"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
    else {
        return false;
    };
    let Some(stdout) = child.stdout.take() else {
        let _ = child.kill();
        let _ = child.wait();
        return false;
    };
    // A reader thread drains stdout, so a child that writes much output
    // cannot block on a full pipe while this thread polls for its exit.
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut text = String::new();
        let _ = stdout.take(PROBE_OUTPUT_LIMIT).read_to_string(&mut text);
        let _ = tx.send(text);
    });
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                if !status.success() {
                    return false;
                }
                let remaining = SELF_PROBE_TIMEOUT.saturating_sub(started.elapsed());
                return rx
                    .recv_timeout(remaining)
                    .is_ok_and(|text| text.starts_with(MCP_VERSION_PREFIX));
            }
            Ok(None) if started.elapsed() < SELF_PROBE_TIMEOUT => {
                std::thread::sleep(Duration::from_millis(20));
            }
            Ok(None) | Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                return false;
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
        fallow_process::HostPaths::from_env()
            .search_dirs()
            .any(|entry| dunce::canonicalize(&entry).ok().as_deref() == Some(dir))
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

/// Register the server with each harness.
///
/// With `skills_on_disk`, the same run also installs the released skills on
/// disk. The entry then sets [`MCP_SKILLS_ENV`] to [`MCP_SKILLS_OFF`], so a
/// client that loads skills from MCP servers does not show each skill twice.
pub fn install(
    ctx: &Ctx,
    harnesses: &[Harness],
    command: Option<&McpCommand>,
    skills_on_disk: bool,
) -> Vec<StepReport> {
    harnesses
        .iter()
        .flat_map(|harness| install_for(ctx, *harness, command, skills_on_disk))
        .collect()
}

/// The `env` object of a JSON entry that turns the server skills off.
fn skills_off_env() -> serde_json::Value {
    serde_json::json!({ MCP_SKILLS_ENV: MCP_SKILLS_OFF })
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

fn install_for(
    ctx: &Ctx,
    harness: Harness,
    command: Option<&McpCommand>,
    skills_on_disk: bool,
) -> Vec<StepReport> {
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
                .detail(if ctx.user {
                    NO_USER_LAUNCHER_DETAIL
                } else {
                    NO_LAUNCHER_DETAIL
                }),
        ];
    };
    if harness == Harness::Claude && ctx.user {
        let env = if skills_on_disk {
            format!("-e {MCP_SKILLS_ENV}={MCP_SKILLS_OFF} ")
        } else {
            String::new()
        };
        return vec![manual_claude_user_step(ctx, Mode::Install).detail(format!(
            "fallow does not edit ~/.claude.json; run: claude mcp add --scope user {env}{SERVER_KEY} -- {}",
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
            let mut entry = serde_json::json!({
                "type": "stdio",
                "command": command.command,
                "args": command.args,
            });
            if skills_on_disk {
                entry["env"] = skills_off_env();
            }
            let json = json_step(ctx, harness, &path, Scope::Shared, Some(&entry));
            let approval = approval_after(ctx, &json);
            vec![json, approval]
        }
        Harness::Cursor => {
            let mut entry = serde_json::json!({
                "command": command.command,
                "args": command.args,
            });
            if skills_on_disk {
                entry["env"] = skills_off_env();
            }
            vec![json_step(ctx, harness, &path, ctx.scope(), Some(&entry))]
        }
        Harness::Codex => vec![codex_step(ctx, &path, Some(&command), skills_on_disk)],
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
        Harness::Codex => vec![codex_step(ctx, &path, None, false)],
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
        Ok(FileOutcome::ServersNotATable) => base
            .with_status(StepStatus::Refused)
            .reason(Reason::McpEntryForeign)
            .detail("the MCP server section here is not a table; fix it or pass --force to replace it"),
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
    if let Some(link) = ctx.symlink_on([path, backup_path(path).as_path()]) {
        return base.symlinked(ctx, &link);
    }
    let outcome = merge_json_server(path, "mcpServers", entry, ctx.dry_run, ctx.force);
    outcome_report(base, outcome, ctx, path, Reason::InvalidJson)
}

/// Pre-approve the project server only when the `.mcp.json` step left a
/// fallow-managed entry. An approval names the server key, not the command,
/// so approving after a refused or failed write would approve whatever
/// command the existing `fallow` entry runs.
fn approval_after(ctx: &Ctx, json: &StepReport) -> StepReport {
    let managed = matches!(json.status, StepStatus::Written | StepStatus::Unchanged);
    if managed || !ctx.approve {
        return approval_step(ctx, Mode::Install);
    }
    let path = ctx.root.join(CLAUDE_LOCAL_SETTINGS);
    let mut skipped = StepReport::new(
        Some(Harness::Claude),
        Step::Mcp,
        StepStatus::Skipped,
        Scope::Local,
    )
    .path(ctx, &path)
    .detail(format!(
        "not approved: {CLAUDE_PROJECT_FILE} has no fallow-managed entry; fix that step first"
    ));
    skipped.reason = json.reason;
    skipped
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
    if let Some(link) = ctx.symlink_on([path.as_path(), backup_path(&path).as_path()]) {
        return base.symlinked(ctx, &link);
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

fn codex_step(
    ctx: &Ctx,
    path: &Path,
    command: Option<&McpCommand>,
    skills_on_disk: bool,
) -> StepReport {
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
    if let Some(link) = ctx.symlink_on([path, backup_path(path).as_path()]) {
        return base.symlinked(ctx, &link);
    }
    let outcome = merge_codex(path, command, skills_on_disk, ctx.dry_run, ctx.force);
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
    /// The server section holds a value that is not a table or object, so
    /// fallow cannot add its entry next to it without `--force`.
    ServersNotATable,
    InvalidPreserved,
    NotAnObject,
}

enum ApprovalOutcome {
    File(FileOutcome),
    ClearedRejection,
}

/// The widest indent unit that an edit copies from an existing JSON file.
/// The serializer repeats the unit once per nesting level on each line, so
/// a wider first indent falls back to two spaces.
const MAX_KEPT_INDENT: usize = 8;

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
                let width = line.len() - line.trim_start_matches([' ', '\t']).len();
                (width > 0 && width < line.len()).then_some(&line[..width])
            })
            .filter(|ws| ws.len() <= MAX_KEPT_INDENT)
            .unwrap_or("  ")
            .to_string();
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
    let known = ["command", "args", "type", "env"];
    if object.keys().any(|key| !known.contains(&key.as_str())) {
        return false;
    }
    if object
        .get("env")
        .is_some_and(|env| *env != skills_off_env())
    {
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
                if !force {
                    return Ok(FileOutcome::ServersNotATable);
                }
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
        .any(|(key, _)| key != "command" && key != "args" && key != "env")
    {
        return false;
    }
    if let Some(env) = table.get("env")
        && !codex_env_is_skills_off(env)
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

/// True when a Codex `env` table holds only the switch that turns the server
/// skills off.
fn codex_env_is_skills_off(env: &toml_edit::Item) -> bool {
    env.as_table_like().is_some_and(|table| {
        table.len() == 1
            && table.get(MCP_SKILLS_ENV).and_then(toml_edit::Item::as_str) == Some(MCP_SKILLS_OFF)
    })
}

/// Set or remove `[mcp_servers.fallow]` in a Codex `config.toml`, keeping
/// formatting and comments of everything else.
pub fn merge_codex(
    path: &Path,
    command: Option<&McpCommand>,
    skills_on_disk: bool,
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
            // An inline `mcp_servers = { ... }` is a table too and keeps its
            // siblings. Any other value is the user's, so only `--force`
            // replaces it.
            if servers.as_table_like().is_none() {
                if !force {
                    return Ok(FileOutcome::ServersNotATable);
                }
                *servers = toml_edit::Item::Table(toml_edit::Table::new());
            }
            if let Some(table) = servers.as_table_mut() {
                table.set_implicit(true);
            }
            let Some(servers) = servers.as_table_like_mut() else {
                return Err(format!("{}: mcp_servers is not a table", path.display()));
            };
            let mut entry = toml_edit::Table::new();
            entry.insert("command", toml_edit::value(command.command.as_str()));
            let mut args = toml_edit::Array::new();
            for arg in &command.args {
                args.push(arg.as_str());
            }
            entry.insert("args", toml_edit::value(args));
            if skills_on_disk {
                let mut env = toml_edit::InlineTable::new();
                env.insert(MCP_SKILLS_ENV, MCP_SKILLS_OFF.into());
                entry.insert("env", toml_edit::value(env));
            }
            servers.insert(SERVER_KEY, toml_edit::Item::Table(entry));
        }
        None => {
            let Some(servers) = doc
                .get_mut("mcp_servers")
                .and_then(toml_edit::Item::as_table_like_mut)
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

/// The warning for a Codex project gate in a repository nested in another
/// work tree. The Codex handler runs no gate there.
pub(super) const NESTED_CODEX_GATE_WARNING: &str = "warning: the Codex gate does not run in a repository nested in another work tree; install in the outer repository";

/// True when the git work tree of `root` lies inside the work tree of
/// another repository (for example a submodule or a nested clone), and the
/// outer repository does not list it as a linked worktree. A work tree at
/// `home` does not count as the outer work tree.
pub(super) fn is_nested_repository(root: &Path, home: Option<&Path>) -> bool {
    let Some(top) = crate::base_worktree::git_toplevel(root) else {
        return false;
    };
    let Some(outer) = top.parent().and_then(crate::base_worktree::git_toplevel) else {
        return false;
    };
    let canonical = |path: &Path| dunce::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    if home.is_some_and(|home| canonical(home) == outer) {
        return false;
    }
    let listed = fallow_engine::changed_files::git_command_in(&outer)
        .args(["worktree", "list", "--porcelain"])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .is_some_and(|output| {
            String::from_utf8_lossy(&output.stdout)
                .lines()
                .filter_map(|line| line.strip_prefix("worktree "))
                .any(|path| canonical(Path::new(path)) == top)
        });
    !listed
}

/// Where `root` sits relative to git.
pub(super) enum GitTop {
    /// No directory at or above `root` holds a `.git` entry.
    Outside,
    /// The canonical work tree top level.
    Top(PathBuf),
    /// Git fails there (for example a `safe.directory` refusal) or prints a
    /// path that is not UTF-8.
    Unknown,
}

pub(super) fn git_top(root: &Path) -> GitTop {
    if !root
        .ancestors()
        .any(|dir| dir.join(".git").symlink_metadata().is_ok())
    {
        return GitTop::Outside;
    }
    crate::base_worktree::git_toplevel(root).map_or(GitTop::Unknown, GitTop::Top)
}

/// `Some(true)` when git tracks one of `paths` (relative to `root`) or a file
/// below it, `Some(false)` when it tracks none, `None` when git cannot run or
/// exits with an error. A caller treats `None` as "tracked". Git reads each
/// path as a literal path, without pathspec magic or globs.
pub fn git_tracks_any(root: &Path, paths: &[PathBuf]) -> Option<bool> {
    let output = fallow_engine::changed_files::git_command_in(root)
        .env("GIT_LITERAL_PATHSPECS", "1")
        .args(["ls-files", "-z", "--"])
        .args(paths)
        .output()
        .ok()?;
    output.status.success().then_some(!output.stdout.is_empty())
}

fn is_git_tracked(root: &Path, relative: &str) -> bool {
    fallow_engine::changed_files::git_command_in(root)
        .args(["ls-files", "--error-unmatch", relative])
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

/// Read the registered command for a harness, for `agent status`.
/// `Loaded(None)` is a file that parses and has no fallow entry.
pub fn registered_command(path: &Path, harness: Harness) -> Existing<Option<McpCommand>> {
    load_existing(path, |text| match harness {
        Harness::Claude | Harness::Cursor => {
            let value: serde_json::Value = serde_json::from_str(text).map_err(|e| e.to_string())?;
            Ok(json_entry_command(&value, harness))
        }
        Harness::Codex => {
            let doc = text
                .parse::<toml_edit::DocumentMut>()
                .map_err(|e| toml_error_position(&e, text))?;
            Ok(toml_entry_command(&doc))
        }
    })
}

/// The position of a TOML parse error, without the message of the parser.
/// That message quotes the line of the file.
fn toml_error_position(err: &toml_edit::TomlError, text: &str) -> String {
    let Some(span) = err.span() else {
        return "TOML parse error".to_owned();
    };
    let before = text.get(..span.start).unwrap_or(text);
    let line = before.matches('\n').count() + 1;
    let column = before
        .rsplit('\n')
        .next()
        .unwrap_or_default()
        .chars()
        .count()
        + 1;
    format!("TOML parse error at line {line}, column {column}")
}

fn json_entry_command(value: &serde_json::Value, harness: Harness) -> Option<McpCommand> {
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

fn toml_entry_command(doc: &toml_edit::DocumentMut) -> Option<McpCommand> {
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

#[cfg(all(test, unix))]
mod serves_mcp_tests {
    use std::os::unix::fs::PermissionsExt;
    use std::path::PathBuf;

    use super::serves_mcp;

    fn script(dir: &tempfile::TempDir, body: &str) -> PathBuf {
        let path = dir.path().join("fallow");
        std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).expect("write script");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))
            .expect("make script executable");
        path
    }

    #[test]
    fn the_mcp_server_version_line_proves_the_entry() {
        let dir = tempfile::tempdir().expect("temporary directory");
        let exe = script(&dir, "echo 'fallow-mcp 3.43.0'");
        // On Linux, a fork in a parallel test can hold the new script open for
        // write, and the spawn then fails with "Text file busy".
        assert!((0..5).any(|_| serves_mcp(&exe)));
    }

    #[test]
    fn a_probe_that_does_not_exit_is_not_the_mcp_server() {
        let dir = tempfile::tempdir().expect("temporary directory");
        let started = std::time::Instant::now();
        assert!(!serves_mcp(&script(&dir, "exec sleep 30")));
        assert!(started.elapsed() < std::time::Duration::from_secs(20));
    }

    /// clap of the plain CLI answers `mcp-server --version` with its own
    /// version line and exit code 0.
    #[test]
    fn the_cli_version_line_is_not_the_mcp_server() {
        let dir = tempfile::tempdir().expect("temporary directory");
        assert!(!serves_mcp(&script(&dir, "echo 'fallow 3.43.0'")));
    }

    #[test]
    fn a_failed_probe_is_not_the_mcp_server() {
        let dir = tempfile::tempdir().expect("temporary directory");
        assert!(!serves_mcp(&script(
            &dir,
            "echo 'fallow-mcp 3.43.0'; exit 1"
        )));
    }

    #[test]
    fn a_missing_binary_is_not_the_mcp_server() {
        let dir = tempfile::tempdir().expect("temporary directory");
        assert!(!serves_mcp(&dir.path().join("missing")));
    }
}

#[cfg(test)]
mod shell_words_tests {
    use super::{McpCommand, McpSource};

    fn command(command: &str, args: &[&str]) -> McpCommand {
        McpCommand {
            command: command.to_owned(),
            args: args.iter().map(|arg| (*arg).to_owned()).collect(),
            source: McpSource::Path,
        }
    }

    #[test]
    fn plain_words_stay_unquoted() {
        assert_eq!(
            command("npx", &["--no", "fallow-mcp"]).shell_words(),
            "npx --no fallow-mcp"
        );
    }

    #[cfg(not(windows))]
    #[test]
    fn a_path_with_shell_characters_stays_one_argument() {
        assert_eq!(
            command("/opt/my tools/fallow-mcp;touch x", &["--root", "/srv/a&b"]).shell_words(),
            "'/opt/my tools/fallow-mcp;touch x' --root '/srv/a&b'"
        );
    }

    #[cfg(windows)]
    #[test]
    fn a_path_with_shell_characters_stays_one_argument() {
        assert_eq!(
            command(r"C:\Program Files\nodejs\npx.cmd", &["--root", r"C:\a&b"]).shell_words(),
            r#""C:\Program Files\nodejs\npx.cmd" --root "C:\a&b""#
        );
    }
}
