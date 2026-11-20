//! The decisions of one run, each with the input that gave it.
//!
//! [`resolve_run`] takes the raw
//! [`fallow_config::RunInputs`] of one surface, loads and validates the
//! config, and decides the selection and the production mode of each analysis
//! and each run setting. The result is one [`ResolvedRun`] with a resolved
//! config per production mode.

mod audit_base;
mod env_policy;
mod notices;
#[cfg(any(test, feature = "testing"))]
pub mod parity;
mod pick;
mod resolve;
pub mod steps;
#[cfg(any(test, feature = "testing"))]
pub mod testing;
mod type_aware;
mod view;

pub use audit_base::{AuditBaseDecision, decide_audit_base};
pub use resolve::{
    BaseConfig, BaseSide, EmptyCause, InputField, InputProblem, ResolveError, resolve_base_run,
    resolve_base_run_with_head_decisions, resolve_run,
};
pub use type_aware::{TypeAwareConfigInput, TypeAwareSettings, decide_type_aware};

use std::path::{Path, PathBuf};

use fallow_config::{
    AnalysisSelection, CommandKind, ConfigKey, EnvSnapshot, EnvVar, FallowConfig, PlannedAnalysis,
    ResolvedConfig, SkipReason, Surface, WorkspaceInfo,
};
use fallow_types::output_format::OutputFormat;
use fallow_types::sourced::Sourced;

pub use pick::{PerAnalysis, ProductionInputs, decide_production};

use crate::project_config::{
    ConfigChainSnapshot, DuplicationScope, ProductionModes, ResolveOptions,
};
use crate::session::AnalysisSession;
use crate::{EngineError, EngineResult};
use steps::{GivenSettings, ModeEntry};

/// Every decision of one run, with its cause. The fields are private and the
/// type has no `Default`, no `Clone` and no `&mut self` method, so a run
/// stays as the resolver made it.
#[derive(Debug)]
pub struct ResolvedRun {
    surface: Surface,
    command: CommandKind,
    root: Sourced<PathBuf>,
    config_file: Sourced<Option<PathBuf>>,
    /// The bytes of the config file and its local `extends` chain at the load.
    config_snapshot: ConfigChainSnapshot,
    /// The validated config, before the production mode is flattened.
    file: FallowConfig,
    dead_code: AnalysisDecision,
    duplication: AnalysisDecision,
    health: AnalysisDecision,
    /// The mode entry of the analyses that run without production mode.
    off: Option<ModeEntry>,
    /// The mode entry of the analyses that run in production mode.
    on: Option<ModeEntry>,
    settings: RunSettings,
    /// The type-aware settings, or the variable whose value the run cannot
    /// use.
    type_aware: type_aware::TypeAwareDecision,
    /// The settings that the surface gives and that win over the environment.
    given: GivenSettings,
    /// The environment snapshot of the run, without the variables that the
    /// run does not read.
    env: EnvSnapshot,
    env_read: Vec<EnvVar>,
    env_ignored: Vec<EnvVar>,
    /// The config error that made the run use the default config.
    fallback: Option<ResolveError>,
    /// The notices of the run, as diagnostics at the root.
    notices: Vec<fallow_config::WorkspaceDiagnostic>,
    /// The raw inputs of the run, for the base side of a comparison.
    inputs: Box<fallow_config::RunInputs>,
}

/// The decisions of one analysis.
#[derive(Debug)]
struct AnalysisDecision {
    selection: Sourced<AnalysisSelection>,
    production: Sourced<bool>,
}

/// The settings of one run. Every mode entry of the run has these values.
#[derive(Debug)]
pub struct RunSettings {
    /// The number of worker threads.
    pub threads: Sourced<usize>,
    /// Whether the run bypasses the parse cache.
    pub no_cache: Sourced<bool>,
    /// Whether the `extends` chain of the config may name `https://` entries.
    pub allow_remote_extends: Sourced<bool>,
    /// The largest file to analyze, in bytes. `None` means no limit.
    pub max_file_size_bytes: Sourced<Option<u64>>,
    /// The cache directory.
    pub cache_dir: Sourced<PathBuf>,
    /// The size limit of the parse cache, in megabytes.
    pub cache_max_size_mb: Sourced<Option<u32>>,
    /// Whether the run reads `workspaces.changedSince`.
    pub package_baselines: Sourced<bool>,
    /// Whether dead code reports the unused exports of entry files.
    pub include_entry_exports: Sourced<bool>,
    /// The output format.
    pub output: OutputFormat,
    /// Whether the run suppresses progress output.
    pub quiet: bool,
}

