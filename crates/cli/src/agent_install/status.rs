//! `fallow agent status`: read-only view of every managed surface.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use serde::Serialize;

use super::{
    Harness, NextAction, SCHEMA_VERSION, Step, display_path, hosts, mcp, resolve_root, skill,
};
use crate::setup_hooks::{
    build_hooks_status, find_managed_block_bounds, home_dir, read_optional_text,
};

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
    /// PATH resolves a different fallow than this build, and the gate runs
    /// what PATH resolves.
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
}

/// What the installed gate script finds at run time, probed once per report.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct GateRuntime {
    jq_missing: bool,
    /// Version reported by the `fallow` on PATH when it is not this build.
    path_version: Option<String>,
}

impl GateRuntime {
    fn probe() -> Self {
        Self {
            jq_missing: mcp::find_on_path("jq").is_none(),
            path_version: path_fallow_drift(),
        }
    }

    fn blocker(&self) -> Option<GateBlocker> {
        if self.jq_missing {
            return Some(GateBlocker::JqMissing);
        }
        self.path_version.clone().map(GateBlocker::PathVersion)
    }
}

/// Version of the `fallow` PATH resolves, when that binary is not this one and
/// reports a version other than this build's.
///
/// The gate prefers PATH over every project-local fallback, so the version it
/// runs is independent of the checkout that installed it. Returns `None` when
/// nothing resolves, when PATH resolves this very executable, or when the
/// probe fails: the gate has further fallbacks and an unreadable binary is not
/// evidence of drift.
fn path_fallow_drift() -> Option<String> {
    let candidate = mcp::find_on_path("fallow")?;
    let resolved = dunce::canonicalize(&candidate).unwrap_or(candidate);
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
    let home = home_dir();
    let surfaces = surfaces(&root, home.as_deref());
    let next_actions = status_next_actions(&surfaces);
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
            Err(error) => crate::error::emit_error_with_style(
                &format!("failed to serialize agent status: {error}"),
                2,
                output,
                json_style,
            ),
        },
        fallow_config::OutputFormat::Human => {
            print!("{}", render_human(&report));
            ExitCode::SUCCESS
        }
        _ => crate::error::emit_error("agent status supports human and json output", 2, output),
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
        for (harness, name, dir) in skill_dirs(home) {
            if skill::inspect(&dir) != skill::SkillState::Absent {
                rows.push(skill_row(root, Some(home), harness, name, &dir));
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
        let user_codex = home.join(mcp::codex_file());
        if mcp::registered_command(&user_codex, Harness::Codex).is_some() {
            rows.push(mcp_row(root, Some(home), Harness::Codex, &user_codex));
        }
    }
    rows.push(mcp_row(
        root,
        home,
        Harness::Cursor,
        &root.join(mcp::cursor_file()),
    ));

    let hooks = build_hooks_status(root);
    let runtime = GateRuntime::probe();
    rows.push(hook_row(Some(Harness::Claude), &hooks.claude, &runtime));
    rows.push(hook_row(Some(Harness::Codex), &hooks.codex_gate, &runtime));
    rows.push(hook_row(Some(Harness::Codex), &hooks.codex, &runtime));
    rows
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
    }
}

fn mcp_row(root: &Path, home: Option<&Path>, harness: Harness, path: &Path) -> SurfaceStatus {
    let (state, detail) = match mcp::registered_command(path, harness) {
        Some(command) if mcp::registered_is_managed(&command) => {
            (SurfaceState::Installed, Some(command.shell_words()))
        }
        Some(command) => (
            SurfaceState::Foreign,
            Some(format!("{} (not written by fallow)", command.shell_words())),
        ),
        None if path.is_file() => (SurfaceState::Absent, Some("no fallow entry".to_string())),
        None => (SurfaceState::Absent, None),
    };
    SurfaceStatus {
        harness: Some(harness),
        step: Step::Mcp,
        state,
        path: display_path(root, home, path),
        detail,
        blocker: None,
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
        Some(_) if status.installed => runtime.blocker(),
        _ => None,
    };
    let script_stale = script_version.is_some_and(|version| version != env!("CARGO_PKG_VERSION"));
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
    }
}

fn status_next_actions(surfaces: &[SurfaceStatus]) -> Vec<NextAction> {
    let mut next: Vec<NextAction> = Vec::new();
    if surfaces
        .iter()
        .any(|row| matches!(row.state, SurfaceState::Absent | SurfaceState::Stale))
    {
        next.push(NextAction {
            id: "agent-install",
            command: "fallow agent install --dry-run".to_string(),
            reason: "Shows what agent install would write for the absent or stale surfaces above; drop --dry-run to apply."
                .to_string(),
            mutating: false,
        });
    }
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
    if surfaces
        .iter()
        .any(|row| row.state == SurfaceState::Foreign)
    {
        next.push(NextAction {
            id: "agent-install-force",
            command: "fallow agent install --force".to_string(),
            reason: "Foreign surfaces were not written by fallow; --force replaces them, otherwise they are left alone."
                .to_string(),
            mutating: true,
        });
    }
    next
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

        let actions = status_next_actions(&[row]);
        let jq = actions
            .iter()
            .find(|action| action.id == "gate-requires-jq")
            .expect("missing jq remediation");
        assert!(jq.reason.contains("exits 0"));
        assert!(!jq.mutating);
    }

    #[test]
    fn a_different_fallow_on_path_makes_the_gate_stale() {
        let runtime = GateRuntime {
            jq_missing: false,
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

        let actions = status_next_actions(&[row]);
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
            path_version: Some("3.17.0".to_string()),
        };
        let row = hook_row(Some(Harness::Claude), &status, &runtime);
        assert_eq!(row.state, SurfaceState::Absent);
        assert_eq!(row.blocker, None);
        assert!(
            status_next_actions(&[row]).iter().all(|action| {
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
        };
        let runtime = GateRuntime {
            jq_missing: true,
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
}
