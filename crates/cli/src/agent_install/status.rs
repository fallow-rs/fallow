//! `fallow agent status`: read-only view of every managed surface.

use std::path::Path;
use std::process::ExitCode;

use serde::Serialize;

use super::{Harness, NextAction, Step, display_path, hosts, mcp, resolve_root, skill};
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

#[derive(Serialize)]
struct SurfaceStatus {
    #[serde(skip_serializing_if = "Option::is_none")]
    harness: Option<Harness>,
    step: Step,
    state: SurfaceState,
    path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    detail: Option<String>,
}

#[derive(Serialize)]
struct StatusReport {
    root: String,
    fallow_version: &'static str,
    detected: Vec<hosts::Detection>,
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
        root: root.display().to_string(),
        fallow_version: env!("CARGO_PKG_VERSION"),
        detected: hosts::detect(&root, home.as_deref()),
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

    for (harness, dir) in [
        (None, root.join(".agents").join("skills").join("fallow")),
        (
            Some(Harness::Claude),
            root.join(".claude").join("skills").join("fallow"),
        ),
    ] {
        rows.push(skill_row(root, home, harness, &dir));
    }
    if let Some(home) = home {
        for (harness, dir) in [
            (None, home.join(".agents").join("skills").join("fallow")),
            (
                Some(Harness::Claude),
                home.join(".claude").join("skills").join("fallow"),
            ),
        ] {
            if skill::inspect(&dir) != skill::SkillState::Absent {
                rows.push(skill_row(root, Some(home), harness, &dir));
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
    rows.push(hook_row(Some(Harness::Claude), &hooks.claude));
    rows.push(hook_row(Some(Harness::Codex), &hooks.codex));
    rows
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
    }
}

fn skill_row(
    root: &Path,
    home: Option<&Path>,
    harness: Option<Harness>,
    dir: &Path,
) -> SurfaceStatus {
    let (state, detail) = match skill::inspect(dir) {
        skill::SkillState::Absent => (SurfaceState::Absent, None),
        skill::SkillState::Foreign => (
            SurfaceState::Foreign,
            Some("skill named fallow without a fallow marker".to_string()),
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
    }
}

fn mcp_row(root: &Path, home: Option<&Path>, harness: Harness, path: &Path) -> SurfaceStatus {
    let (state, detail) = match mcp::registered_command(path, harness) {
        Some(command) => (SurfaceState::Installed, Some(command.shell_words())),
        None if path.is_file() => (SurfaceState::Absent, Some("no fallow entry".to_string())),
        None => (SurfaceState::Absent, None),
    };
    SurfaceStatus {
        harness: Some(harness),
        step: Step::Mcp,
        state,
        path: display_path(root, home, path),
        detail,
    }
}

fn hook_row(
    harness: Option<Harness>,
    status: &crate::setup_hooks::HookSurfaceStatus,
) -> SurfaceStatus {
    let state = if status.installed {
        SurfaceState::Installed
    } else if status.user_edited {
        SurfaceState::Foreign
    } else {
        SurfaceState::Absent
    };
    let detail = status
        .script_version
        .as_ref()
        .map(|version| format!("gate script from fallow {version}"));
    SurfaceStatus {
        harness,
        step: Step::Hooks,
        state,
        path: status.path.clone(),
        detail,
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
    if report.detected.is_empty() {
        let _ = writeln!(out, "  detected: none");
    } else {
        for detection in &report.detected {
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
