//! `fallow agent status`: read-only view of every managed surface.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use serde::Serialize;

use super::{
    Harness, NextAction, SCHEMA_VERSION, Step, display_path, hosts, mcp, resolve_root, skill,
};
use crate::setup_hooks::{build_hooks_status, find_managed_block_bounds};
use crate::state_file::{Existing, read_optional_text};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum SurfaceState {
    Installed,
    /// Installed by an older fallow; rerun `agent install` to refresh.
    Stale,
    Absent,
    /// Present but not written by fallow.
    Foreign,
}

/// Why an installed gate surface still audits nothing.
///
/// The gate script resolves its dependencies when Claude Code runs it, so a
/// file that is present and fallow-managed can be a no-op. Carried out of band
/// so the remediation can name the reason instead of parsing `detail`.
#[derive(Clone, Debug, PartialEq, Eq)]
enum GateBlocker {
    /// `jq` is not on PATH. The script exits 0 after one stderr line, which a
    /// PreToolUse hook never shows.
    JqMissing,
    /// A native-capable gate cannot find either supported JSON reader.
    JsonReaderMissing,
    /// A candidate launcher exists, but status cannot execute project code.
    NativeReaderUnverified,
    /// The installed native gate differs from the verified managed template.
    ModifiedNativeGate,
    /// PATH resolves a different fallow than this build, and the gate runs
    /// what PATH resolves because the project has no local launcher.
    PathVersion(String),
}

#[derive(Serialize)]
struct SurfaceStatus {
    #[serde(skip_serializing_if = "Option::is_none")]
    harness: Option<Harness>,
    step: Step,
    state: SurfaceState,
    path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    detail: Option<String>,
    #[serde(skip)]
    blocker: Option<GateBlocker>,
    #[serde(skip)]
    user_scope: bool,
}

/// Evidence available from inventorying a native gate and its launchers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum NativeGateState {
    Available,
    Missing,
    Modified,
    Unverified,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct GateRuntime {
    jq_missing: bool,
    /// Native script identity and the reader evidence available without execution.
    native_gates: Vec<(String, NativeGateState)>,
    /// Version reported by the `fallow` on PATH when it is not this build.
    path_version: Option<String>,
}

impl GateRuntime {
    fn probe(root: &Path, hooks: &crate::setup_hooks::HooksStatusReport) -> Self {
        Self {
            jq_missing: mcp::find_on_path("jq").is_none(),
            native_gates: [&hooks.claude, &hooks.codex_gate]
                .into_iter()
                .filter_map(|status| native_gate_probe(root, status))
                .collect(),
            path_version: path_fallow_drift(root),
        }
    }

    fn blocker(&self, path: &str) -> Option<GateBlocker> {
        let native = self.native_gates.iter().find(|(gate, _)| gate == path);
        if matches!(native, Some((_, NativeGateState::Modified))) {
            return Some(GateBlocker::ModifiedNativeGate);
        }
        if self.jq_missing {
            match native {
                Some((_, NativeGateState::Available)) => {}
                Some((_, NativeGateState::Missing)) => return Some(GateBlocker::JsonReaderMissing),
                Some((_, NativeGateState::Unverified)) => {
                    return Some(GateBlocker::NativeReaderUnverified);
                }
                _ => return Some(GateBlocker::JqMissing),
            }
        }
        self.path_version.clone().map(GateBlocker::PathVersion)
    }
}

fn native_gate_probe(
    root: &Path,
    status: &crate::setup_hooks::HookSurfaceStatus,
) -> Option<(String, NativeGateState)> {
    if !status.installed {
        return None;
    }
    let script = root.join(&status.path);
    let text = read_optional_text(&script).ok().flatten()?;
    if !text.contains("# Native JSON protocol: fallow-gate-json-v1") {
        return None;
    }
    // A user-scope gate records the path of its fallow. The comparison
    // accepts any recorded path in the exact line that the install writes.
    let user = crate::setup_hooks::is_user_scope_gate_script(&text);
    let trusted = if user {
        crate::setup_hooks::render_gate_script(true, None)
    } else {
        crate::setup_hooks::rendered_gate_script()
    };
    if !gate_matches_template(&text, &trusted, user) {
        return Some((status.path.clone(), NativeGateState::Modified));
    }
    Some((status.path.clone(), native_reader_inventory(root)))
}

/// Header comment that records which fallow version wrote the gate.
const INSTALLER_VERSION_LINE: &str = "# Installer version: ";

/// Compares an installed gate with the managed template line by line. The
/// installer version comment changes with each fallow release, so it does not
/// make a gate modified. Every other byte must match. CRLF line endings count
/// as an edit, because bash cannot run a script with them.
/// The start of the line of a user-scope gate that records its fallow.
const GATE_BIN_LINE: &str = "GATE_FALLOW_BIN=";

fn gate_matches_template(installed: &str, trusted: &str, user: bool) -> bool {
    normalized_gate_lines(installed, user).eq(normalized_gate_lines(trusted, user))
}

/// True for the exact line that the install writes for a user-scope gate:
/// `GATE_FALLOW_BIN='<path>'`, with an empty or absolute path (`/...` or
/// `X:/...`) and no quote or line end in it.
fn is_recorded_bin_line(line: &str) -> bool {
    let Some(value) = line
        .strip_prefix("GATE_FALLOW_BIN='")
        .and_then(|rest| rest.strip_suffix('\''))
    else {
        return false;
    };
    let bytes = value.as_bytes();
    let absolute = value.is_empty()
        || value.starts_with('/')
        || (bytes.len() > 2 && bytes[0].is_ascii_alphabetic() && &bytes[1..3] == b":/");
    absolute && !value.contains(['\'', '\n', '\r'])
}