impl RunSettings {
    const fn resolve_options(&self) -> ResolveOptions {
        ResolveOptions {
            output: self.output,
            threads: *self.threads.value(),
            no_cache: *self.no_cache.value(),
            quiet: self.quiet,
        }
    }
}

/// One analysis of a run: it runs, or the run skips it with a reason.
pub enum AnalysisSlot<'run> {
    /// The analysis runs.
    Runs(Runnable<'run>),
    /// The run skips the analysis.
    Skipped {
        /// Why the run skips the analysis.
        reason: &'run Sourced<AnalysisSelection>,
    },
}

/// An analysis that runs. Only [`ResolvedRun::analysis`] makes one, so a
/// skipped analysis has no config and no session.
#[derive(Clone, Copy)]
pub struct Runnable<'run> {
    run: &'run ResolvedRun,
    analysis: PlannedAnalysis,
}

/// The config key of a skip reason, as the config file spells it.
const fn config_key_name(key: ConfigKey) -> &'static str {
    match key {
        ConfigKey::AuditDeadCode => "audit.deadCode",
        ConfigKey::AuditHealth => "audit.health",
        ConfigKey::AuditDupes => "audit.dupes",
        ConfigKey::DuplicatesEnabled => "duplicates.enabled",
    }
}

/// The config key that skips an analysis, if a config key skips it.
fn skip_key(selection: AnalysisSelection) -> Option<ConfigKey> {
    match selection {
        AnalysisSelection::Skipped(SkipReason::Config(key)) => Some(key),
        AnalysisSelection::Run | AnalysisSelection::Skipped(SkipReason::NotRequested) => None,
    }
}

impl ResolvedRun {
    /// The surface that started the run.
    pub const fn surface(&self) -> Surface {
        self.surface
    }

    /// The command of the run.
    pub const fn command(&self) -> CommandKind {
        self.command
    }

    /// The project root.
    pub const fn root(&self) -> &Sourced<PathBuf> {
        &self.root
    }

    /// The config file. `None` when the run uses the default config.
    pub const fn config_file(&self) -> &Sourced<Option<PathBuf>> {
        &self.config_file
    }

    /// The validated config, before the production mode is flattened.
    pub const fn config(&self) -> &FallowConfig {
        &self.file
    }

    const fn decision(&self, analysis: PlannedAnalysis) -> &AnalysisDecision {
        match analysis {
            PlannedAnalysis::DeadCode => &self.dead_code,
            PlannedAnalysis::Duplication => &self.duplication,
            PlannedAnalysis::Health => &self.health,
        }
    }

