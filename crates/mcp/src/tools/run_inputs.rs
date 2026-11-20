//! The API call of each typed tool.
//!
//! [`typed_call`] is the one place that builds the shared analysis options of
//! a typed tool: the environment scope of the tool, the production parameter,
//! the MCP surface and the environment snapshot of the call. The resolver of
//! `fallow-api` then decides the production mode of each analysis from the
//! tool parameters, the `FALLOW_PRODUCTION*` variables that the MCP surface
//! reads for the command, and the config. In test builds, `TypedTool::entry`
//! names the programmatic entry of each tool for the parity rows. Each
//! runner names its own entry, so the integration tests check the runners.

use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use fallow_api::AnalysisOptions;
#[cfg(test)]
use fallow_api::run::ProgrammaticEntry;
use fallow_api::run::{EnvSnapshot, HostInputs, Surface};

use super::api_runtime::{EnvScope, SharedParams, shared_analysis_options};

/// A tool that runs in process through `fallow-api`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum TypedTool {
    Analyze,
    CheckChanged,
    CheckArchitecture,
    FindDupes,
    CheckHealth,
    Audit,
    DecisionSurface,
    ProjectInfo,
    ListBoundaries,
    FeatureFlags,
    TraceExport,
    TraceFile,
    TraceImportPath,
    TraceError,
    TraceDependency,
    TraceClone,
    MutationPlan,
    /// The `combined` host call of Code Mode.
    Combined,
}

impl TypedTool {
    /// Every typed tool.
    #[cfg(test)]
    const ALL: [Self; 18] = [
        Self::Analyze,
        Self::CheckChanged,
        Self::CheckArchitecture,
        Self::FindDupes,
        Self::CheckHealth,
        Self::Audit,
        Self::DecisionSurface,
        Self::ProjectInfo,
        Self::ListBoundaries,
        Self::FeatureFlags,
        Self::TraceExport,
        Self::TraceFile,
        Self::TraceImportPath,
        Self::TraceError,
        Self::TraceDependency,
        Self::TraceClone,
        Self::MutationPlan,
        Self::Combined,
    ];

    /// The name of the tool on the MCP wire. Code Mode names the combined
    /// host call `combined`.
    #[cfg(test)]
    pub(super) const fn name(self) -> &'static str {
        match self {
            Self::Analyze => "analyze",
            Self::CheckChanged => "check_changed",
            Self::CheckArchitecture => "check_architecture",
            Self::FindDupes => "find_dupes",
            Self::CheckHealth => "check_health",
            Self::Audit => "audit",
            Self::DecisionSurface => "decision_surface",
            Self::ProjectInfo => "project_info",
            Self::ListBoundaries => "list_boundaries",
            Self::FeatureFlags => "feature_flags",
            Self::TraceExport => "trace_export",
            Self::TraceFile => "trace_file",
            Self::TraceImportPath => "trace_import_path",
            Self::TraceError => "trace_error",
            Self::TraceDependency => "trace_dependency",
            Self::TraceClone => "trace_clone",
            Self::MutationPlan => "mutation_plan",
            Self::Combined => "combined",
        }
    }

    /// The tool with the wire name `name`.
    #[cfg(test)]
    fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|tool| tool.name() == name)
    }

    /// The entry that the `fallow-api` runner of the tool uses, with the
    /// production parameters of an audit or a combined call. The parity rows
    /// resolve the run of a call with this entry. Each runner names its own
    /// entry, so this table does not decide the production mode of a tool
    /// call. The `typed_route_env_production*` integration tests run each tool
    /// that reads the `FALLOW_PRODUCTION*` variables through its runner and
    /// compare it with the CLI.
    #[cfg(test)]
    pub(super) fn entry(self, production: ToolProduction) -> ProgrammaticEntry {
        match self {
            Self::Analyze | Self::CheckChanged | Self::CheckArchitecture => {
                ProgrammaticEntry::DeadCode
            }
            Self::FindDupes | Self::TraceClone => ProgrammaticEntry::Duplication,
            Self::CheckHealth => ProgrammaticEntry::Health,
            Self::DecisionSurface => ProgrammaticEntry::DecisionSurface,
            Self::ProjectInfo | Self::ListBoundaries => ProgrammaticEntry::List,
            Self::FeatureFlags => ProgrammaticEntry::FeatureFlags,
            Self::TraceExport | Self::TraceFile => ProgrammaticEntry::TraceChain,
            Self::TraceImportPath => ProgrammaticEntry::TracePath,
            Self::TraceError => ProgrammaticEntry::TraceError,
            Self::TraceDependency => ProgrammaticEntry::TraceDependency,
            Self::MutationPlan => ProgrammaticEntry::Mutation,
            // The typed route of `audit` runs only without `only` and
            // `skip`, so the config decides the selection.
            Self::Audit => {
                let audit = production.audit();
                ProgrammaticEntry::Audit {
                    dead_code: None,
                    duplication: None,
                    health: None,
                    production: audit.production,
                    production_dead_code: audit.dead_code,
                    production_health: audit.health,
                    production_dupes: audit.dupes,
                }
            }
            Self::Combined => ProgrammaticEntry::Combined {
                dead_code: true,
                duplication: true,
                health: true,
                production_dead_code: None,
                production_health: None,
                production_dupes: None,
            },
        }
    }

    /// Where the tool puts the inherited `FALLOW_CHANGED_SINCE` and
    /// `FALLOW_DIFF_FILE`.
    const fn env_scope(self) -> EnvScope {
        match self {
            Self::Analyze
            | Self::CheckChanged
            | Self::CheckArchitecture
            | Self::FindDupes
            | Self::CheckHealth
            | Self::FeatureFlags
            | Self::Combined => EnvScope::Ambient,
            Self::Audit | Self::DecisionSurface => EnvScope::AuditBase,
            // These envelopes carry no `request_outcomes`, so a ref that
            // stood down would widen the result with nothing to say so: keep
            // the hard error of an explicit ref.
            Self::ProjectInfo
            | Self::ListBoundaries
            | Self::TraceExport
            | Self::TraceFile
            | Self::TraceImportPath
            | Self::TraceError
            | Self::TraceDependency
            | Self::TraceClone => EnvScope::HardRef,
            // The plan scope comes from the own `changed_since`, `paths` and
            // `all` parameters of the tool.
            Self::MutationPlan => EnvScope::Own,
        }
    }
}