fn normalized_gate_lines(text: &str, user: bool) -> impl Iterator<Item = &str> {
    text.split('\n').map(move |line| {
        if line.starts_with(INSTALLER_VERSION_LINE) {
            INSTALLER_VERSION_LINE
        } else if user && is_recorded_bin_line(line) {
            GATE_BIN_LINE
        } else {
            line
        }
    })
}

fn native_reader_inventory(root: &Path) -> NativeGateState {
    let current = std::env::current_exe()
        .ok()
        .and_then(|path| dunce::canonicalize(path).ok());
    let candidate_state = |candidate: &Path| {
        if current.as_ref().is_some_and(|current| {
            dunce::canonicalize(candidate).is_ok_and(|candidate| &candidate == current)
        }) {
            NativeGateState::Available
        } else {
            NativeGateState::Unverified
        }
    };
    // The gate tries the launcher of its install root, then the fallow on
    // PATH, then the package manager of the install root. It never uses a
    // launcher of a parent directory.
    let local = root.join("node_modules/.bin/fallow");
    if is_executable_file(&local) {
        return candidate_state(&local);
    }
    if let Some(path) = mcp::find_on_path("fallow") {
        return candidate_state(&path);
    }
    let package_runner = mcp::find_on_path("yarn").is_some() || mcp::find_on_path("npx").is_some();
    if package_runner && root.join("package.json").is_file() {
        return NativeGateState::Unverified;
    }
    NativeGateState::Missing
}

/// Version of the `fallow` PATH resolves, when that binary is not this one and
/// reports a version other than this build's.
///
/// The gate runs an executable project-local `node_modules/.bin/fallow` first
/// and the `fallow` on PATH only when no such launcher exists. Returns `None`
/// when that launcher exists, when nothing resolves, when PATH resolves this
/// very executable, or when the probe fails: the gate has further fallbacks
/// and an unreadable binary is not evidence of drift. Status does not run
/// project code, so a local launcher that is executable but cannot start
/// still counts as the runner, although the gate then falls back to PATH.
fn path_fallow_drift(root: &Path) -> Option<String> {
    if is_executable_file(&root.join("node_modules/.bin/fallow")) {
        return None;
    }
    let candidate = std::path::absolute(mcp::find_on_path("fallow")?).ok()?;
    let resolved = dunce::canonicalize(&candidate).unwrap_or_else(|_| candidate.clone());
    // Resolve directory aliases without losing ownership of a final symlink.
    let entry = candidate
        .parent()
        .and_then(|parent| dunce::canonicalize(parent).ok())
        .and_then(|parent| candidate.file_name().map(|name| parent.join(name)));
    let owned_by = |owner: &Path| {
        candidate.starts_with(owner)
            || resolved.starts_with(owner)
            || entry.as_ref().is_some_and(|path| path.starts_with(owner))
    };
    let project = dunce::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
    let project_launcher = project.ancestors().any(|anchor| {
        let owns_project = anchor.parent().is_some()
            && (anchor.join(".git").exists() || anchor.join("package.json").is_file());
        (owns_project && owned_by(anchor)) || owned_by(&anchor.join("node_modules"))
    });
    if owned_by(&project) || project_launcher {
        return None;
    }
    let current = std::env::current_exe()
        .ok()
        .map(|exe| dunce::canonicalize(&exe).unwrap_or(exe));
    if current.as_deref() == Some(resolved.as_path()) {
        return None;
    }
    let mut command = std::process::Command::new(&resolved);
    command
        .arg("--version")
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    let output = fallow_process::output(&mut command).ok()?;
    if !output.status.success() {
        return None;
    }
    let reported = parse_version_output(&String::from_utf8_lossy(&output.stdout))?;
    (reported != env!("CARGO_PKG_VERSION")).then_some(reported)
}

/// Whether `path` is a file that the gate's `[ -x ]` test accepts. Windows
/// has no executable bit, so there a file is enough.
fn is_executable_file(path: &Path) -> bool {
    let Ok(metadata) = std::fs::metadata(path) else {
        return false;
    };
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        metadata.is_file() && metadata.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        metadata.is_file()
    }
}

/// Read the version out of a `fallow --version` line (`fallow 3.23.0`).
fn parse_version_output(stdout: &str) -> Option<String> {
    let line = stdout.lines().next()?.trim();
    let version = line.rsplit(' ').next().unwrap_or(line).trim();
    (!version.is_empty()).then(|| version.to_string())
}

#[derive(Serialize)]
struct StatusReport {
    kind: &'static str,
    schema_version: u32,
    fallow_version: &'static str,
    root: String,
    evidence: Vec<hosts::Detection>,
    surfaces: Vec<SurfaceStatus>,
    next_actions: Vec<NextAction>,
}

/// Entry point for `fallow agent status`.
pub fn run_agent_status(
    root: &Path,
    root_explicit: bool,
    output: fallow_config::OutputFormat,
    json_style: crate::json_style::JsonStyle,
) -> ExitCode {
    let root = resolve_root(root, root_explicit);
    let home = fallow_process::HostPaths::from_env().home;
    let surfaces = surfaces(&root, home.as_deref());
    if root.join(".codex/hooks/fallow-gate.sh").is_file()
        && mcp::is_nested_repository(&root, home.as_deref())
    {
        eprintln!("{}", mcp::NESTED_CODEX_GATE_WARNING);
    }
    let root_arg = root_explicit.then(|| super::root_arg(&root));
    let next_actions = status_next_actions(&surfaces, root_arg.as_deref());
    let report = StatusReport {
        kind: "agent-status",
        schema_version: SCHEMA_VERSION,
        fallow_version: env!("CARGO_PKG_VERSION"),
        root: root.display().to_string().replace('\\', "/"),
        evidence: hosts::detect(&root, home.as_deref()),
        surfaces,
        next_actions,
    };
    match output {
        fallow_config::OutputFormat::Json => match json_style.serialize(&report) {
            Ok(json) => {
                crate::report::sink::outln!("{json}");
                ExitCode::SUCCESS
            }
            Err(error) => crate::error::emit_input_error_with_style(
                &format!("failed to serialize agent status: {error}"),
                output,
                json_style,
            ),
        },
        fallow_config::OutputFormat::Human => {
            print!("{}", render_human(&report));
            ExitCode::SUCCESS
        }
        _ => crate::error::emit_input_error("agent status supports human and json output", output),
    }
}

