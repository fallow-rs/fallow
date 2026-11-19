//! The raw inputs of one run, before the engine resolves them.
//!
//! Each surface (the CLI, the Rust API, the Node bindings, the language
//! server and the MCP server) translates its own flags, options, settings or
//! tool parameters into one [`RunInputs`]. The engine resolves the inputs,
//! the config file and the [`EnvSnapshot`] into one decision per setting.

mod dupes;
mod env;

use std::path::PathBuf;

use fallow_types::output_format::OutputFormat;
use fallow_types::sourced::{Given, Origin};
use serde::Serialize;

pub use dupes::DupesInputs;
pub use env::{EnvProblem, EnvSnapshot, EnvValue, EnvVar};

use crate::{PlannedAnalysis, TypeAwareRequire};

/// The type-aware settings that a surface gives, such as the
/// `--type-aware`, `--type-aware-project` and `--type-aware-require` flags.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TypeAwareInputs {
    /// Turn type-aware analysis on or off.
    pub enabled: Option<Given<bool>>,
    /// The TypeScript project configs. An empty list is not an input.
    pub projects: Option<Given<Vec<PathBuf>>>,
    /// The completeness policy.
    pub require: Option<Given<TypeAwareRequire>>,
}

/// The surface that starts a run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum Surface {
    /// The `fallow` command line.
    Cli,
    /// The Rust API of `fallow-api`.
    Api,
    /// The Node bindings.
    Node,
    /// The language server.
    Lsp,
    /// The MCP server.
    Mcp,
}

/// The command of a run. There is one variant per distinct rule for the
/// environment variables and for the selection of analyses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum CommandKind {
    /// The bare `fallow` run and the combined API route.
    Combined,
    /// `fallow audit`.
    Audit,
    /// `fallow dead-code` and the dead-code routes.
    DeadCode,
    /// `fallow dupes` and the duplication routes.
    Dupes,
    /// `fallow health` and the health routes.
    Health,
    /// `fallow list`.
    List,
    /// `fallow fix`.
    Fix,
    /// `fallow watch`.
    Watch,
    /// `fallow viz`.
    Viz,
    /// `fallow flags`.
    Flags,
    /// `fallow suppressions`.
    Suppressions,
    /// `fallow trace` for a dependency.
    TraceDependency,
    /// `fallow trace --path`.
    TracePath,
    /// `fallow trace` for a re-export chain.
    TraceChain,
    /// `fallow trace-error`.
    TraceError,
    /// `fallow security`.
    Security,
    /// `fallow inspect`.
    Inspect,
    /// `fallow coverage analyze`.
    CoverageAnalyze,
    /// `fallow coverage upload`.
    CoverageUpload,
    /// `fallow guard`.
    Guard,
    /// `fallow rule-pack`.
    RulePack,
    /// `fallow baselines prune`.
    BaselinesPrune,
    /// `fallow mutation`.
    Mutation,
    /// `fallow similar-code`.
    SimilarCode,
    /// `fallow plugin-check`.
    PluginCheck,
    /// The MCP decision surface.
    DecisionSurface,
    /// `fallow doctor`.
    Doctor,
    /// An editor session of the language server.
    Editor,
}

/// How a run selects its analyses.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SelectionInput {
    /// `--only` and `--skip` lists.
    Lists {
        /// The analyses that `--only` names.
        only: Vec<PlannedAnalysis>,
        /// The analyses that `--skip` names.
        skip: Vec<PlannedAnalysis>,
        /// The input that carries the lists.
        origin: Origin,
    },
    /// One bool for each analysis, as the options of the combined API route.
    Booleans {
        /// Run the dead-code analysis.
        dead_code: bool,
        /// Run the duplication analysis.
        duplication: bool,
        /// Run the health analysis.
        health: bool,
        /// The input that carries the bools.
        origin: Origin,
    },
    /// One optional bool for each analysis, as the options of the audit API
    /// route.
    Options {
        /// The request for the dead-code analysis.
        dead_code: Option<bool>,
        /// The request for the duplication analysis.
        duplication: Option<bool>,
        /// The request for the health analysis.
        health: Option<bool>,
        /// The input that carries the options.
        origin: Origin,
    },
    /// A command that runs exactly one analysis.
    Single(PlannedAnalysis),
}

