//! One resolved run for each programmatic call.
//!
//! Each typed runner that loads a config calls [`resolve_programmatic_run`]
//! once with its own [`ProgrammaticEntry`]. The resolver loads and validates
//! the config, decides the selection and the production mode of each
//! analysis, and resolves one config for each production mode. The runner
//! opens each session from that run, so the analyses of one call share one
//! config load.

use std::path::PathBuf;

pub use fallow_config::{CommandKind, EnvSnapshot, EnvVar, HostInputs, SelectionInput, Surface};
pub use fallow_engine::resolved_run::{
    AnalysisSlot, ResolveError, ResolvedRun, Runnable, resolve_run,
};
#[cfg(feature = "testing")]
pub use fallow_engine::resolved_run::{parity, testing};

/// The head run of an audit with `options`, resolved through the steps of
/// the audit runner. The base ref is `HEAD`, because the run does not depend
/// on it. A parity row of a surface that calls the audit runner uses it.
///
/// # Errors
///
/// Returns the errors of the audit runner.
#[cfg(feature = "testing")]
pub fn resolve_audit_head_run(options: &crate::AuditOptions) -> ProgrammaticResult<ResolvedRun> {
    let folded = crate::runtime::analysis_options_for_audit(options, "HEAD");
    crate::runtime::resolve_audit_run(options, &folded)
}

use fallow_config::{PlannedAnalysis, RunInputs};
use fallow_engine::resolved_run::{InputField, InputProblem};
use fallow_types::output_format::OutputFormat;
use fallow_types::sourced::{Given, Origin};

use crate::{AnalysisOptions, ProgrammaticError};

type ProgrammaticResult<T> = Result<T, ProgrammaticError>;

/// The typed runner that starts a run. Each entry maps to one
/// [`CommandKind`] and one [`SelectionInput`].
#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum ProgrammaticEntry {
    /// The dead-code runners: dead code, circular dependencies, boundary
    /// violations and architecture.
    DeadCode,
    /// The duplication runner.
    Duplication,
    /// The health runner.
    Health,
    /// The feature flags runner.
    FeatureFlags,
    /// The project info and the boundary list runners.
    List,
    /// The import path trace.
    TracePath,
    /// The export, file and clone traces.
    TraceChain,
    /// The error trace.
    TraceError,
    /// The dependency trace.
    TraceDependency,
    /// The decision surface.
    DecisionSurface,
    /// The mutation runners.
    Mutation,
    /// The similar-code runners.
    SimilarCode,
    /// The doctor.
    Doctor,
    /// The combined runner.
    Combined {
        /// Run the dead-code analysis.
        dead_code: bool,
        /// Run the duplication analysis.
        duplication: bool,
        /// Run the health analysis.
        health: bool,
        /// The production mode of the dead-code analysis.
        production_dead_code: Option<bool>,
        /// The production mode of the health analysis.
        production_health: Option<bool>,
        /// The production mode of the duplication analysis.
        production_dupes: Option<bool>,
    },
    /// The audit runner.
    Audit {
        /// The request for the dead-code analysis.
        dead_code: Option<bool>,
        /// The request for the duplication analysis.
        duplication: Option<bool>,
        /// The request for the health analysis.
        health: Option<bool>,
        /// `AuditOptions::production`: production mode for every analysis
        /// when `analysis.production_override` is `None`.
        production: bool,
        /// The production mode of the dead-code analysis.
        production_dead_code: Option<bool>,
        /// The production mode of the health analysis.
        production_health: Option<bool>,
        /// The production mode of the duplication analysis.
        production_dupes: Option<bool>,
    },
}

