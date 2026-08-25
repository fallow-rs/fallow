//! `fallow agent`: one-shot onboarding of coding-agent harnesses.
//!
//! `fallow agent install` composes the pieces that previously lived behind
//! separate entry points (`init --agents`, `hooks install --target agent`,
//! the README's MCP snippet, and the skill shipped under
//! `node_modules/fallow/skills/fallow`) into one idempotent pass per detected
//! harness. Every file or block it writes carries a versioned
//! `fallow:agent-install` marker so `status` can report it and `uninstall`
//! can remove exactly that content and nothing else.

mod guide;
mod hooks;
pub mod hosts;
mod mcp;
mod skill;
mod status;

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use serde::Serialize;

pub use crate::setup_hooks::Mode;
use crate::setup_hooks::{display_rel, home_dir};

pub use status::run_agent_status;

/// Marker grammar version. Bump when the marker format changes so `status`
/// can tell an older install apart from a corrupt one.
pub const MARKER_VERSION: &str = "v1";
/// Common prefix of every marker this command writes.
pub const MARKER_PREFIX: &str = "fallow:agent-install";

/// A coding-agent harness fallow knows how to wire up.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, clap::ValueEnum)]
#[serde(rename_all = "lowercase")]
pub enum Harness {
    /// Claude Code: `.mcp.json`, `.claude/skills/`, `.claude/settings.json` gate, `CLAUDE.md` import.
    Claude,
    /// OpenAI Codex CLI: `.codex/config.toml`, `.agents/skills/`, `AGENTS.md` block.
    Codex,
    /// Cursor: `.cursor/mcp.json`, `.agents/skills/`, `AGENTS.md`.
    Cursor,
}

impl Harness {
    pub const ALL: [Self; 3] = [Self::Claude, Self::Codex, Self::Cursor];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Claude => "claude",
            Self::Codex => "codex",
            Self::Cursor => "cursor",
        }
    }

    pub const fn display_name(self) -> &'static str {
        match self {
            Self::Claude => "Claude Code",
            Self::Codex => "Codex",
            Self::Cursor => "Cursor",
        }
    }
}

/// `--harness` argument: explicit harnesses or automatic detection.
#[derive(Clone, Copy, Debug, PartialEq, Eq, clap::ValueEnum)]
pub enum HarnessArg {
    /// Select every harness with evidence in the project, home directory, or environment.
    Auto,
    /// Claude Code.
    Claude,
    /// OpenAI Codex CLI.
    Codex,
    /// Cursor.
    Cursor,
}

/// One installable piece. `--without` skips it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, clap::ValueEnum)]
#[serde(rename_all = "lowercase")]
pub enum Step {
    /// `AGENTS.md` scaffold or task-map block, plus the `CLAUDE.md` import for Claude Code.
    Guide,
    /// The fallow skill under `.claude/skills/` or `.agents/skills/`.
    Skill,
    /// MCP server registration for the harness.
    Mcp,
    /// The commit and push gate (`fallow hooks install --target agent`).
    Hooks,
}

impl Step {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Guide => "guide",
            Self::Skill => "skill",
            Self::Mcp => "mcp",
            Self::Hooks => "hooks",
        }
    }
}

/// What happened to one path during install or uninstall.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StepStatus {
    /// File or block created or updated (or would be, under `--dry-run`).
    Written,
    /// Managed content removed (or would be, under `--dry-run`).
    Removed,
    /// Already in the desired state.
    Unchanged,
    /// Nothing to do for this harness or scope; `reason` says why.
    Skipped,
    /// Existing content is not fallow-managed; left untouched without `--force`.
    Refused,
    /// I/O or parse error; `detail` carries the message.
    Failed,
}

/// Whether a path is normally committed with the project or stays with the
/// person who ran the command.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Scope {
    Shared,
    Local,
}

/// One row of the install, uninstall, or status report.
#[derive(Clone, Debug, Serialize)]
pub struct StepReport {
    /// `None` for harness-neutral files such as `AGENTS.md` or `.agents/skills/`.
    pub harness: Option<Harness>,
    pub step: Step,
    pub status: StepStatus,
    pub scope: Scope,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

impl StepReport {
    pub fn new(harness: Option<Harness>, step: Step, status: StepStatus, scope: Scope) -> Self {
        Self {
            harness,
            step,
            status,
            scope,
            path: None,
            reason: None,
            detail: None,
        }
    }