fn surfaces(root: &Path, home: Option<&Path>) -> Vec<SurfaceStatus> {
    let mut rows: Vec<SurfaceStatus> = Vec::new();

    let agents = root.join("AGENTS.md");
    rows.push(guide_row(root, home, &agents, None));
    let claude_md = root.join("CLAUDE.md");
    rows.push(claude_import_row(root, home, &claude_md));

    for (harness, name, dir) in skill_dirs(root) {
        rows.push(skill_row(root, home, harness, name, &dir));
    }
    if let Some(home) = home {
        let user_scope = home != root;
        for (harness, name, dir) in skill_dirs(home) {
            if skill::inspect(&dir) != skill::SkillState::Absent {
                let mut row = skill_row(root, Some(home), harness, name, &dir);
                row.user_scope = user_scope;
                rows.push(row);
            }
        }
    }

    rows.push(mcp_row(
        root,
        home,
        Harness::Claude,
        &root.join(mcp::claude_project_file()),
    ));
    rows.push(mcp_row(
        root,
        home,
        Harness::Codex,
        &root.join(mcp::codex_file()),
    ));
    if let Some(home) = home {
        rows.extend(user_mcp_rows(root, home));
    }
    rows.push(mcp_row(
        root,
        home,
        Harness::Cursor,
        &root.join(mcp::cursor_file()),
    ));

    if let Some(home) = home.filter(|home| *home != root) {
        rows.extend(user_gate_rows(root, home));
    }

    let hooks = build_hooks_status(root);
    let runtime = GateRuntime::probe(root, &hooks);
    rows.push(hook_row(Some(Harness::Claude), &hooks.claude, &runtime));
    rows.push(hook_row(Some(Harness::Codex), &hooks.codex_gate, &runtime));
    rows.push(hook_row(Some(Harness::Codex), &hooks.codex, &runtime));
    rows
}

/// Rows for the user-level MCP entries in `home`. An entry that starts a
/// package runner runs the fallow-mcp of each project, so it is stale.
fn user_mcp_rows(root: &Path, home: &Path) -> Vec<SurfaceStatus> {
    let mut rows = Vec::new();
    // `~/.claude.json` holds the user-level Claude servers. Status only reads
    // it; fallow never edits it.
    for (harness, file) in [
        (Harness::Claude, ".claude.json"),
        (Harness::Codex, mcp::codex_file()),
        (Harness::Cursor, mcp::cursor_file()),
    ] {
        let path = home.join(file);
        // The project row already covers a home directory that is the root.
        if harness == Harness::Cursor && home == root {
            continue;
        }
        let Existing::Loaded(Some(command)) = mcp::registered_command(&path, harness) else {
            continue;
        };
        let mut row = mcp_row(root, Some(home), harness, &path);
        row.user_scope = home != root;
        if row.user_scope && row.state == SurfaceState::Installed && runs_a_package_runner(&command)
        {
            let fix = if harness == Harness::Claude {
                "run the claude mcp add command that agent install --user prints"
            } else {
                "agent install --user writes an absolute launcher"
            };
            row.state = SurfaceState::Stale;
            row.detail = Some(format!(
                "{}: a user-level entry runs the fallow-mcp of each project it opens; {fix}",
                command.shell_words()
            ));
        }
        rows.push(row);
    }
    rows
}

/// True when an MCP entry starts a package runner that resolves `fallow-mcp`
/// from the current project: `npx`, `bunx` or `pnpm dlx`, also as a `.cmd`
/// or `.exe` file and by path.
fn runs_a_package_runner(command: &mcp::McpCommand) -> bool {
    let name = command
        .command
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();
    let name = name
        .strip_suffix(".cmd")
        .or_else(|| name.strip_suffix(".exe"))
        .unwrap_or(&name);
    match name {
        "npx" | "bunx" => true,
        "pnpm" => command.args.first().is_some_and(|arg| arg == "dlx"),
        _ => false,
    }
}

/// Rows for the user-level gate scripts that fallow wrote in `home`. A script
/// without the user scope comes from an older fallow and searches launchers in
/// the session tree, so it is stale.
fn user_gate_rows(root: &Path, home: &Path) -> Vec<SurfaceStatus> {
    [(Harness::Claude, ".claude"), (Harness::Codex, ".codex")]
        .into_iter()
        .filter_map(|(harness, dir)| {
            let path = home.join(dir).join("hooks").join("fallow-gate.sh");
            let text = read_optional_text(&path).ok().flatten()?;
            if !crate::setup_hooks::is_generated_gate_script(&text) {
                return None;
            }
            let (state, detail) = if crate::setup_hooks::is_user_scope_gate_script(&text) {
                (SurfaceState::Installed, "user-level gate script")
            } else {
                (
                    SurfaceState::Stale,
                    "user-level gate script from an older fallow; it runs a fallow launcher of the session directory",
                )
            };
            Some(SurfaceStatus {
                harness: Some(harness),
                step: Step::Hooks,
                state,
                path: display_path(root, Some(home), &path),
                detail: Some(detail.to_string()),
                blocker: None,
                user_scope: true,
            })
        })
        .collect()
}

