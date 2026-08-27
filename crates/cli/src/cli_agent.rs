use std::path::Path;
use std::process::ExitCode;

use crate::agent_install::{
    AgentInstallOptions, AgentUninstallOptions, HarnessArg, Step, run_agent_install,
    run_agent_status, run_agent_uninstall,
};

#[derive(clap::Subcommand)]
pub enum AgentCli {
    /// Wire fallow into the coding-agent harnesses used by this project in
    /// one pass: the `AGENTS.md` task map (plus a `CLAUDE.md` import for
    /// Claude Code), the fallow skill, the MCP server registration, and the
    /// commit/push gate. Every file or block is marked so `status` can report
    /// it and `uninstall` removes exactly that content. Re-running is
    /// byte-stable. Without `--harness` the harnesses are detected from the
    /// project, the home directory, and the session environment; when nothing
    /// is detected only harness-neutral files are written.
    Install {
        /// Harness to wire; repeatable. Defaults to `auto`.
        #[arg(long, value_enum, value_delimiter = ',')]
        harness: Vec<HarnessArg>,

        /// Write user-scoped files (skill and MCP config under $HOME) instead
        /// of project files. The `AGENTS.md` guide is project-only and is
        /// skipped under this flag.
        #[arg(long)]
        user: bool,

        /// Skip a step; repeatable.
        #[arg(long, value_enum, value_delimiter = ',')]
        without: Vec<Step>,

        /// Print the plan without touching the filesystem.
        #[arg(long)]
        dry_run: bool,

        /// Replace skills, hook scripts, or config files fallow did not write.
        #[arg(long)]
        force: bool,

        /// Pre-approve the project-scoped MCP server for yourself by listing it
        /// in `.claude/settings.local.json` (Claude Code asks otherwise).
        #[arg(long)]
        approve: bool,

        /// Append `.claude/` to the project's `.gitignore`.
        #[arg(long)]
        gitignore_claude: bool,
    },

    /// Show which agent surfaces are installed, stale, absent, or foreign.
    Status,

    /// Remove every fallow-managed agent surface. Files fallow authored whole
    /// are deleted only while they still match what fallow wrote; otherwise
    /// only the managed blocks are removed.
    Uninstall {
        /// Harness to unwire; repeatable. Defaults to `auto`.
        #[arg(long, value_enum, value_delimiter = ',')]
        harness: Vec<HarnessArg>,

        /// Remove user-scoped files under $HOME instead of project files.
        #[arg(long)]
        user: bool,

        /// Print what would be removed without touching the filesystem.
        #[arg(long)]
        dry_run: bool,

        /// Also remove skills and hook scripts fallow did not write.
        #[arg(long)]
        force: bool,
    },
}

pub fn run_agent_command(
    root: &Path,
    root_explicit: bool,
    subcommand: AgentCli,
    output: fallow_config::OutputFormat,
    json_style: crate::json_style::JsonStyle,
) -> ExitCode {
    match subcommand {
        AgentCli::Install {
            harness,
            user,
            without,
            dry_run,
            force,
            approve,
            gitignore_claude,
        } => run_agent_install(
            &AgentInstallOptions {
                root,
                root_explicit,
                harnesses: &harness,
                user,
                without: &without,
                dry_run,
                force,
                approve,
                gitignore_claude,
            },
            output,
            json_style,
        ),
        AgentCli::Status => run_agent_status(root, root_explicit, output, json_style),
        AgentCli::Uninstall {
            harness,
            user,
            dry_run,
            force,
        } => run_agent_uninstall(
            &AgentUninstallOptions {
                root,
                root_explicit,
                harnesses: &harness,
                user,
                dry_run,
                force,
            },
            output,
            json_style,
        ),
    }
}
