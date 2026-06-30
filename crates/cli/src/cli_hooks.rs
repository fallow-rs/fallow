use std::path::Path;
use std::process::ExitCode;

use crate::error::emit_error;
use crate::init;
use crate::setup_hooks;

#[derive(clap::ValueEnum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum HooksTargetArg {
    /// Shell-level Git pre-commit hook under .git/hooks/ or .husky/.
    Git,
    /// Agent-level Claude Code / Codex gate.
    Agent,
}

#[derive(clap::Subcommand)]
pub enum HooksCli {
    /// Show installed hook state for Git, Claude, and Codex surfaces.
    Status,

    /// Install a fallow-managed hook.
    Install {
        /// Hook surface to install.
        #[arg(long, value_enum)]
        target: HooksTargetArg,

        /// Fallback base branch/ref for Git pre-commit hooks when no upstream is set.
        #[arg(long)]
        branch: Option<String>,

        /// Target a specific agent surface when --target agent is used.
        #[arg(long, value_enum)]
        agent: Option<setup_hooks::HookAgentArg>,

        /// Print what would be written without touching the filesystem.
        #[arg(long)]
        dry_run: bool,

        /// Overwrite an existing managed or user-edited hook.
        #[arg(long)]
        force: bool,

        /// Write agent hooks to the user's home directory instead of the project root.
        #[arg(long)]
        user: bool,

        /// Append `.claude/` to the project's `.gitignore` for Claude agent hooks.
        #[arg(long)]
        gitignore_claude: bool,
    },

    /// Remove a fallow-managed hook.
    Uninstall {
        /// Hook surface to remove.
        #[arg(long, value_enum)]
        target: HooksTargetArg,

        /// Target a specific agent surface when --target agent is used.
        #[arg(long, value_enum)]
        agent: Option<setup_hooks::HookAgentArg>,

        /// Print what would be removed without touching the filesystem.
        #[arg(long)]
        dry_run: bool,

        /// Remove a user-edited hook script or Git hook instead of preserving it.
        #[arg(long)]
        force: bool,

        /// Remove agent hooks from the user's home directory instead of the project root.
        #[arg(long)]
        user: bool,
    },
}

pub fn run_hooks_command(
    root: &Path,
    subcommand: HooksCli,
    output: fallow_config::OutputFormat,
) -> ExitCode {
    match subcommand {
        HooksCli::Status => setup_hooks::run_hooks_status(root, output),
        install @ HooksCli::Install { .. } => run_hooks_install(root, install, output),
        uninstall @ HooksCli::Uninstall { .. } => run_hooks_uninstall(root, &uninstall, output),
    }
}

/// Handle `fallow hooks install` for both the git and agent targets.
fn run_hooks_install(
    root: &Path,
    install: HooksCli,
    output: fallow_config::OutputFormat,
) -> ExitCode {
    let HooksCli::Install {
        target,
        branch,
        agent,
        dry_run,
        force,
        user,
        gitignore_claude,
    } = install
    else {
        unreachable!("hooks install handler only handles install commands");
    };

    match target {
        HooksTargetArg::Git => {
            if agent.is_some() || user || gitignore_claude {
                return emit_error(
                    "--agent, --user, and --gitignore-claude are only valid with `fallow hooks install --target agent`",
                    2,
                    output,
                );
            }
            init::run_git_hooks_install(&init::GitHooksInstallOptions {
                root,
                branch: branch.as_deref(),
                dry_run,
                force,
            })
        }
        HooksTargetArg::Agent => {
            if branch.is_some() {
                return emit_error(
                    "--branch is only valid with `fallow hooks install --target git`",
                    2,
                    output,
                );
            }
            setup_hooks::run_setup_hooks_with_label(
                &setup_hooks::SetupHooksOptions {
                    root,
                    agent,
                    dry_run,
                    force,
                    user,
                    gitignore_claude,
                    uninstall: false,
                },
                "fallow hooks install --target agent",
            )
        }
    }
}

/// Handle `fallow hooks uninstall` for both the git and agent targets.
fn run_hooks_uninstall(
    root: &Path,
    uninstall: &HooksCli,
    output: fallow_config::OutputFormat,
) -> ExitCode {
    let HooksCli::Uninstall {
        target,
        agent,
        dry_run,
        force,
        user,
    } = *uninstall
    else {
        unreachable!("hooks uninstall handler only handles uninstall commands");
    };

    match target {
        HooksTargetArg::Git => {
            if agent.is_some() || user {
                return emit_error(
                    "--agent and --user are only valid with `fallow hooks uninstall --target agent`",
                    2,
                    output,
                );
            }
            init::run_git_hooks_uninstall(&init::GitHooksUninstallOptions {
                root,
                dry_run,
                force,
            })
        }
        HooksTargetArg::Agent => setup_hooks::run_setup_hooks_with_label(
            &setup_hooks::SetupHooksOptions {
                root,
                agent,
                dry_run,
                force,
                user,
                gitignore_claude: false,
                uninstall: true,
            },
            "fallow hooks uninstall --target agent",
        ),
    }
}