/// Every released skill directory under `base`, harness-neutral first.
fn skill_dirs(base: &Path) -> Vec<(Option<Harness>, &'static str, PathBuf)> {
    [(None, ".agents"), (Some(Harness::Claude), ".claude")]
        .into_iter()
        .flat_map(|(harness, host)| {
            skill::RELEASED_SKILLS.iter().map(move |released| {
                (
                    harness,
                    released.name,
                    base.join(host).join("skills").join(released.name),
                )
            })
        })
        .collect()
}

fn guide_row(
    root: &Path,
    home: Option<&Path>,
    path: &Path,
    harness: Option<Harness>,
) -> SurfaceStatus {
    let text = read_optional_text(path).ok().flatten();
    let (state, detail) = match text.as_deref() {
        None => (SurfaceState::Absent, None),
        Some(text) if find_managed_block_bounds(text).is_some() => {
            let detail = if super::guide::is_authored(text) {
                "authored by fallow, task map block present"
            } else {
                "task map block present"
            };
            (SurfaceState::Installed, Some(detail.to_string()))
        }
        Some(_) => (
            SurfaceState::Foreign,
            Some("no fallow task map block".to_string()),
        ),
    };
    SurfaceStatus {
        harness,
        step: Step::Guide,
        state,
        path: display_path(root, home, path),
        detail,
        blocker: None,
        user_scope: false,
    }
}

fn claude_import_row(root: &Path, home: Option<&Path>, path: &Path) -> SurfaceStatus {
    let text = read_optional_text(path).ok().flatten();
    let (state, detail) = match text.as_deref() {
        None => (SurfaceState::Absent, None),
        Some(text) if text.lines().any(|line| line.trim() == "@AGENTS.md") => (
            SurfaceState::Installed,
            Some("imports AGENTS.md".to_string()),
        ),
        Some(_) => (
            SurfaceState::Foreign,
            Some("no @AGENTS.md import".to_string()),
        ),
    };
    SurfaceStatus {
        harness: Some(Harness::Claude),
        step: Step::Guide,
        state,
        path: display_path(root, home, path),
        detail,
        blocker: None,
        user_scope: false,
    }
}

fn skill_row(
    root: &Path,
    home: Option<&Path>,
    harness: Option<Harness>,
    name: &str,
    dir: &Path,
) -> SurfaceStatus {
    let (state, detail) = match skill::inspect(dir) {
        skill::SkillState::Absent => (SurfaceState::Absent, None),
        skill::SkillState::Foreign => (
            SurfaceState::Foreign,
            Some(format!("skill named {name} without a fallow marker")),
        ),
        skill::SkillState::Managed { flavor, version } => {
            let state = if version == env!("CARGO_PKG_VERSION") {
                SurfaceState::Installed
            } else {
                SurfaceState::Stale
            };
            (
                state,
                Some(format!(
                    "{} skill from fallow {version}",
                    match flavor {
                        skill::Flavor::Stub => "pointer",
                        skill::Flavor::Embedded => "embedded",
                    }
                )),
            )
        }
    };
    SurfaceStatus {
        harness,
        step: Step::Skill,
        state,
        path: display_path(root, home, dir),
        detail,
        blocker: None,
        user_scope: false,
    }
}

fn mcp_row(root: &Path, home: Option<&Path>, harness: Harness, path: &Path) -> SurfaceStatus {
    let (state, detail) = match mcp::registered_command(path, harness) {
        Existing::Loaded(Some(command)) if mcp::registered_is_managed(&command) => {
            (SurfaceState::Installed, Some(command.shell_words()))
        }
        Existing::Loaded(Some(command)) => (
            SurfaceState::Foreign,
            Some(format!("{} (not written by fallow)", command.shell_words())),
        ),
        Existing::Loaded(None) => (SurfaceState::Absent, Some("no fallow entry".to_string())),
        Existing::Absent => (SurfaceState::Absent, None),
        Existing::Unreadable(err) => (
            SurfaceState::Absent,
            Some(format!("cannot read the file: {err}")),
        ),
        Existing::Invalid(reason) => (
            SurfaceState::Absent,
            Some(format!(
                "the file does not parse ({reason}); agent install keeps it unchanged"
            )),
        ),
    };
    SurfaceStatus {
        harness: Some(harness),
        step: Step::Mcp,
        state,
        path: display_path(root, home, path),
        detail,
        blocker: None,
        user_scope: false,
    }
}

fn hook_row(
    harness: Option<Harness>,
    status: &crate::setup_hooks::HookSurfaceStatus,
    runtime: &GateRuntime,
) -> SurfaceStatus {
    // Only a script-backed surface executes the gate; the AGENTS.md routing
    // block is managed prose and carries no script version.
    let script_version = status.script_version.as_deref();
    let blocker = match script_version {
        Some(_) if status.installed => runtime.blocker(&status.path),
        _ => None,
    };
    let script_stale = script_version.is_some_and(|version| version != env!("CARGO_PKG_VERSION"))
        || (status.installed && status.handler_stale);
    let state = if status.installed {
        if script_stale || blocker.is_some() {
            SurfaceState::Stale
        } else {
            SurfaceState::Installed
        }
    } else if status.user_edited {
        SurfaceState::Foreign
    } else {
        SurfaceState::Absent
    };
    let mut parts: Vec<String> = Vec::new();
    if status.installed && status.handler_stale {
        parts.push(
            "the hook handler is not the one that this fallow writes for this root, so it may not run this gate"
                .to_string(),
        );
    }
    if let Some(version) = script_version {
        parts.push(if script_stale {
            format!(
                "gate script from fallow {version}, this build is {}",
                env!("CARGO_PKG_VERSION")
            )
        } else {
            format!("gate script from fallow {version}")
        });
    }
    match &blocker {
        Some(GateBlocker::JqMissing) => {
            parts.push("jq is not on PATH, so the gate exits without auditing".to_string());
        }
        Some(GateBlocker::JsonReaderMissing) => {
            parts.push("the gate cannot find a native Fallow JSON reader or jq, so it exits without auditing".to_string());
        }
        Some(GateBlocker::ModifiedNativeGate) => {
            parts.push("the installed native gate differs from this build's managed script; its runtime is unverified".to_string());
        }
        Some(GateBlocker::NativeReaderUnverified) => {
            parts.push("a native reader launcher is present, but its runtime is unverified; status does not execute project launchers or package managers".to_string());
        }
        Some(GateBlocker::PathVersion(version)) => {
            parts.push(format!(
                "PATH resolves fallow {version}, and the gate runs that, not this build"
            ));
        }
        None => {}
    }
    let detail = (!parts.is_empty()).then(|| parts.join("; "));
    SurfaceStatus {
        harness,
        step: Step::Hooks,
        state,
        path: status.path.clone(),
        detail,
        blocker,
        user_scope: false,
    }
}