impl ProgrammaticEntry {
    /// The command of the entry.
    #[must_use]
    pub const fn command(&self) -> CommandKind {
        match self {
            Self::DeadCode => CommandKind::DeadCode,
            Self::Duplication => CommandKind::Dupes,
            Self::Health => CommandKind::Health,
            Self::FeatureFlags => CommandKind::Flags,
            Self::List => CommandKind::List,
            Self::TracePath => CommandKind::TracePath,
            Self::TraceChain => CommandKind::TraceChain,
            Self::TraceError => CommandKind::TraceError,
            Self::TraceDependency => CommandKind::TraceDependency,
            Self::DecisionSurface => CommandKind::DecisionSurface,
            Self::Mutation => CommandKind::Mutation,
            Self::SimilarCode => CommandKind::SimilarCode,
            Self::Doctor => CommandKind::Doctor,
            Self::Combined { .. } => CommandKind::Combined,
            Self::Audit { .. } => CommandKind::Audit,
        }
    }

    /// How the entry selects its analyses.
    #[must_use]
    pub const fn selection(&self) -> SelectionInput {
        match self {
            Self::Duplication => SelectionInput::Single(PlannedAnalysis::Duplication),
            Self::Health => SelectionInput::Single(PlannedAnalysis::Health),
            Self::Combined {
                dead_code,
                duplication,
                health,
                ..
            } => SelectionInput::Booleans {
                dead_code: *dead_code,
                duplication: *duplication,
                health: *health,
                origin: Origin::ApiOption("combined"),
            },
            Self::Audit {
                dead_code,
                duplication,
                health,
                ..
            } => SelectionInput::Options {
                dead_code: *dead_code,
                duplication: *duplication,
                health: *health,
                origin: Origin::ApiOption("audit"),
            },
            Self::DeadCode
            | Self::FeatureFlags
            | Self::List
            | Self::TracePath
            | Self::TraceChain
            | Self::TraceError
            | Self::TraceDependency
            | Self::DecisionSurface
            | Self::Mutation
            | Self::SimilarCode
            | Self::Doctor => SelectionInput::Single(PlannedAnalysis::DeadCode),
        }
    }

    /// Whether the runner of the entry reads the production options of
    /// [`AnalysisOptions`]. The mutation, similar-code and doctor runners
    /// take the production mode from the config only.
    const fn reads_production(&self) -> bool {
        !matches!(self, Self::Mutation | Self::SimilarCode | Self::Doctor)
    }
}

/// Settings of one runner that are not in [`AnalysisOptions`].
#[derive(Debug, Clone, Default)]
pub(crate) struct RouteSettings {
    /// The `include_entry_exports` option of the runner.
    pub include_entry_exports: bool,
    /// The base ref that the caller of an audit names.
    pub audit_base: Option<NamedAuditBase>,
}

/// The base ref that the caller of an audit names, by the option that names
/// it.
#[derive(Debug, Clone)]
pub(crate) enum NamedAuditBase {
    /// The `base` option of the audit.
    Base(String),
    /// The `changed_since` option of the analysis.
    ChangedSince(String),
}

impl NamedAuditBase {
    /// The `base` option of `options`, else its `analysis.changed_since`.
    pub(crate) fn of(options: &crate::AuditOptions) -> Option<Self> {
        options.base.clone().map(Self::Base).or_else(|| {
            options
                .analysis
                .changed_since
                .clone()
                .map(Self::ChangedSince)
        })
    }
}

/// Resolve the run of one programmatic call.
///
/// `options.host` names the surface and the environment snapshot. `None`
/// means the Rust API with a snapshot of the process environment. The surface
/// names the origin of each given value: an option of the Rust API, an option
/// of the Node bindings, or an MCP tool parameter.
///
/// # Errors
///
/// Returns the structured error of [`ProgrammaticError`] with the same code
/// and context as the runners: `FALLOW_INVALID_ROOT`,
/// `FALLOW_INVALID_CONFIG_PATH`, `FALLOW_INVALID_THREADS`,
/// `FALLOW_CONFIG_LOAD_FAILED`, `FALLOW_COMBINED_EMPTY` or
/// `FALLOW_AUDIT_NO_ANALYSIS`.
pub fn resolve_programmatic_run(
    options: &AnalysisOptions,
    entry: &ProgrammaticEntry,
) -> ProgrammaticResult<ResolvedRun> {
    resolve_route_run(options, entry, RouteSettings::default())
}