    pub fn path(mut self, root: &Path, path: &Path) -> Self {
        self.path = Some(display_rel(root, path));
        self
    }

    pub fn reason(mut self, reason: &'static str) -> Self {
        self.reason = Some(reason);
        self
    }

    pub fn detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = Some(detail.into());
        self
    }

    pub fn failed(
        harness: Option<Harness>,
        step: Step,
        scope: Scope,
        message: impl Into<String>,
    ) -> Self {
        Self::new(harness, step, StepStatus::Failed, scope).detail(message)
    }
}

/// A follow-up the user or agent should run after the install.
#[derive(Clone, Debug, Serialize)]
pub struct NextStep {
    pub id: &'static str,
    pub command: String,
    pub reason: String,
}

/// Options for `fallow agent install`.
pub struct AgentInstallOptions<'a> {
    pub root: &'a Path,
    /// True when `--root` was passed explicitly; otherwise the git toplevel wins.
    pub root_explicit: bool,
    pub harnesses: &'a [HarnessArg],
    pub user: bool,
    pub without: &'a [Step],
    pub dry_run: bool,
    pub force: bool,
    pub approve: bool,
    pub gitignore_claude: bool,
}

/// Options for `fallow agent uninstall`.
pub struct AgentUninstallOptions<'a> {
    pub root: &'a Path,
    pub root_explicit: bool,
    pub harnesses: &'a [HarnessArg],
    pub user: bool,
    pub dry_run: bool,
    pub force: bool,
}

/// Shared execution context for every step.
pub struct Ctx {
    pub root: PathBuf,
    pub home: Option<PathBuf>,
    pub user: bool,
    pub dry_run: bool,
    pub force: bool,
    pub approve: bool,
    pub gitignore_claude: bool,
    pub mode: Mode,
}

impl Ctx {
    /// Base directory for a harness-owned file: `$HOME` under `--user`,
    /// otherwise the project root.
    pub fn scope_base(&self) -> Result<&Path, String> {
        if self.user {
            self.home.as_deref().ok_or_else(|| {
                "Cannot resolve the user home directory; unset --user or set $HOME.".to_string()
            })
        } else {
            Ok(&self.root)
        }
    }

    pub const fn scope(&self) -> Scope {
        if self.user {
            Scope::Local
        } else {
            Scope::Shared
        }
    }
}

#[derive(Serialize)]
struct Report {
    root: String,
    mode: &'static str,
    dry_run: bool,
    harnesses: Vec<Harness>,
    /// True when the harness list came from detection rather than `--harness`.
    detected: bool,
    evidence: Vec<hosts::Detection>,
    steps: Vec<StepReport>,
    next_steps: Vec<NextStep>,
}

/// Pick the project root: an explicit `--root` is used as given; otherwise
/// the git toplevel of the resolved root, so running from a monorepo package
/// directory still writes `.mcp.json` and `AGENTS.md` where the harnesses
/// read them.
pub fn resolve_root(root: &Path, root_explicit: bool) -> PathBuf {
    if root_explicit {
        return dunce::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
    }
    crate::base_worktree::git_toplevel(root)
        .unwrap_or_else(|| dunce::canonicalize(root).unwrap_or_else(|_| root.to_path_buf()))
}

fn select_harnesses(
    requested: &[HarnessArg],
    root: &Path,
    home: Option<&Path>,
) -> (Vec<Harness>, bool, Vec<hosts::Detection>) {
    let auto = requested.is_empty() || requested.contains(&HarnessArg::Auto);
    let evidence = hosts::detect(root, home);
    if auto {
        let harnesses = evidence.iter().map(|d| d.harness).collect();
        return (harnesses, true, evidence);
    }
    let mut harnesses: Vec<Harness> = Harness::ALL
        .into_iter()
        .filter(|harness| {
            requested.iter().any(|arg| match arg {
                HarnessArg::Claude => *harness == Harness::Claude,
                HarnessArg::Codex => *harness == Harness::Codex,
                HarnessArg::Cursor => *harness == Harness::Cursor,
                HarnessArg::Auto => false,
            })
        })
        .collect();
    harnesses.dedup();
    (harnesses, false, evidence)
}