fn status_next_actions(surfaces: &[SurfaceStatus], root_arg: Option<&str>) -> Vec<NextAction> {
    let scope = root_arg.unwrap_or_default();
    let mut next: Vec<NextAction> = Vec::new();
    for (id, user_flag, scope_name, matches_scope) in [
        ("agent-install", "", "project", false),
        ("agent-install-user", " --user", "user-scope", true),
    ] {
        if surfaces.iter().any(|row| {
            row.user_scope == matches_scope
                && matches!(row.state, SurfaceState::Absent | SurfaceState::Stale)
        }) {
            next.push(NextAction {
                id,
                command: format!("fallow agent install{user_flag} --dry-run{scope}"),
                reason: format!(
                    "Shows what agent install{user_flag} would write for the absent or stale {scope_name} surfaces above; drop --dry-run to apply."
                ),
                mutating: false,
            });
        }
    }
    push_gate_next_actions(surfaces, scope, &mut next);
    for (id, user_flag, scope_name, matches_scope) in [
        ("agent-install-force", "", "project", false),
        ("agent-install-user-force", " --user", "user-scope", true),
    ] {
        if surfaces
            .iter()
            .any(|row| row.user_scope == matches_scope && row.state == SurfaceState::Foreign)
        {
            next.push(NextAction {
                id,
                command: format!("fallow agent install{user_flag} --force{scope}"),
                reason: format!(
                    "Foreign {scope_name} surfaces were not written by fallow; --force replaces them, otherwise they are left alone."
                ),
                mutating: true,
            });
        }
    }
    next
}

fn push_gate_next_actions(surfaces: &[SurfaceStatus], scope: &str, next: &mut Vec<NextAction>) {
    if surfaces
        .iter()
        .any(|row| row.blocker == Some(GateBlocker::JqMissing))
    {
        next.push(NextAction {
            id: "gate-requires-jq",
            command: "jq --version".to_string(),
            reason:
                "The agent gate script needs jq to read the tool input. Without it the script exits 0 after a single stderr line, which a PreToolUse hook never shows, so every commit and push passes ungated. Install jq."
                    .to_string(),
            mutating: false,
        });
    }
    if surfaces
        .iter()
        .any(|row| row.blocker == Some(GateBlocker::JsonReaderMissing))
    {
        next.push(NextAction {
            id: "gate-json-reader",
            command: "fallow hooks gate-runtime probe".to_string(),
            reason: "The gate needs a current Fallow binary on PATH or in the project to read JSON. Upgrade that installation, or install jq for compatibility with an older binary. Project trust still needs approval in the harness.".to_string(),
            mutating: false,
        });
    }
    if surfaces
        .iter()
        .any(|row| row.blocker == Some(GateBlocker::NativeReaderUnverified))
    {
        next.push(NextAction {
            id: "gate-reader-unverified",
            command: "fallow hooks gate-runtime probe".to_string(),
            reason: "After approving this project's code, run this probe with the gate's Fallow installation. Status inventories local launchers and package managers without executing them; native support is not yet verified.".to_string(),
            mutating: false,
        });
    }
    if surfaces
        .iter()
        .any(|row| row.blocker == Some(GateBlocker::ModifiedNativeGate))
    {
        next.push(NextAction {
            id: "gate-script-refresh",
            command: format!("fallow hooks install --target agent --force --dry-run{scope}"),
            reason: "Review the installed gate's edits and this preview before replacing it. Status does not execute modified hook scripts or verify their runtime.".to_string(),
            mutating: false,
        });
    }
    if let Some(version) = surfaces.iter().find_map(|row| match &row.blocker {
        Some(GateBlocker::PathVersion(version)) => Some(version.clone()),
        _ => None,
    }) {
        next.push(NextAction {
            id: "gate-path-version",
            command: "fallow --version".to_string(),
            reason: format!(
                "The gate runs the fallow PATH resolves, which reports {version}, not this build ({}). Upgrade the fallow on PATH so the gate audits with the version you are testing.",
                env!("CARGO_PKG_VERSION")
            ),
            mutating: false,
        });
    }
}