/// The production parameters of one tool call.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct ToolProduction {
    /// The `production` parameter.
    pub production: Option<bool>,
    /// The `production_dead_code` parameter of `audit`.
    pub dead_code: Option<bool>,
    /// The `production_health` parameter of `audit`.
    pub health: Option<bool>,
    /// The `production_dupes` parameter of `audit`.
    pub dupes: Option<bool>,
}

impl ToolProduction {
    /// The parameters of a tool that has only the `production` parameter.
    pub(super) const fn global(production: Option<bool>) -> Self {
        Self {
            production,
            dead_code: None,
            health: None,
            dupes: None,
        }
    }

    /// The production fields of `AuditOptions`. Like the CLI flag, the
    /// parameter of one analysis can only turn production mode on, so `false`
    /// counts as not set.
    pub(super) fn audit(self) -> AuditProduction {
        let on = |value: Option<bool>| value.filter(|&on| on);
        AuditProduction {
            production: self.production.unwrap_or(false),
            dead_code: on(self.dead_code),
            health: on(self.health),
            dupes: on(self.dupes),
        }
    }
}

/// The production fields of `AuditOptions` for one `audit` call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct AuditProduction {
    pub production: bool,
    pub dead_code: Option<bool>,
    pub health: Option<bool>,
    pub dupes: Option<bool>,
}

/// The API call of one typed tool.
#[derive(Debug)]
pub(super) struct TypedCall {
    /// The runner of `fallow-api` that the tool calls.
    #[cfg(test)]
    pub entry: ProgrammaticEntry,
    /// The analysis options of the call. A tool sets its own fields on top
    /// with struct update syntax.
    pub analysis: AnalysisOptions,
}

/// The API call of `tool`: the shared parameters, the `production`
/// parameter, the environment scope of the tool, the cancellation token, and
/// the MCP surface with `env`. A handler passes `EnvSnapshot::capture()` once
/// per call.
pub(super) fn typed_call(
    tool: TypedTool,
    shared: SharedParams<'_>,
    production: ToolProduction,
    env: EnvSnapshot,
    cancellation: Option<Arc<AtomicBool>>,
) -> TypedCall {
    let shared = shared_analysis_options(shared, tool.env_scope(), &env, cancellation);
    let analysis = AnalysisOptions {
        production: production.production.unwrap_or(false),
        production_override: production.production,
        host: Some(HostInputs {
            surface: Surface::Mcp,
            env,
        }),
        ..shared
    };
    TypedCall {
        #[cfg(test)]
        entry: tool.entry(production),
        analysis,
    }
}

#[cfg(test)]
#[path = "run_parity_tests.rs"]
mod parity_tests;
