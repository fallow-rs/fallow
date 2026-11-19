//! Which environment variables a run reads, by surface and command.
//!
//! The table records the behaviour of each surface. A variable that the
//! table does not allow for a run stays out of every decision of that run.

use fallow_config::{CommandKind, EnvVar, Surface};

/// The production variables (`FALLOW_PRODUCTION*`) for one group of commands.
struct ProductionRow {
    commands: &'static [CommandKind],
    cli: bool,
    mcp: bool,
}

/// The rows of the production variables. A command that is in no row reads
/// no production variable. The Rust API, the Node bindings and the language
/// server read no production variable for any command.
const PRODUCTION_ROWS: [ProductionRow; 9] = [
    // Flags, then variables, then config: `crates/cli/src/lib.rs:1059`,
    // `crates/cli/src/dispatch_audit_inputs.rs:32`,
    // `crates/cli/src/dispatch.rs:1652`, `:1762`, `:2284`. MCP: the `analyze`,
    // `check_changed`, `check_architecture`, `audit`, `find_dupes`,
    // `trace_clone`, `check_health` and Code Mode `combined` tools
    // (`crates/mcp/src/tools/run_inputs.rs`).
    ProductionRow {
        commands: &[
            CommandKind::Combined,
            CommandKind::Audit,
            CommandKind::DeadCode,
            CommandKind::Dupes,
            CommandKind::Health,
        ],
        cli: true,
        mcp: true,
    },
    // Flags, then variables, then config: the commands resolve a run in
    // `crates/cli/src/dispatch.rs`. MCP: `feature_flags`, `project_info` and
    // `list_boundaries` (`crates/mcp/src/tools/run_inputs.rs`); `fix` and
    // `suppressions` run the CLI (`crates/mcp/src/tools/fix.rs:44`,
    // `crates/mcp/src/tools/suppressions.rs:45`).
    ProductionRow {
        commands: &[
            CommandKind::List,
            CommandKind::Fix,
            CommandKind::Watch,
            CommandKind::Viz,
            CommandKind::Flags,
            CommandKind::Suppressions,
        ],
        cli: true,
        mcp: true,
    },
    // `crates/cli/src/dispatch.rs:495`. MCP: `trace_dependency`.
    ProductionRow {
        commands: &[CommandKind::TraceDependency],
        cli: true,
        mcp: true,
    },
    // `crates/cli/src/dispatch_baselines.rs:62`.
    ProductionRow {
        commands: &[CommandKind::BaselinesPrune],
        cli: true,
        mcp: false,
    },
    // Config only: `crates/cli/src/trace_path.rs`, `docs/environment-variables.md:69`.
    // MCP: `trace_import_path` and `trace_error` take the parameter only.
    ProductionRow {
        commands: &[CommandKind::TracePath, CommandKind::TraceError],
        cli: false,
        mcp: false,
    },
    // The export and file traces. The CLI runs them as `fallow dead-code
    // --trace` and `--trace-file`, a dead-code command. The MCP tools
    // `trace_export` and `trace_file` read the variables on both routes.
    ProductionRow {
        commands: &[CommandKind::TraceChain],
        cli: false,
        mcp: true,
    },
    // Config only, a production flag is an error: `crates/cli/src/lib.rs:216`
    // (security), `crates/cli/src/lib.rs:980` with `crates/api/src/doctor.rs:90`
    // (doctor).
    ProductionRow {
        commands: &[CommandKind::Security, CommandKind::Doctor],
        cli: false,
        mcp: false,
    },
    // The CLI starts each analysis of `inspect` as a child process that
    // inherits the environment (`crates/cli/src/inspect.rs:917`), and
    // `build_child_args` (`:954`) passes a production flag only when one is
    // given. So the children read the variables.
    ProductionRow {
        commands: &[CommandKind::Inspect],
        cli: true,
        mcp: false,
    },
    // Config only or a local flag: `crates/cli/src/coverage/analyze.rs:502`,
    // `crates/cli/src/coverage/upload_common.rs:362`,
    // `crates/cli/src/guard.rs:28`, `crates/cli/src/rule_pack/list.rs:11`,
    // `crates/api/src/runtime/mutation.rs:233`,
    // `crates/api/src/runtime/similar_code.rs:1562`,
    // `crates/cli/src/plugin_check.rs:74`,
    // `crates/mcp/src/tools/decision_surface.rs:36`. The editor uses the
    // `fallow.production` setting.
    ProductionRow {
        commands: &[
            CommandKind::CoverageAnalyze,
            CommandKind::CoverageUpload,
            CommandKind::Guard,
            CommandKind::RulePack,
            CommandKind::Mutation,
            CommandKind::SimilarCode,
            CommandKind::PluginCheck,
            CommandKind::DecisionSurface,
            CommandKind::Editor,
        ],
        cli: false,
        mcp: false,
    },
];

