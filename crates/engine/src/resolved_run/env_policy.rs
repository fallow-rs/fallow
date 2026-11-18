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
const PRODUCTION_ROWS: [ProductionRow; 7] = [
    // Flags, then variables, then config: `crates/cli/src/lib.rs:1059`,
    // `crates/cli/src/dispatch_audit_inputs.rs:32`,
    // `crates/cli/src/dispatch.rs:1652`, `:1762`, `:2284`. MCP: the `analyze`,
    // `audit`, `dupes`, `check_health` and combined tools call
    // `typed_production` (`crates/mcp/src/tools/api_runtime.rs:201`).
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
    // `crates/cli/src/dispatch.rs:1626`, `:1575`, `:1542`, `:1522`, `:1207`,
    // `:1254`. MCP: `flags` calls `typed_production`
    // (`crates/mcp/src/tools/flags.rs:130`); `fix` and `suppressions` run the
    // CLI (`crates/mcp/src/tools/fix.rs:44`,
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
    // `crates/cli/src/dispatch.rs:495`. MCP: `trace_dependency` calls
    // `typed_production` (`crates/mcp/src/tools/trace.rs:619`).
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
    // MCP: `trace_import_path` and `trace_error` take the parameter only
    // (`crates/mcp/src/tools/trace.rs:629`).
    ProductionRow {
        commands: &[
            CommandKind::TracePath,
            CommandKind::TraceChain,
            CommandKind::TraceError,
        ],
        cli: false,
        mcp: false,
    },
    // Config only, a production flag is an error: `crates/cli/src/lib.rs:216`
    // (security), `crates/cli/src/lib.rs:980` with `crates/api/src/doctor.rs:90`
    // (doctor).
    ProductionRow {
        commands: &[CommandKind::Security, CommandKind::Doctor],
        cli: false,
        mcp: false,
    },
    // Config only or a local flag: `crates/cli/src/inspect.rs:321`,
    // `crates/cli/src/coverage/analyze.rs:502`,
    // `crates/cli/src/coverage/upload_common.rs:362`,
    // `crates/cli/src/guard.rs:28`, `crates/cli/src/rule_pack/list.rs:11`,
    // `crates/api/src/runtime/mutation.rs:233`,
    // `crates/api/src/runtime/similar_code.rs:1562`,
    // `crates/cli/src/plugin_check.rs:74`,
    // `crates/mcp/src/tools/decision_surface.rs:36`. The editor uses the
    // `fallow.production` setting.
    ProductionRow {
        commands: &[
            CommandKind::Inspect,
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
        // The fetcher of a remote `extends` entry reads the timeout:
        // `crates/config/src/config/parsing.rs:415`.
        EnvVar::MaxFileSize
        | EnvVar::CacheDir
        | EnvVar::CacheMaxSize
        | EnvVar::PackageBaselines
        | EnvVar::ExtendsTimeoutSecs => true,
        // The analysis scope of the MCP ambient fields.
        EnvVar::ChangedSince | EnvVar::DiffFile => surface == Surface::Mcp,
        // `docs/environment-variables.md:99`: the CLI and the MCP `audit` and
        // `check_health` tools.
        EnvVar::Coverage | EnvVar::CoverageRoot => matches!(surface, Surface::Cli | Surface::Mcp),
        // `crates/api/src/runtime/type_aware_config.rs`, the audit base of
        // `crates/api/src/audit_run/base_ref.rs`: every surface except the
        // language server.
        EnvVar::AuditBase
        | EnvVar::TypeAware
        | EnvVar::TypeAwareProjects
        | EnvVar::TypeAwareRequire => surface != Surface::Lsp,
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