/// [`resolve_programmatic_run`] with the settings of one runner.
pub(crate) fn resolve_route_run(
    options: &AnalysisOptions,
    entry: &ProgrammaticEntry,
    route: RouteSettings,
) -> ProgrammaticResult<ResolvedRun> {
    resolve_route_run_raw(options, entry, route).map_err(|err| programmatic_error(&err))
}

/// [`resolve_route_run`] with the error of the resolver, for a runner that
/// maps some errors to its own code.
pub(crate) fn resolve_route_run_raw(
    options: &AnalysisOptions,
    entry: &ProgrammaticEntry,
    route: RouteSettings,
) -> Result<ResolvedRun, ResolveError> {
    fallow_engine::resolved_run::resolve_run(&run_inputs(options, entry, route))
}

/// The names of one option on the Rust API, on the Node bindings and as an
/// MCP tool parameter.
struct OptionName {
    api: &'static str,
    node: &'static str,
    mcp: &'static str,
}

const ROOT: OptionName = OptionName {
    api: "analysis.root",
    node: "root",
    mcp: "root",
};
const CONFIG_PATH: OptionName = OptionName {
    api: "analysis.config_path",
    node: "configPath",
    mcp: "config",
};
const ALLOW_REMOTE_EXTENDS: OptionName = OptionName {
    api: "analysis.allow_remote_extends",
    node: "allowRemoteExtends",
    mcp: "allow_remote_extends",
};
const NO_CACHE: OptionName = OptionName {
    api: "analysis.no_cache",
    node: "noCache",
    mcp: "no_cache",
};
const THREADS: OptionName = OptionName {
    api: "analysis.threads",
    node: "threads",
    mcp: "threads",
};
const NO_PACKAGE_BASELINES: OptionName = OptionName {
    api: "analysis.no_package_baselines",
    node: "noPackageBaselines",
    mcp: "no_package_baselines",
};
const PRODUCTION_OVERRIDE: OptionName = OptionName {
    api: "analysis.production_override",
    node: "production",
    mcp: "production",
};
const PRODUCTION: OptionName = OptionName {
    api: "analysis.production",
    node: "production",
    mcp: "production",
};
const AUDIT_PRODUCTION: OptionName = OptionName {
    api: "audit.production",
    node: "production",
    mcp: "production",
};
const INCLUDE_ENTRY_EXPORTS: OptionName = OptionName {
    api: "include_entry_exports",
    node: "includeEntryExports",
    mcp: "include_entry_exports",
};
const PRODUCTION_DEAD_CODE: OptionName = OptionName {
    api: "production_dead_code",
    node: "productionDeadCode",
    mcp: "production_dead_code",
};
const PRODUCTION_HEALTH: OptionName = OptionName {
    api: "production_health",
    node: "productionHealth",
    mcp: "production_health",
};
const PRODUCTION_DUPES: OptionName = OptionName {
    api: "production_dupes",
    node: "productionDupes",
    mcp: "production_dupes",
};
const AUDIT_BASE: OptionName = OptionName {
    api: "audit.base",
    node: "base",
    mcp: "base",
};
const CHANGED_SINCE: OptionName = OptionName {
    api: "analysis.changed_since",
    node: "changedSince",
    mcp: "changed_since",
};
const TYPE_AWARE: OptionName = OptionName {
    api: "analysis.type_aware.enabled",
    node: "typeAware.enabled",
    mcp: "type_aware",
};

/// The environment snapshot of one programmatic call: the snapshot of
/// `options.host`, else a snapshot of the process environment.
pub(crate) fn host_env(options: &AnalysisOptions) -> EnvSnapshot {
    options
        .host
        .as_ref()
        .map_or_else(EnvSnapshot::capture, |host| host.env.clone())
}