/// Entry point for `fallow agent install`.
pub fn run_agent_install(
    opts: &AgentInstallOptions<'_>,
    output: fallow_config::OutputFormat,
    json_style: crate::json_style::JsonStyle,
) -> ExitCode {
    let root = resolve_root(opts.root, opts.root_explicit);
    let home = home_dir();
    let (harnesses, detected, evidence) = select_harnesses(opts.harnesses, &root, home.as_deref());
    let ctx = Ctx {
        root: root.clone(),
        home,
        user: opts.user,
        dry_run: opts.dry_run,
        force: opts.force,
        approve: opts.approve,
        gitignore_claude: opts.gitignore_claude,
        mode: Mode::Install,
    };
    let skip = |step: Step| opts.without.contains(&step);

    let mut steps: Vec<StepReport> = Vec::new();
    if !skip(Step::Guide) {
        steps.extend(guide::install(&ctx, &harnesses));
    }
    if !skip(Step::Skill) {
        steps.extend(skill::install(&ctx, &harnesses));
    }
    let mcp_command = if skip(Step::Mcp) {
        None
    } else {
        let command = mcp::resolve_command(&root);
        steps.extend(mcp::install(&ctx, &harnesses, command.as_ref()));
        command
    };
    if !skip(Step::Hooks) {
        steps.extend(hooks::install(&ctx, &harnesses));
    }

    let next_steps = next_steps(&ctx, &harnesses, detected, mcp_command.as_ref(), &steps);
    let report = Report {
        root: root.display().to_string(),
        mode: "install",
        dry_run: opts.dry_run,
        harnesses,
        detected,
        evidence,
        steps,
        next_steps,
    };
    render(&report, output, json_style)
}

/// Entry point for `fallow agent uninstall`.
pub fn run_agent_uninstall(
    opts: &AgentUninstallOptions<'_>,
    output: fallow_config::OutputFormat,
    json_style: crate::json_style::JsonStyle,
) -> ExitCode {
    let root = resolve_root(opts.root, opts.root_explicit);
    let home = home_dir();
    let (harnesses, detected, evidence) = select_harnesses(opts.harnesses, &root, home.as_deref());
    let ctx = Ctx {
        root: root.clone(),
        home,
        user: opts.user,
        dry_run: opts.dry_run,
        force: opts.force,
        approve: false,
        gitignore_claude: false,
        mode: Mode::Uninstall,
    };

    let mut steps: Vec<StepReport> = Vec::new();
    steps.extend(hooks::uninstall(&ctx, &harnesses));
    steps.extend(mcp::uninstall(&ctx, &harnesses));
    steps.extend(skill::uninstall(&ctx, &harnesses));
    steps.extend(guide::uninstall(&ctx, &harnesses));

    let report = Report {
        root: root.display().to_string(),
        mode: "uninstall",
        dry_run: opts.dry_run,
        harnesses,
        detected,
        evidence,
        steps,
        next_steps: Vec::new(),
    };
    render(&report, output, json_style)
}

fn next_steps(
    ctx: &Ctx,
    harnesses: &[Harness],
    detected: bool,
    mcp_command: Option<&mcp::McpCommand>,
    steps: &[StepReport],
) -> Vec<NextStep> {
    let mut next: Vec<NextStep> = Vec::new();
    if detected && harnesses.is_empty() {
        next.push(NextStep {
            id: "choose-harness",
            command: "fallow agent install --harness claude".to_string(),
            reason: "No harness was detected; pass --harness claude, codex, or cursor to wire one explicitly."
                .to_string(),
        });
    }
    if let Some(command) = mcp_command {
        if harnesses.contains(&Harness::Codex) {
            next.push(NextStep {
                id: "codex-mcp-add",
                command: format!("codex mcp add fallow -- {}", command.shell_words()),
                reason: "A project-level .codex/config.toml only applies once Codex trusts the project; the user-level entry works immediately."
                    .to_string(),
            });
        }
        if harnesses.contains(&Harness::Claude) && ctx.user {
            next.push(NextStep {
                id: "claude-mcp-add-user",
                command: format!("claude mcp add --scope user fallow -- {}", command.shell_words()),
                reason: "fallow does not edit ~/.claude.json; register the user-scope server through the Claude CLI."
                    .to_string(),
            });
        }
        let approval_skipped = steps.iter().any(|step| {
            step.harness == Some(Harness::Claude)
                && step.step == Step::Mcp
                && step.reason == Some("approval_not_requested")
        });
        if approval_skipped {
            next.push(NextStep {
                id: "claude-approve-mcp",
                command: "fallow agent install --harness claude --approve".to_string(),
                reason: "Claude Code asks before starting a project-scoped MCP server; --approve records that approval for you in .claude/settings.local.json."
                    .to_string(),
            });
        }
    }
    if !has_config_file(&ctx.root) {
        next.push(NextStep {
            id: "recommend-config",
            command: "fallow recommend --format json".to_string(),
            reason: "No fallow config was found; recommend proposes one from the detected stack without writing anything."
                .to_string(),
        });
    }
    next
}

