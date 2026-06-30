use std::path::Path;
use std::process::ExitCode;

use crate::impact;
use crate::telemetry;

#[derive(Clone, Copy, clap::Subcommand)]
pub enum ImpactCli {
    /// Enable local Impact tracking for this project.
    Enable,
    /// Disable Impact tracking (existing history is retained).
    Disable,
    /// Set the user-global default for new projects (on or off). A per-project
    /// `enable`/`disable` always wins over this default.
    Default {
        /// `on` to record in every project by default, `off` to require an
        /// explicit per-project `enable`.
        #[arg(value_enum)]
        state: ToggleState,
    },
    /// Delete this project's stored history (or all projects with `--all`).
    Reset {
        /// Delete every project's Impact history, not just this one.
        #[arg(long)]
        all: bool,
    },
    /// Show whether Impact tracking is enabled and how much history exists.
    Status,
}

#[derive(Clone, Copy, clap::ValueEnum)]
pub enum ToggleState {
    On,
    Off,
}

/// Row ordering for `fallow impact --all`.
#[derive(Clone, Copy, clap::ValueEnum)]
pub enum ImpactSortCli {
    /// Most recently recorded project first (default).
    Recent,
    /// Most findings resolved first.
    Resolved,
    /// Most commits contained first.
    Contained,
    /// Alphabetical by project label.
    Name,
}

impl ImpactSortCli {
    const fn to_impact(self) -> impact::CrossRepoSort {
        match self {
            Self::Recent => impact::CrossRepoSort::Recent,
            Self::Resolved => impact::CrossRepoSort::Resolved,
            Self::Contained => impact::CrossRepoSort::Contained,
            Self::Name => impact::CrossRepoSort::Name,
        }
    }
}

/// The `fallow impact --all` cross-repo view options, bundled so
/// `dispatch_impact` takes one parameter instead of three.
#[derive(Clone, Copy)]
pub struct ImpactCrossRepoOpts {
    pub all: bool,
    pub sort: ImpactSortCli,
    pub limit: Option<usize>,
}

pub fn dispatch_impact(
    root: &Path,
    quiet: bool,
    output: fallow_config::OutputFormat,
    subcommand: Option<ImpactCli>,
    cross_repo: ImpactCrossRepoOpts,
) -> ExitCode {
    let ImpactCrossRepoOpts { all, sort, limit } = cross_repo;
    if all {
        if subcommand.is_some() {
            return crate::emit_known_failure(
                "`fallow impact --all` is a read-only cross-repo view and cannot be combined \
                 with a subcommand (enable/disable/default/reset)",
                2,
                output,
                telemetry::FailureReason::Validation,
            );
        }
        return render_impact_all(quiet, output, sort, limit);
    }
    match subcommand {
        Some(ImpactCli::Enable) => impact_enable(root, quiet),
        Some(ImpactCli::Disable) => impact_disable(root, quiet),
        Some(ImpactCli::Default { state }) => impact_set_default(state, quiet),
        Some(ImpactCli::Reset { all }) => impact_reset(root, all, quiet),
        Some(ImpactCli::Status) | None => render_impact_status(root, quiet, output),
    }
}

/// Enable Fallow Impact for this project; print the first-enable guidance.
fn impact_enable(root: &Path, quiet: bool) -> ExitCode {
    let newly = impact::enable(root);
    if !quiet {
        if newly {
            println!(
                "Fallow Impact enabled for this project. Each `fallow audit` / pre-commit \
                 gate run is recorded in your user config dir (never written into the \
                 repo, never uploaded)."
            );
            println!(
                "Tip: run `fallow init --hooks` (or add `--gate-marker pre-commit` to \
                 your existing hook's `fallow audit` line) so blocked-then-fixed \
                 commits are recorded as contained."
            );
        } else {
            println!("Fallow Impact is already enabled.");
        }
    }
    ExitCode::SUCCESS
}

/// Disable Fallow Impact for this project; history is retained.
fn impact_disable(root: &Path, quiet: bool) -> ExitCode {
    let was_enabled = impact::disable(root);
    if !quiet {
        println!(
            "{}",
            if was_enabled {
                "Fallow Impact disabled. Existing history is retained."
            } else {
                "Fallow Impact was already disabled."
            }
        );
    }
    ExitCode::SUCCESS
}

