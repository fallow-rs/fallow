//! Hooks step: the commit and push gate, delegated to the
//! `hooks install --target agent` engine so both entry points stay
//! byte-identical.

use super::{Ctx, Harness, Mode, Scope, Step, StepReport, StepStatus};
use crate::setup_hooks::{
    AgentsOutcome, HookAgentArg, ScriptOutcome, SettingsOutcome, SetupHooksOptions,
    execute_agent_hooks,
};

pub fn install(ctx: &Ctx, harnesses: &[Harness]) -> Vec<StepReport> {
    harnesses.iter().flat_map(|h| run(ctx, *h)).collect()
}

pub fn uninstall(ctx: &Ctx, harnesses: &[Harness]) -> Vec<StepReport> {
    harnesses.iter().flat_map(|h| run(ctx, *h)).collect()
}

fn run(ctx: &Ctx, harness: Harness) -> Vec<StepReport> {
    let agent = match harness {
        Harness::Claude => HookAgentArg::Claude,
        Harness::Codex => {
            if ctx.user {
                return vec![
                    StepReport::new(
                        Some(harness),
                        Step::Hooks,
                        StepStatus::Skipped,
                        Scope::Local,
                    )
                    .reason("user_scope_unsupported")
                    .detail("the Codex gate lives in the project AGENTS.md"),
                ];
            }
            HookAgentArg::Codex
        }
        Harness::Cursor => {
            return vec![
                StepReport::new(Some(harness), Step::Hooks, StepStatus::Skipped, ctx.scope())
                    .reason("unsupported_harness")
                    .detail("Cursor's beforeShellExecution hook uses a different contract; Cursor still reads AGENTS.md"),
            ];
        }
    };
    let opts = SetupHooksOptions {
        root: &ctx.root,
        agent: Some(agent),
        dry_run: ctx.dry_run,
        force: ctx.force,
        user: ctx.user,
        gitignore_claude: ctx.gitignore_claude,
        uninstall: ctx.mode == Mode::Uninstall,
    };
    let report = match execute_agent_hooks(&opts, ctx.mode) {
        Ok(Some(report)) => report,
        Ok(None) => return Vec::new(),
        Err(message) => {
            return vec![StepReport::failed(
                Some(harness),
                Step::Hooks,
                ctx.scope(),
                message,
            )];
        }
    };

    let mut steps: Vec<StepReport> = Vec::new();
    if let Some(claude) = report.claude {
        steps.extend(claude_steps(ctx, harness, &claude));
    }
    if let Some(codex) = report.codex {
        steps.push(codex_step(ctx, harness, &codex));
    }
    steps
}

fn claude_steps(
    ctx: &Ctx,
    harness: Harness,
    claude: &crate::setup_hooks::ClaudeReport,
) -> Vec<StepReport> {
    let mut steps: Vec<StepReport> = Vec::new();
    {
        let settings_status = match (&claude.settings_outcome, ctx.mode) {
            (SettingsOutcome::Created | SettingsOutcome::Updated { .. }, Mode::Install) => {
                StepStatus::Written
            }
            (SettingsOutcome::Updated { .. }, Mode::Uninstall) => StepStatus::Removed,
            (SettingsOutcome::Created, Mode::Uninstall) => StepStatus::Unchanged,
            (SettingsOutcome::Unchanged { .. } | SettingsOutcome::NotPresent, _) => {
                StepStatus::Unchanged
            }
        };
        steps.push(
            StepReport::new(Some(harness), Step::Hooks, settings_status, ctx.scope())
                .path(ctx, &claude.settings_path)
                .detail("PreToolUse gate handler"),
        );
        let (script_status, reason) = match claude.script_outcome {
            ScriptOutcome::Created | ScriptOutcome::Updated => (StepStatus::Written, None),
            ScriptOutcome::Removed => (StepStatus::Removed, None),
            ScriptOutcome::Unchanged | ScriptOutcome::NotPresent => (StepStatus::Unchanged, None),
            ScriptOutcome::UserEditedPreserved => (StepStatus::Refused, Some("user_edited")),
        };
        let mut script = StepReport::new(Some(harness), Step::Hooks, script_status, ctx.scope())
            .path(ctx, &claude.script_path)
            .detail("gate script");
        if matches!(claude.script_outcome, ScriptOutcome::UserEditedPreserved) {
            script.detail = Some("no fallow marker; pass --force to replace it".to_string());
        }
        script.reason = reason;
        steps.push(script);
    }
    steps
}

fn codex_step(ctx: &Ctx, harness: Harness, codex: &crate::setup_hooks::CodexReport) -> StepReport {
    {
        let (status, reason) = match codex.outcome {
            AgentsOutcome::Inserted | AgentsOutcome::Replaced => (StepStatus::Written, None),
            AgentsOutcome::Removed => (StepStatus::Removed, None),
            AgentsOutcome::Unchanged | AgentsOutcome::NotPresent => (StepStatus::Unchanged, None),
            AgentsOutcome::MalformedPreserved => {
                (StepStatus::Refused, Some("managed_block_malformed"))
            }
        };
        let detail = match codex.outcome {
            AgentsOutcome::MalformedPreserved => {
                "fallow markers are out of order; repair AGENTS.md by hand".to_string()
            }
            _ => "gate block".to_string(),
        };
        let mut step = StepReport::new(Some(harness), Step::Hooks, status, Scope::Shared)
            .path(ctx, &codex.agents_path)
            .detail(detail);
        step.reason = reason;
        step
    }
}