    /// The analysis, when it runs, or the reason why the run skips it.
    pub fn analysis(&self, analysis: PlannedAnalysis) -> AnalysisSlot<'_> {
        let decision = self.decision(analysis);
        if decision.selection.value().runs() {
            AnalysisSlot::Runs(Runnable {
                run: self,
                analysis,
            })
        } else {
            AnalysisSlot::Skipped {
                reason: &decision.selection,
            }
        }
    }

    /// The production mode of the analysis.
    pub const fn production(&self, analysis: PlannedAnalysis) -> &Sourced<bool> {
        &self.decision(analysis).production
    }

    /// The production mode of each analysis.
    pub fn production_modes(&self) -> ProductionModes {
        ProductionModes {
            dead_code: *self.dead_code.production.value(),
            health: *self.health.production.value(),
            dupes: *self.duplication.production.value(),
        }
    }

    /// The settings of the run.
    pub const fn settings(&self) -> &RunSettings {
        &self.settings
    }

    /// The type-aware settings of the run.
    ///
    /// # Errors
    ///
    /// Returns [`ResolveError::Input`] with [`InputField::Env`] when
    /// `FALLOW_TYPE_AWARE` or `FALLOW_TYPE_AWARE_REQUIRE` has a value that the
    /// run cannot use. The run itself resolves, so a command that reads no
    /// type-aware setting does not fail.
    pub fn type_aware(&self) -> Result<&TypeAwareSettings, ResolveError> {
        self.type_aware.result()
    }

    /// The base ref candidates of an audit run. A run of another command
    /// has none.
    #[must_use]
    fn audit_base(&self) -> AuditBaseDecision {
        decide_audit_base(&self.inputs)
    }

    /// The environment snapshot of the run. It holds only the variables that
    /// the run reads on its surface and command.
    pub const fn env(&self) -> &EnvSnapshot {
        &self.env
    }

    /// The set variables that the run reads.
    #[cfg(test)]
    fn env_read(&self) -> &[EnvVar] {
        &self.env_read
    }

    /// The set variables that the run ignores on its surface and command.
    pub fn env_ignored(&self) -> &[EnvVar] {
        &self.env_ignored
    }

    /// Whether the run uses the default config because the discovered config
    /// failed to load, to validate or to give workspace metadata.
    #[cfg(test)]
    const fn fell_back(&self) -> bool {
        self.fallback.is_some()
    }

    /// The error of the discovered config when the run uses the default
    /// config because that config failed to load, to validate or to give
    /// workspace metadata.
    pub const fn fallback_error(&self) -> Option<&ResolveError> {
        self.fallback.as_ref()
    }

    /// The notices of the run: the decisions of the resolver that a surface
    /// shows as one note each, such as an environment variable whose value
    /// the run ignores. Each session of the run also carries them in its
    /// workspace diagnostics.
    pub fn notices(&self) -> &[fallow_config::WorkspaceDiagnostic] {
        &self.notices
    }

    /// Whether a config input changed since the load or during the load: the
    /// bytes of the config file or of a file of its local `extends` chain, or
    /// a plugin file, a rule pack or an `autoDiscover` directory that config
    /// resolution read. A long-lived caller then resolves the run again.
    pub fn sources_changed(&self) -> bool {
        self.config_snapshot.changed()
            || [self.off.as_ref(), self.on.as_ref()]
                .into_iter()
                .flatten()
                .any(|entry| entry.inputs.snapshot() != entry.inputs_before_resolve)
    }

    /// The resources that the `plugins` config field names and that do not
    /// resolve.
    pub fn plugin_readiness(&self) -> Vec<fallow_config::ConfiguredPluginDiagnostic> {
        fallow_config::diagnose_configured_external_plugins(self.root.value(), &self.file.plugins)
    }

    /// The mode entry of a production mode.
    fn entry(&self, production: bool) -> Option<&ModeEntry> {
        if production {
            self.on.as_ref()
        } else {
            self.off.as_ref()
        }
    }
}

impl<'run> Runnable<'run> {
    /// The analysis.
    pub const fn analysis(&self) -> PlannedAnalysis {
        self.analysis
    }

