//! `fallow agent status`: read-only view of every managed surface.

use std::path::Path;
use std::process::ExitCode;

use serde::Serialize;

use super::{Harness, Step, hosts, mcp, resolve_root, skill};
use crate::setup_hooks::{
    build_hooks_status, display_rel, find_managed_block_bounds, home_dir, read_optional_text,
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
    let report = StatusReport {
        root: root.display().to_string(),
        fallow_version: env!("CARGO_PKG_VERSION"),
        detected: hosts::detect(&root, home.as_deref()),
        surfaces: surfaces(&root, home.as_deref()),
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
            print_human(&report);
            ExitCode::SUCCESS
        }
        _ => crate::error::emit_error("agent status supports human and json output", 2, output),
    }
}

fn surfaces(root: &Path, home: Option<&Path>) -> Vec<SurfaceStatus> {
    let mut rows: Vec<SurfaceStatus> = Vec::new();

    let agents = root.join("AGENTS.md");
    rows.push(guide_row(root, &agents, None));
    let claude_md = root.join("CLAUDE.md");
    rows.push(claude_import_row(root, &claude_md));

    for (harness, dir) in [
        (None, root.join(".agents").join("skills").join("fallow")),
        (
            Some(Harness::Claude),
            root.join(".claude").join("skills").join("fallow"),
        ),
    ] {
        rows.push(skill_row(root, harness, &dir));
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
                rows.push(skill_row(root, harness, &dir));
            }
        }
    }

    rows.push(mcp_row(
        root,
        Harness::Claude,
        &root.join(mcp::claude_project_file()),
    ));
    rows.push(mcp_row(root, Harness::Codex, &root.join(mcp::codex_file())));
    if let Some(home) = home {
        let user_codex = home.join(mcp::codex_file());
        if mcp::registered_command(&user_codex, Harness::Codex).is_some() {
            rows.push(mcp_row(root, Harness::Codex, &user_codex));
        }
    }
    rows.push(mcp_row(
        root,
        Harness::Cursor,
        &root.join(mcp::cursor_file()),
    ));

    let hooks = build_hooks_status(root);
    rows.push(hook_row(Some(Harness::Claude), &hooks.claude));
    rows.push(hook_row(Some(Harness::Codex), &hooks.codex));
    rows
}

fn guide_row(root: &Path, path: &Path, harness: Option<Harness>) -> SurfaceStatus {
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
        path: display_rel(root, path),
        detail,
    }
}

fn claude_import_row(root: &Path, path: &Path) -> SurfaceStatus {
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
        path: display_rel(root, path),
        detail,
    }
}

fn skill_row(root: &Path, harness: Option<Harness>, dir: &Path) -> SurfaceStatus {
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
        path: display_rel(root, dir),
        detail,
    }
}

fn mcp_row(root: &Path, harness: Harness, path: &Path) -> SurfaceStatus {
    let (state, detail) = match mcp::registered_command(path, harness) {
        Some(command) => (SurfaceState::Installed, Some(command.shell_words())),
        None if path.is_file() => (SurfaceState::Absent, Some("no fallow entry".to_string())),
        None => (SurfaceState::Absent, None),
    };
    SurfaceStatus {
        harness: Some(harness),
        step: Step::Mcp,
        state,
        path: display_rel(root, path),
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

fn print_human(report: &StatusReport) {
    eprintln!("fallow agent status");
    eprintln!("  root: {}", report.root);
    if report.detected.is_empty() {
        eprintln!("  detected: none");
    } else {
        for detection in &report.detected {
            eprintln!(
                "  detected: {} ({})",
                detection.harness.as_str(),
                detection.evidence.join(", ")
            );
        }
    }
    eprintln!();
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
            Some(detail) => eprintln!("  {:<40}  {state:<10} {label}  {detail}", row.path),
            None => eprintln!("  {:<40}  {state:<10} {label}", row.path),
        }
    }
}