/// Whether a run of `command` on `surface` reads `var`.
pub fn applies(var: EnvVar, surface: Surface, command: CommandKind) -> bool {
    match var {
        EnvVar::Production
        | EnvVar::ProductionDeadCode
        | EnvVar::ProductionHealth
        | EnvVar::ProductionDupes => production_applies(surface, command),
        // Every host applies them after resolution:
        // `crates/engine/src/project_config/mod.rs`, `apply_env_overrides_from`.
        // Each config load passes the timeout of the snapshot to the fetcher
        // of a remote `extends` entry: `ConfigFileOptions::new`.
        EnvVar::MaxFileSize
        | EnvVar::CacheDir
        | EnvVar::CacheMaxSize
        | EnvVar::PackageBaselines
        | EnvVar::ExtendsTimeoutSecs => true,
        // The analysis scope of the MCP ambient fields
        // (`shared_analysis_options` in `crates/mcp/src/tools/api_runtime.rs`).
        EnvVar::ChangedSince => surface == Surface::Mcp,
        // MCP: `env_diff_file` in `crates/mcp/src/tools/api_runtime.rs`. The CLI sets up the
        // diff filter for every command (`crates/cli/src/cli_startup.rs:844`),
        // and the filter reads the variable when no flag gives a diff
        // (`crates/cli/src/report/ci/diff_filter.rs:150`).
        EnvVar::DiffFile => matches!(surface, Surface::Cli | Surface::Mcp),
        // `docs/environment-variables.md:99`: the `health`, bare, `audit` and
        // `viz` CLI commands (`crates/cli/src/dispatch.rs:2179`), and the MCP
        // `audit` and `check_health` tools and the Code Mode `combined` call
        // (`resolve_typed_coverage_inputs` in
        // `crates/mcp/src/tools/api_runtime.rs`).
        EnvVar::Coverage | EnvVar::CoverageRoot => coverage_applies(surface, command),
        // Only the audit base of `crates/api/src/audit_run/base_ref.rs:64`
        // reads it.
        EnvVar::AuditBase => command == CommandKind::Audit,
        // `crates/api/src/runtime/type_aware_config.rs:24`: every surface
        // except the language server.
        EnvVar::TypeAware => surface != Surface::Lsp,
        // `crates/cli/src/check/mod.rs:906`, `crates/cli/src/health/mod.rs:541`.
        EnvVar::TypeAwareProjects => surface == Surface::Cli,
        // `crates/cli/src/check/mod.rs:914`, `crates/cli/src/health/mod.rs:553`,
        // and the doctor of the API (`crates/api/src/doctor.rs:713`).
        EnvVar::TypeAwareRequire => {
            surface == Surface::Cli || (surface == Surface::Api && command == CommandKind::Doctor)
        }
    }
}

const fn coverage_applies(surface: Surface, command: CommandKind) -> bool {
    match surface {
        Surface::Cli => matches!(
            command,
            CommandKind::Health | CommandKind::Combined | CommandKind::Audit | CommandKind::Viz
        ),
        Surface::Mcp => matches!(
            command,
            CommandKind::Health | CommandKind::Audit | CommandKind::Combined
        ),
        Surface::Api | Surface::Node | Surface::Lsp => false,
    }
}

fn production_applies(surface: Surface, command: CommandKind) -> bool {
    let Some(row) = PRODUCTION_ROWS
        .iter()
        .find(|row| row.commands.contains(&command))
    else {
        return false;
    };
    match surface {
        Surface::Cli => row.cli,
        Surface::Mcp => row.mcp,
        Surface::Api | Surface::Node | Surface::Lsp => false,
    }
}

#[cfg(test)]
mod tests {
    use rustc_hash::FxHashSet;

    use super::*;

    #[test]
    fn each_command_is_in_at_most_one_production_row() {
        let mut seen = FxHashSet::default();
        for row in &PRODUCTION_ROWS {
            for command in row.commands {
                assert!(seen.insert(*command), "{command:?} is in two rows");
            }
        }
    }
}