fn render_human(report: &StatusReport) -> String {
    use std::fmt::Write as _;
    let mut out = String::new();
    let _ = writeln!(out, "fallow agent status");
    let _ = writeln!(out, "  root: {}", report.root);
    if report.evidence.is_empty() {
        let _ = writeln!(out, "  detected: none");
    } else {
        for detection in &report.evidence {
            let _ = writeln!(
                out,
                "  detected: {} ({})",
                detection.harness.as_str(),
                detection.evidence.join(", ")
            );
        }
    }
    out.push('\n');
    for row in &report.surfaces {
        let label = match row.harness {
            Some(harness) => format!("{} ({})", row.step.as_str(), harness.as_str()),
            None => row.step.as_str().to_string(),
        };
        let state = match row.state {
            SurfaceState::Installed => "installed",
            SurfaceState::Stale => "stale",
            SurfaceState::Absent => "absent",
            SurfaceState::Foreign => "foreign",
        };
        match &row.detail {
            Some(detail) => {
                let _ = writeln!(
                    out,
                    "  {:<40}  {state:<10}  {label:<16}  {detail}",
                    row.path
                );
            }
            None => {
                let _ = writeln!(out, "  {:<40}  {state:<10}  {label}", row.path);
            }
        }
    }
    if !report.next_actions.is_empty() {
        out.push('\n');
        let _ = writeln!(out, "Next:");
        for next in &report.next_actions {
            let _ = writeln!(out, "  {}", next.command);
            let _ = writeln!(out, "    {}", next.reason);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_install::MARKER_PREFIX;
    use crate::setup_hooks::HookSurfaceStatus;

    fn codex_gate_row(rows: &[SurfaceStatus]) -> &SurfaceStatus {
        rows.iter()
            .find(|row| {
                row.harness == Some(Harness::Codex)
                    && row.step == Step::Hooks
                    && row.path == ".codex/hooks/fallow-gate.sh"
            })
            .expect("codex gate row")
    }

    #[test]
    fn status_reports_a_row_for_the_codex_gate() {
        let dir = tempfile::tempdir().unwrap();
        let rows = surfaces(dir.path(), None);
        assert_eq!(codex_gate_row(&rows).state, SurfaceState::Absent);

        let opts = crate::setup_hooks::SetupHooksOptions {
            root: dir.path(),
            agent: Some(crate::setup_hooks::HookAgentArg::Codex),
            dry_run: false,
            force: false,
            user: false,
            home: None,
            gitignore_claude: false,
            uninstall: false,
        };
        crate::setup_hooks::execute_agent_hooks(&opts, crate::setup_hooks::Mode::Install)
            .unwrap()
            .unwrap();

        let rows = surfaces(dir.path(), None);
        let row = codex_gate_row(&rows);
        // A host without jq or with another fallow on PATH reports `stale`
        // with a runtime blocker. A fresh script must never be stale itself.
        match row.state {
            SurfaceState::Installed => {}
            SurfaceState::Stale => assert!(row.blocker.is_some(), "{:?}", row.detail),
            other => panic!("unexpected state {other:?}"),
        }
        assert!(
            rows.iter()
                .any(|row| row.path == "AGENTS.md" && row.step == Step::Hooks),
            "the AGENTS.md routing block keeps its own row"
        );
    }

    fn claude_gate(script_version: Option<&str>) -> HookSurfaceStatus {
        HookSurfaceStatus {
            installed: true,
            managed_block_present: true,
            user_edited: false,
            path: ".claude/hooks/fallow-gate.sh".to_string(),
            script_version: script_version.map(str::to_string),
            min_version_floor: Some("2.85.0".to_string()),
            handler_stale: false,
        }
    }

    #[test]
    fn gate_script_from_an_older_fallow_is_stale() {
        let row = hook_row(
            Some(Harness::Claude),
            &claude_gate(Some("1.0.0")),
            &GateRuntime::default(),
        );
        assert_eq!(row.state, SurfaceState::Stale);
        assert_eq!(
            row.detail.as_deref(),
            Some(
                format!(
                    "gate script from fallow 1.0.0, this build is {}",
                    env!("CARGO_PKG_VERSION")
                )
                .as_str()
            )
        );
    }

    #[test]
    fn gate_script_from_this_fallow_is_installed() {
        let row = hook_row(
            Some(Harness::Claude),
            &claude_gate(Some(env!("CARGO_PKG_VERSION"))),
            &GateRuntime::default(),
        );
        assert_eq!(row.state, SurfaceState::Installed);
        assert_eq!(row.blocker, None);
    }

    #[test]
    fn missing_jq_makes_the_gate_stale_and_names_the_reason() {
        let runtime = GateRuntime {
            jq_missing: true,
            native_gates: Vec::new(),
            path_version: None,
        };
        let row = hook_row(
            Some(Harness::Claude),
            &claude_gate(Some(env!("CARGO_PKG_VERSION"))),
            &runtime,
        );
        assert_eq!(row.state, SurfaceState::Stale);
        assert_eq!(row.blocker, Some(GateBlocker::JqMissing));
        assert!(
            row.detail
                .as_deref()
                .is_some_and(|detail| detail.contains("jq is not on PATH")),
            "detail was {:?}",
            row.detail
        );

        let actions = status_next_actions(&[row], None);
        let jq = actions
            .iter()
            .find(|action| action.id == "gate-requires-jq")
            .expect("missing jq remediation");
        assert!(jq.reason.contains("exits 0"));
        assert!(!jq.mutating);
    }

    #[test]
    fn native_gate_readiness_tracks_its_reader_instead_of_jq() {
        for ready in [true, false] {
            let runtime = GateRuntime {
                jq_missing: true,
                native_gates: vec![(
                    ".claude/hooks/fallow-gate.sh".to_string(),
                    if ready {
                        NativeGateState::Available
                    } else {
                        NativeGateState::Missing
                    },
                )],
                path_version: None,
            };
            let row = hook_row(
                Some(Harness::Claude),
                &claude_gate(Some(env!("CARGO_PKG_VERSION"))),
                &runtime,
            );
            if ready {
                assert_eq!(row.state, SurfaceState::Installed);
                assert_eq!(row.blocker, None);
            } else {
                assert_eq!(row.state, SurfaceState::Stale);
                assert_eq!(row.blocker, Some(GateBlocker::JsonReaderMissing));
                assert!(
                    status_next_actions(&[row], None)
                        .iter()
                        .any(|action| action.id == "gate-json-reader")
                );
            }
        }
    }

    #[test]
    fn a_different_fallow_on_path_makes_the_gate_stale() {
        let runtime = GateRuntime {
            jq_missing: false,
            native_gates: Vec::new(),
            path_version: Some("3.17.0".to_string()),
        };
        let row = hook_row(
            Some(Harness::Claude),
            &claude_gate(Some(env!("CARGO_PKG_VERSION"))),
            &runtime,
        );
        assert_eq!(row.state, SurfaceState::Stale);
        assert_eq!(
            row.blocker,
            Some(GateBlocker::PathVersion("3.17.0".to_string()))
        );

        let actions = status_next_actions(&[row], None);
        let drift = actions
            .iter()
            .find(|action| action.id == "gate-path-version")
            .expect("missing PATH drift remediation");
        assert!(drift.reason.contains("3.17.0"));
        assert!(drift.reason.contains(env!("CARGO_PKG_VERSION")));
    }

    #[test]
    fn an_absent_gate_does_not_report_a_runtime_blocker() {
        let mut status = claude_gate(Some("1.0.0"));
        status.installed = false;
        let runtime = GateRuntime {
            jq_missing: true,
            native_gates: Vec::new(),
            path_version: Some("3.17.0".to_string()),
        };
        let row = hook_row(Some(Harness::Claude), &status, &runtime);
        assert_eq!(row.state, SurfaceState::Absent);
        assert_eq!(row.blocker, None);
        assert!(
            status_next_actions(&[row], None).iter().all(|action| {
                action.id != "gate-requires-jq" && action.id != "gate-path-version"
            })
        );
    }

    #[test]
    fn the_codex_prose_block_is_not_probed_for_a_gate_runtime() {
        let status = HookSurfaceStatus {
            installed: true,
            managed_block_present: true,
            user_edited: false,
            path: "AGENTS.md".to_string(),
            script_version: None,
            min_version_floor: None,
            handler_stale: false,
        };
        let runtime = GateRuntime {
            jq_missing: true,
            native_gates: Vec::new(),
            path_version: Some("3.17.0".to_string()),
        };
        let row = hook_row(Some(Harness::Codex), &status, &runtime);
        assert_eq!(row.state, SurfaceState::Installed);
        assert_eq!(row.blocker, None);
        assert_eq!(row.detail, None);
    }

    #[test]
    fn version_output_parses_the_prefixed_and_bare_forms() {
        assert_eq!(
            parse_version_output("fallow 3.17.0\n").as_deref(),
            Some("3.17.0")
        );
        assert_eq!(parse_version_output("3.17.0").as_deref(), Some("3.17.0"));
        assert_eq!(parse_version_output(""), None);
    }

    fn probe_installed_gate(text: &str) -> NativeGateState {
        let dir = tempfile::tempdir().unwrap();
        let status = claude_gate(Some(env!("CARGO_PKG_VERSION")));
        let script = dir.path().join(&status.path);
        std::fs::create_dir_all(script.parent().unwrap()).unwrap();
        std::fs::write(&script, text).unwrap();
        native_gate_probe(dir.path(), &status).unwrap().1
    }

    #[test]
    fn native_gate_from_an_older_installer_version_is_not_modified() {
        let older = crate::setup_hooks::rendered_gate_script().replacen(
            &format!("# Installer version: {}", env!("CARGO_PKG_VERSION")),
            "# Installer version: 0.0.1-older",
            1,
        );
        assert_ne!(older, crate::setup_hooks::rendered_gate_script());
        assert_ne!(probe_installed_gate(&older), NativeGateState::Modified);
    }

    #[test]
    fn a_package_runner_entry_counts_for_each_runner_form() {
        let entry = |command: &str, args: &[&str]| mcp::McpCommand {
            command: command.to_string(),
            args: args.iter().map(|arg| (*arg).to_string()).collect(),
            source: mcp::McpSource::Registered,
        };
        for (command, args) in [
            ("npx", &["--no", "fallow-mcp"][..]),
            ("npx.cmd", &["fallow-mcp"][..]),
            ("C:/nodejs/NPX.CMD", &["fallow-mcp"][..]),
            ("bunx", &["fallow-mcp"][..]),
            ("pnpm", &["dlx", "fallow-mcp"][..]),
            ("pnpm.cmd", &["dlx", "fallow-mcp"][..]),
        ] {
            assert!(runs_a_package_runner(&entry(command, args)), "{command}");
        }
        assert!(!runs_a_package_runner(&entry(
            "/usr/local/bin/fallow-mcp",
            &[]
        )));
        assert!(!runs_a_package_runner(&entry(
            "pnpm",
            &["exec", "fallow-mcp"]
        )));
    }

    #[test]
    fn user_scope_native_gate_matches_the_user_scope_template() {
        let user = crate::setup_hooks::render_gate_script(true, Some("/opt/tools/bin/fallow"));
        assert_ne!(probe_installed_gate(&user), NativeGateState::Modified);
        let edited = user.replacen("exit 2", "exit 0", 1);
        assert_eq!(probe_installed_gate(&edited), NativeGateState::Modified);
        for line in [
            "GATE_FALLOW_BIN='/opt/tools/bin/fallow'; touch /tmp/x",
            "GATE_FALLOW_BIN='relative/fallow'",
            "GATE_FALLOW_BIN=\"/opt/fallow\"",
        ] {
            let extra = user.replacen("GATE_FALLOW_BIN='/opt/tools/bin/fallow'", line, 1);
            assert_eq!(
                probe_installed_gate(&extra),
                NativeGateState::Modified,
                "{line}"
            );
        }
        let windows = user.replacen("/opt/tools/bin/fallow", "C:/tools/fallow.exe", 1);
        assert_ne!(probe_installed_gate(&windows), NativeGateState::Modified);
        let project = crate::setup_hooks::rendered_gate_script().replacen(
            "GATE_FALLOW_BIN=''",
            "GATE_FALLOW_BIN='/opt/fallow'",
            1,
        );
        assert_eq!(probe_installed_gate(&project), NativeGateState::Modified);
    }

    #[test]
    fn native_gate_with_crlf_line_endings_is_modified() {
        // Bash cannot run a script with CRLF line endings, so it is not healthy.
        let crlf = crate::setup_hooks::rendered_gate_script().replace('\n', "\r\n");
        assert_eq!(probe_installed_gate(&crlf), NativeGateState::Modified);
    }

    #[test]
    fn native_gate_with_an_edit_is_modified() {
        let edited = crate::setup_hooks::rendered_gate_script().replacen(
            "set -euo pipefail",
            "set -euo pipefail\ntouch /tmp/x",
            1,
        );
        assert_eq!(probe_installed_gate(&edited), NativeGateState::Modified);
        let edited_version_line = crate::setup_hooks::rendered_gate_script().replacen(
            "# Installer version: ",
            "touch /tmp/x # Installer version: ",
            1,
        );
        assert_eq!(
            probe_installed_gate(&edited_version_line),
            NativeGateState::Modified
        );
    }

    fn mcp_row_for(dir: &Path, harness: Harness, file: &str, text: Option<&str>) -> SurfaceStatus {
        let path = dir.join(file);
        if let Some(text) = text {
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, text).unwrap();
        }
        mcp_row(dir, None, harness, &path)
    }

    #[test]
    fn an_mcp_file_with_a_trailing_comma_is_not_reported_as_no_entry() {
        let dir = tempfile::tempdir().unwrap();
        let row = mcp_row_for(
            dir.path(),
            Harness::Claude,
            mcp::claude_project_file(),
            Some(r#"{"mcpServers": {},}"#),
        );
        assert_eq!(row.state, SurfaceState::Absent);
        let detail = row.detail.unwrap_or_default();
        assert!(detail.contains("does not parse"), "{detail}");
    }

    #[test]
    fn an_mcp_file_without_a_fallow_entry_reports_no_entry() {
        let dir = tempfile::tempdir().unwrap();
        let row = mcp_row_for(
            dir.path(),
            Harness::Claude,
            mcp::claude_project_file(),
            Some(r#"{"mcpServers": {}}"#),
        );
        assert_eq!(row.detail.as_deref(), Some("no fallow entry"));
    }

    #[test]
    fn a_missing_mcp_file_has_no_detail() {
        let dir = tempfile::tempdir().unwrap();
        let row = mcp_row_for(
            dir.path(),
            Harness::Claude,
            mcp::claude_project_file(),
            None,
        );
        assert_eq!(row.state, SurfaceState::Absent);
        assert_eq!(row.detail, None);
    }

    #[test]
    fn a_codex_file_with_malformed_toml_is_named() {
        let dir = tempfile::tempdir().unwrap();
        let row = mcp_row_for(
            dir.path(),
            Harness::Codex,
            mcp::codex_file(),
            Some("[mcp_servers"),
        );
        assert_eq!(row.state, SurfaceState::Absent);
        let detail = row.detail.unwrap_or_default();
        assert!(detail.contains("does not parse"), "{detail}");
    }

    #[test]
    fn stale_and_foreign_home_skills_emit_user_scoped_actions() {
        let project = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();

        let claude_skill = home.path().join(".claude/skills/fallow");
        std::fs::create_dir_all(&claude_skill).unwrap();
        std::fs::write(
            claude_skill.join("SKILL.md"),
            format!("<!-- {MARKER_PREFIX} v1 skill=embedded version=0.0.1-old -->\n# Stale\n"),
        )
        .unwrap();

        let agents_skill = home.path().join(".agents/skills/fallow");
        std::fs::create_dir_all(&agents_skill).unwrap();
        std::fs::write(agents_skill.join("SKILL.md"), "# Foreign skill\n").unwrap();

        let rows = surfaces(project.path(), Some(home.path()));
        let actions = status_next_actions(&rows, None);

        assert_eq!(
            actions
                .iter()
                .find(|a| a.id == "agent-install-user")
                .map(|a| a.command.as_str()),
            Some("fallow agent install --user --dry-run")
        );
        assert_eq!(
            actions
                .iter()
                .find(|a| a.id == "agent-install-user-force")
                .map(|a| a.command.as_str()),
            Some("fallow agent install --user --force")
        );
        assert!(
            !actions.iter().any(|a| a.id == "agent-install-force"),
            "project has no foreign surfaces: {actions:?}"
        );

        let user_only_stale = vec![
            SurfaceStatus {
                harness: Some(Harness::Claude),
                step: Step::Skill,
                state: SurfaceState::Installed,
                path: ".claude/skills/fallow".to_string(),
                detail: None,
                blocker: None,
                user_scope: false,
            },
            SurfaceStatus {
                harness: Some(Harness::Claude),
                step: Step::Skill,
                state: SurfaceState::Stale,
                path: "~/.claude/skills/fallow".to_string(),
                detail: None,
                blocker: None,
                user_scope: true,
            },
        ];
        let user_only_actions = status_next_actions(&user_only_stale, Some(" --root /repo"));
        assert!(!user_only_actions.iter().any(|a| a.id == "agent-install"));
        assert_eq!(
            user_only_actions
                .iter()
                .find(|a| a.id == "agent-install-user")
                .map(|a| a.command.as_str()),
            Some("fallow agent install --user --dry-run --root /repo")
        );
    }
}