/// What a run does when its config cannot be used.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ConfigFallback {
    /// The run fails.
    #[default]
    Fail,
    /// A discovered config that fails to load, to validate, or to give
    /// workspace metadata gives the defaults. An explicit config path still
    /// fails.
    DefaultsOnDiscoveredError,
}

/// The raw inputs of one run.
///
/// The surface and the environment snapshot are private: [`RunInputs::new`]
/// is the only way to set them, so each run names both.
#[derive(Debug, Clone)]
pub struct RunInputs {
    surface: Surface,
    env: EnvSnapshot,
    /// The command of the run.
    pub command: CommandKind,
    /// How the run selects its analyses.
    pub selection: SelectionInput,
    /// What the run does when its config cannot be used.
    pub config_fallback: ConfigFallback,
    /// The project root. Without it, the current directory of the snapshot.
    pub root: Option<Given<PathBuf>>,
    /// An explicit config file. A relative path joins the current directory
    /// of the snapshot.
    pub config_path: Option<Given<PathBuf>>,
    /// Permit `https://` entries in the `extends` chain of the config.
    pub allow_remote_extends: Option<Given<bool>>,
    /// Bypass the parse cache.
    pub no_cache: Option<Given<bool>>,
    /// The number of worker threads.
    pub threads: Option<Given<usize>>,
    /// The largest file to analyze, in megabytes. `0` means no limit.
    pub max_file_size_mb: Option<Given<u32>>,
    /// The cache directory.
    pub cache_dir: Option<Given<PathBuf>>,
    /// Read `workspaces.changedSince` of the config.
    pub package_baselines: Option<Given<bool>>,
    /// Report unused exports of entry files.
    pub include_entry_exports: Option<Given<bool>>,
    /// The production mode of every analysis.
    pub production: Option<Given<bool>>,
    /// The production mode of the dead-code analysis.
    pub production_dead_code: Option<Given<bool>>,
    /// The production mode of the health analysis.
    pub production_health: Option<Given<bool>>,
    /// The production mode of the duplication analysis.
    pub production_dupes: Option<Given<bool>>,
    /// The analysis flags of the run, such as `--unused-exports`, with the
    /// analysis that each flag belongs to.
    pub analysis_flags: Vec<(PlannedAnalysis, Origin)>,
    /// The type-aware settings that the surface gives.
    pub type_aware: TypeAwareInputs,
    /// The base ref of an audit that the caller names, such as `--base` or
    /// the `base` parameter of an MCP tool.
    pub audit_base: Option<Given<String>>,
    /// The output format.
    pub output: OutputFormat,
    /// Suppress progress output.
    pub quiet: bool,
}

impl RunInputs {
    /// The inputs of a run of `command` on `surface`, with every optional
    /// input unset, human output and no progress output.
    #[must_use]
    pub fn new(
        surface: Surface,
        env: EnvSnapshot,
        command: CommandKind,
        selection: SelectionInput,
    ) -> Self {
        Self {
            surface,
            env,
            command,
            selection,
            config_fallback: ConfigFallback::Fail,
            root: None,
            config_path: None,
            allow_remote_extends: None,
            no_cache: None,
            threads: None,
            max_file_size_mb: None,
            cache_dir: None,
            package_baselines: None,
            include_entry_exports: None,
            production: None,
            production_dead_code: None,
            production_health: None,
            production_dupes: None,
            analysis_flags: Vec::new(),
            type_aware: TypeAwareInputs::default(),
            audit_base: None,
            output: OutputFormat::Human,
            quiet: true,
        }
    }

    /// The surface that starts the run.
    #[must_use]
    pub const fn surface(&self) -> Surface {
        self.surface
    }

    /// The environment snapshot of the run.
    #[must_use]
    pub const fn env(&self) -> &EnvSnapshot {
        &self.env
    }
}

/// The inputs of a host that starts runs without a command, such as a
/// session store of the language server.
#[derive(Debug, Clone)]
pub struct HostInputs {
    /// The surface of the host.
    pub surface: Surface,
    /// The environment snapshot of the host.
    pub env: EnvSnapshot,
}