fn has_config_file(root: &Path) -> bool {
    [
        ".fallowrc.json",
        ".fallowrc.jsonc",
        "fallow.toml",
        ".fallow.toml",
    ]
    .iter()
    .any(|name| root.join(name).is_file())
}

fn render(
    report: &Report,
    output: fallow_config::OutputFormat,
    json_style: crate::json_style::JsonStyle,
) -> ExitCode {
    let blocked = report
        .steps
        .iter()
        .any(|step| matches!(step.status, StepStatus::Refused | StepStatus::Failed));
    let exit = if blocked {
        ExitCode::from(2)
    } else {
        ExitCode::SUCCESS
    };
    match output {
        fallow_config::OutputFormat::Json => match json_style.serialize(report) {
            Ok(json) => {
                crate::report::sink::outln!("{json}");
                exit
            }
            Err(error) => crate::error::emit_error_with_style(
                &format!("failed to serialize agent report: {error}"),
                2,
                output,
                json_style,
            ),
        },
        fallow_config::OutputFormat::Human => {
            print_human(report);
            exit
        }
        _ => crate::error::emit_error("agent commands support human and json output", 2, output),
    }
}

fn print_human(report: &Report) {
    let suffix = if report.dry_run { " (dry run)" } else { "" };
    eprintln!("fallow agent {}{suffix}", report.mode);
    eprintln!("  root: {}", report.root);
    let names: Vec<&str> = report.harnesses.iter().map(|h| h.as_str()).collect();
    let origin = if report.detected {
        "detected"
    } else {
        "from --harness"
    };
    if names.is_empty() {
        eprintln!("  harnesses: none {origin}; harness-neutral files only");
    } else {
        eprintln!("  harnesses: {} ({origin})", names.join(", "));
    }

    let shared: Vec<&StepReport> = report
        .steps
        .iter()
        .filter(|step| step.scope == Scope::Shared)
        .collect();
    let local: Vec<&StepReport> = report
        .steps
        .iter()
        .filter(|step| step.scope == Scope::Local)
        .collect();
    if !shared.is_empty() {
        eprintln!();
        eprintln!("Shared with your team (commit these):");
        for step in shared {
            print_step(step, report.dry_run);
        }
    }
    if !local.is_empty() {
        eprintln!();
        eprintln!("Local to you:");
        for step in local {
            print_step(step, report.dry_run);
        }
    }
    if !report.next_steps.is_empty() {
        eprintln!();
        eprintln!("Next steps:");
        for next in &report.next_steps {
            eprintln!("  {}", next.command);
            eprintln!("    {}", next.reason);
        }
    }
}

fn print_step(step: &StepReport, dry_run: bool) {
    let status = match (step.status, dry_run) {
        (StepStatus::Written, true) => "would write",
        (StepStatus::Written, false) => "written",
        (StepStatus::Removed, true) => "would remove",
        (StepStatus::Removed, false) => "removed",
        (StepStatus::Unchanged, _) => "unchanged",
        (StepStatus::Skipped, _) => "skipped",
        (StepStatus::Refused, _) => "refused",
        (StepStatus::Failed, _) => "failed",
    };
    let label = match step.harness {
        Some(harness) => format!("{} ({})", step.step.as_str(), harness.as_str()),
        None => step.step.as_str().to_string(),
    };
    let path = step.path.as_deref().unwrap_or("-");
    let mut note = String::new();
    if let Some(reason) = step.reason {
        note.push_str(reason);
    }
    if let Some(detail) = &step.detail {
        if !note.is_empty() {
            note.push_str(": ");
        }
        note.push_str(detail);
    }
    if note.is_empty() {
        eprintln!("  {path:<40}  {status:<12} {label}");
    } else {
        eprintln!("  {path:<40}  {status:<12} {label}  {note}");
    }
}

#[cfg(test)]
mod tests;