/// Set the user-global Impact default for new projects.
fn impact_set_default(state: ToggleState, quiet: bool) -> ExitCode {
    let on = matches!(state, ToggleState::On);
    let changed = impact::set_global_default(on);
    if !quiet {
        let verb = if on { "on" } else { "off" };
        let body = if on {
            "New projects now record Impact by default. A per-project `fallow impact \
             disable` still opts that repo out."
        } else {
            "New projects no longer record by default; run `fallow impact enable` per \
             project to opt in."
        };
        if changed {
            println!("Fallow Impact default set to {verb}. {body}");
        } else {
            println!("Fallow Impact default was already {verb}.");
        }
    }
    ExitCode::SUCCESS
}

/// Reset Impact history for this project, or every project with `--all`.
fn impact_reset(root: &Path, all: bool, quiet: bool) -> ExitCode {
    if all {
        let removed = impact::reset_all();
        if !quiet {
            println!(
                "{}",
                if removed {
                    "Removed all Fallow Impact history."
                } else {
                    "No Fallow Impact history to remove."
                }
            );
        }
    } else {
        let removed = impact::reset(root);
        if !quiet {
            println!(
                "{}",
                if removed {
                    "Removed this project's Fallow Impact history."
                } else {
                    "No Fallow Impact history for this project."
                }
            );
        }
    }
    ExitCode::SUCCESS
}

fn render_impact_status(root: &Path, quiet: bool, output: fallow_config::OutputFormat) -> ExitCode {
    let store = impact::load(root);
    let report = impact::build_report(&store);
    let is_human = matches!(output, fallow_config::OutputFormat::Human);
    let rendered = match output {
        fallow_config::OutputFormat::Json => impact::render_json(&report),
        fallow_config::OutputFormat::Markdown => impact::render_markdown(&report),
        fallow_config::OutputFormat::Human => impact::render_human(&report),
        fallow_config::OutputFormat::Sarif
        | fallow_config::OutputFormat::Compact
        | fallow_config::OutputFormat::CodeClimate
        | fallow_config::OutputFormat::PrCommentGithub
        | fallow_config::OutputFormat::PrCommentGitlab
        | fallow_config::OutputFormat::ReviewGithub
        | fallow_config::OutputFormat::ReviewGitlab
        | fallow_config::OutputFormat::Badge => {
            return crate::emit_known_failure(
                "impact supports human, json, and markdown output",
                2,
                output,
                telemetry::FailureReason::UnsupportedFormat,
            );
        }
    };
    println!("{rendered}");
    if is_human && !quiet {
        println!("  Store key: {}", impact::resolved_project_key(root));
        match impact::resolved_store_path(root) {
            Some(path) => println!("  Store file: {}", path.display()),
            None => println!("  Store file: (no user config dir resolved; not persisted)"),
        }
    }
    ExitCode::SUCCESS
}

/// Render the cross-repo `fallow impact --all` roll-up. Reads the user config
/// dir; never reads `--root`. Human output adds one store-dir discoverability
/// line gated on `is_human && !quiet`; JSON/markdown stay path-free.
fn render_impact_all(
    quiet: bool,
    output: fallow_config::OutputFormat,
    sort: ImpactSortCli,
    limit: Option<usize>,
) -> ExitCode {
    let report = impact::aggregate(sort.to_impact());
    let is_human = matches!(output, fallow_config::OutputFormat::Human);
    let rendered = match output {
        fallow_config::OutputFormat::Json => impact::render_cross_repo_json(&report),
        fallow_config::OutputFormat::Markdown => impact::render_cross_repo_markdown(&report),
        fallow_config::OutputFormat::Human => impact::render_cross_repo_human(&report, limit),
        fallow_config::OutputFormat::Sarif
        | fallow_config::OutputFormat::Compact
        | fallow_config::OutputFormat::CodeClimate
        | fallow_config::OutputFormat::PrCommentGithub
        | fallow_config::OutputFormat::PrCommentGitlab
        | fallow_config::OutputFormat::ReviewGithub
        | fallow_config::OutputFormat::ReviewGitlab
        | fallow_config::OutputFormat::Badge => {
            return crate::emit_known_failure(
                "impact --all supports human, json, and markdown output",
                2,
                output,
                telemetry::FailureReason::UnsupportedFormat,
            );
        }
    };
    println!("{rendered}");
    if is_human
        && !quiet
        && let Some(dir) = impact::store_dir()
    {
        println!("  Stores: {}", dir.display());
    }
    ExitCode::SUCCESS
}