    /// The production mode of the analysis.
    pub const fn production(&self) -> &'run Sourced<bool> {
        self.run.production(self.analysis)
    }

    /// The run of the analysis.
    pub const fn run(&self) -> &'run ResolvedRun {
        self.run
    }

    fn entry(&self) -> &'run ModeEntry {
        let Some(entry) = self.run.entry(*self.production().value()) else {
            unreachable!("the resolver makes a mode entry for each analysis that runs");
        };
        entry
    }

    /// The resolved config of the production mode of the analysis.
    pub fn config(&self) -> &'run ResolvedConfig {
        &self.entry().config
    }

    /// The workspaces under the root.
    pub fn workspaces(&self) -> &'run [WorkspaceInfo] {
        &self.entry().workspaces
    }

    /// The diagnostics of the workspace discovery.
    pub fn workspace_diagnostics(&self) -> &'run [fallow_config::WorkspaceDiagnostic] {
        &self.entry().workspace_diagnostics
    }

    /// Whether the two analyses analyze the same files: both run in the same
    /// production mode.
    #[cfg(test)]
    fn shares_files_with(&self, other: Runnable<'_>) -> bool {
        self.production().value() == other.production().value()
    }

    /// A session of the analysis.
    pub fn open_session(&self) -> AnalysisSession {
        self.session_from(self.entry().config.clone())
    }

    /// A session of the analysis, with the rule settings of one command.
    ///
    /// # Errors
    ///
    /// Returns an error when `adjust` changes a decided field: the root, the
    /// production mode, the ignore or workspace patterns, the max file size,
    /// the cache directory or size, `no_cache`, the thread count or
    /// `include_entry_exports`.
    pub fn open_session_adjusted(
        &self,
        adjust: impl FnOnce(&mut ResolvedConfig),
    ) -> EngineResult<AnalysisSession> {
        let decided = &self.entry().config;
        let mut config = decided.clone();
        adjust(&mut config);
        if let Some(field) = changed_decided_field(decided, &config) {
            return Err(EngineError::new(format!(
                "a command adjustment changed the resolved field `{field}`"
            )));
        }
        Ok(self.session_from(config))
    }

    fn session_from(&self, config: ResolvedConfig) -> AnalysisSession {
        let entry = self.entry();
        let project = ModeEntry {
            config,
            inputs: entry.inputs.clone(),
            inputs_before_resolve: entry.inputs_before_resolve.clone(),
            workspaces: entry.workspaces.clone(),
            workspace_diagnostics: entry.workspace_diagnostics.clone(),
            workspace_discovery_ms: entry.workspace_discovery_ms,
        }
        .into_project_config(
            self.run.config_file.value().clone(),
            self.duplication_scope(),
        );
        AnalysisSession::from_project_config(project)
    }

    /// The duplication files of a session of this analysis: the session files
    /// when duplication runs in the same production mode, else the files of
    /// the config of the duplication mode.
    fn duplication_scope(&self) -> DuplicationScope {
        let dupes = *self.run.duplication.production.value();
        if dupes == *self.production().value() {
            return DuplicationScope::SameAsSession;
        }
        let mut config = self.run.file.clone();
        steps::flatten_production(&mut config, self.run.config_file.value().is_some(), dupes);
        DuplicationScope::Own {
            config: Box::new(config),
            options: self.run.settings.resolve_options(),
            env: Box::new(self.run.given.env_for_steps(&self.run.env)),
            given: Box::new(self.run.given.clone()),
        }
    }
}

/// The name of the first decided field that differs between the two configs.
fn changed_decided_field(
    decided: &ResolvedConfig,
    adjusted: &ResolvedConfig,
) -> Option<&'static str> {
    let checks: [(&'static str, bool); 12] = [
        ("root", decided.root == adjusted.root),
        ("production", decided.production == adjusted.production),
        (
            "production_ignore",
            decided.production_ignore == adjusted.production_ignore,
        ),
        (
            "ignore_patterns",
            decided
                .ignore_patterns
                .same_globs(&adjusted.ignore_patterns),
        ),
        (
            "workspace_patterns",
            decided.workspace_patterns == adjusted.workspace_patterns,
        ),
        (
            "max_file_size_bytes",
            decided.max_file_size_bytes == adjusted.max_file_size_bytes,
        ),
        ("cache_dir", decided.cache_dir == adjusted.cache_dir),
        (
            "cache_max_size_mb",
            decided.cache_max_size_mb == adjusted.cache_max_size_mb,
        ),
        ("no_cache", decided.no_cache == adjusted.no_cache),
        ("threads", decided.threads == adjusted.threads),
        (
            "include_entry_exports",
            decided.include_entry_exports == adjusted.include_entry_exports,
        ),
        (
            "workspace_changed_since",
            decided.workspace_changed_since == adjusted.workspace_changed_since,
        ),
    ];
    checks
        .into_iter()
        .find_map(|(field, same)| (!same).then_some(field))
}

/// The default thread count of a run: the available parallelism of the host,
/// else 1.
#[must_use]
pub fn available_threads() -> usize {
    threads_or_one(std::thread::available_parallelism())
}

/// The thread count of an `available_parallelism` result. One thread when the
/// host cannot tell, so a run never starts more workers than it can prove.
fn threads_or_one(parallelism: std::io::Result<std::num::NonZeroUsize>) -> usize {
    parallelism.map_or(1, std::num::NonZeroUsize::get)
}

/// The root of a path input: absolute paths stay, relative paths join `cwd`.
fn join_cwd(path: &Path, cwd: Option<&Path>) -> Option<PathBuf> {
    if path.is_absolute() {
        Some(path.to_path_buf())
    } else {
        cwd.map(|cwd| cwd.join(path))
    }
}

#[cfg(test)]
#[path = "resolve_tests.rs"]
mod resolve_tests;

#[cfg(test)]
#[path = "run_tests.rs"]
mod run_tests;

#[cfg(test)]
#[path = "base_tests.rs"]
mod base_tests;