/// The raw run inputs of one programmatic call.
pub(crate) fn run_inputs(
    options: &AnalysisOptions,
    entry: &ProgrammaticEntry,
    route: RouteSettings,
) -> RunInputs {
    let HostInputs { surface, env } = options.host.clone().unwrap_or_else(|| HostInputs {
        surface: Surface::Api,
        env: EnvSnapshot::capture(),
    });
    let origin = |name: OptionName| match surface {
        Surface::Node => Origin::NodeOption(name.node),
        Surface::Mcp => Origin::ToolParam(name.mcp),
        Surface::Api | Surface::Cli | Surface::Lsp => Origin::ApiOption(name.api),
    };
    let given = |value, name| Given {
        value,
        origin: origin(name),
    };
    let when_true = |value: bool, name| value.then(|| given(true, name));
    let mut inputs = RunInputs::new(surface, env, entry.command(), entry.selection());
    inputs.root = options.root.clone().map(|root| Given {
        value: root,
        origin: origin(ROOT),
    });
    inputs.config_path = options.config_path.clone().map(|path: PathBuf| Given {
        value: path,
        origin: origin(CONFIG_PATH),
    });
    inputs.allow_remote_extends = when_true(options.allow_remote_extends, ALLOW_REMOTE_EXTENDS);
    inputs.no_cache = when_true(options.no_cache, NO_CACHE);
    inputs.threads = options.threads.map(|threads| Given {
        value: threads,
        origin: origin(THREADS),
    });
    inputs.package_baselines = options
        .no_package_baselines
        .then(|| given(false, NO_PACKAGE_BASELINES));
    inputs.include_entry_exports = when_true(route.include_entry_exports, INCLUDE_ENTRY_EXPORTS);
    inputs.audit_base = route.audit_base.map(|named| {
        let (value, name) = match named {
            NamedAuditBase::Base(value) => (value, AUDIT_BASE),
            NamedAuditBase::ChangedSince(value) => (value, CHANGED_SINCE),
        };
        Given {
            value,
            origin: origin(name),
        }
    });
    inputs.type_aware.enabled = options
        .type_aware
        .enabled
        .map(|enabled| given(enabled, TYPE_AWARE));
    inputs.output = OutputFormat::Json;
    inputs.quiet = true;
    if entry.reads_production() {
        set_production(&mut inputs, options, entry, given);
    }
    inputs
}

fn set_production(
    inputs: &mut RunInputs,
    options: &AnalysisOptions,
    entry: &ProgrammaticEntry,
    given: impl Fn(bool, OptionName) -> Given<bool>,
) {
    let global_option = |legacy: bool, name| {
        options
            .production_override
            .map(|value| given(value, PRODUCTION_OVERRIDE))
            .or_else(|| legacy.then(|| given(true, name)))
    };
    let own = |value: Option<bool>, name| value.map(|value| given(value, name));
    let (global, per_analysis) = match entry {
        ProgrammaticEntry::Audit {
            production,
            production_dead_code,
            production_health,
            production_dupes,
            ..
        } => (
            global_option(*production, AUDIT_PRODUCTION),
            Some((*production_dead_code, *production_health, *production_dupes)),
        ),
        ProgrammaticEntry::Combined {
            production_dead_code,
            production_health,
            production_dupes,
            ..
        } => (
            global_option(options.production, PRODUCTION),
            Some((*production_dead_code, *production_health, *production_dupes)),
        ),
        _ => (global_option(options.production, PRODUCTION), None),
    };
    inputs.production = global;
    if let Some((dead_code, health, dupes)) = per_analysis {
        inputs.production_dead_code = own(dead_code, PRODUCTION_DEAD_CODE);
        inputs.production_health = own(health, PRODUCTION_HEALTH);
        inputs.production_dupes = own(dupes, PRODUCTION_DUPES);
    }
}

const CONFIG_LOAD_FAILED: &str = "FALLOW_CONFIG_LOAD_FAILED";
const CONFIG_PATH_CONTEXT: &str = "analysis.configPath";

/// The message of `FALLOW_COMBINED_EMPTY`.
pub(crate) const COMBINED_EMPTY_MESSAGE: &str =
    "combined analysis requires at least one enabled section";

/// The message of `FALLOW_AUDIT_NO_ANALYSIS`.
pub(crate) const AUDIT_NO_ANALYSIS_MESSAGE: &str = "this audit selects no domain: the options, the audit.deadCode, audit.health and audit.dupes config keys, and duplicates.enabled leave nothing to run";

/// The structured error of a resolver error, with the code and the context
/// that the typed runners return.
pub(crate) fn programmatic_error(err: &ResolveError) -> ProgrammaticError {
    let config_error = |message: String| {
        ProgrammaticError::new(format!("failed to load config: {message}"), 2)
            .with_code(CONFIG_LOAD_FAILED)
            .with_context(CONFIG_PATH_CONTEXT)
    };
    match err {
        ResolveError::Input {
            field,
            problem,
            message,
        } => {
            let (code, context) = match (field, problem) {
                (InputField::Root, InputProblem::CwdUnavailable) => {
                    ("FALLOW_CWD_UNAVAILABLE", "analysis.root")
                }
                (InputField::Root, _) => ("FALLOW_INVALID_ROOT", "analysis.root"),
                (InputField::ConfigPath, _) => ("FALLOW_INVALID_CONFIG_PATH", CONFIG_PATH_CONTEXT),
                (InputField::Threads, _) => ("FALLOW_INVALID_THREADS", "analysis.threads"),
                // The typed routes refuse a type-aware layer with this code, so
                // a caller that can run the CLI falls back on it.
                (InputField::Env(EnvVar::TypeAware), _) => (
                    crate::TYPE_AWARE_CONFIG_UNSUPPORTED,
                    EnvVar::TypeAware.name(),
                ),
                (InputField::Env(var), _) => ("FALLOW_INVALID_ENV", var.name()),
            };
            ProgrammaticError::new(message.clone(), 2)
                .with_code(code)
                .with_context(context)
        }
        ResolveError::ConfigLoad(load) => config_error(load.chain().to_owned()),
        ResolveError::NothingSelected { command, .. } => match command {
            CommandKind::Audit => ProgrammaticError::new(AUDIT_NO_ANALYSIS_MESSAGE, 2)
                .with_code("FALLOW_AUDIT_NO_ANALYSIS")
                .with_context("audit"),
            _ => ProgrammaticError::new(COMBINED_EMPTY_MESSAGE, 2)
                .with_code("FALLOW_COMBINED_EMPTY")
                .with_context("combined"),
        },
        _ => config_error(err.message()),
    }
}

/// The runnable of `analysis` in `run`.
///
/// # Errors
///
/// Returns `FALLOW_INTERNAL_ERROR` when the run skips the analysis: each
/// runner resolves a run that selects the analyses that it opens.
pub(crate) fn runnable(
    run: &ResolvedRun,
    analysis: PlannedAnalysis,
) -> ProgrammaticResult<Runnable<'_>> {
    match run.analysis(analysis) {
        AnalysisSlot::Runs(runnable) => Ok(runnable),
        AnalysisSlot::Skipped { .. } => Err(ProgrammaticError::new(
            format!("the resolved run skips the {analysis:?} analysis"),
            2,
        )
        .with_code("FALLOW_INTERNAL_ERROR")),
    }
}

/// A session of `runnable` with the rule settings of one runner.
///
/// # Errors
///
/// Returns `FALLOW_INTERNAL_ERROR` when `adjust` changes a decided field of
/// the run: the runners write rule settings only.
pub(crate) fn open_session_adjusted(
    runnable: &Runnable<'_>,
    adjust: impl FnOnce(&mut fallow_config::ResolvedConfig),
) -> ProgrammaticResult<fallow_engine::session::AnalysisSession> {
    runnable.open_session_adjusted(adjust).map_err(|err| {
        ProgrammaticError::new(err.to_string(), 2).with_code("FALLOW_INTERNAL_ERROR")
    })
}

#[cfg(test)]
#[path = "run_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "run_parity_tests.rs"]
mod parity_tests;
