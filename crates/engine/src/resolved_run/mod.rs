//! The decisions of one run, each with the input that gave it.
//!
//! [`resolve_run`](resolve::resolve_run) takes the raw
//! [`fallow_config::RunInputs`] of one surface, loads and validates the
//! config, and decides the selection and the production mode of each analysis
//! and each run setting. The result is one [`ResolvedRun`] with a resolved
//! config per production mode.
#![cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "only the tests of this crate read the resolved run; the entry points of `project_config` call the steps"
    )
)]

mod env_policy;
#[cfg(any(test, feature = "testing"))]
mod parity;
mod pick;
mod resolve;
pub mod steps;
#[cfg(any(test, feature = "testing"))]
mod testing;
mod view;

use std::path::{Path, PathBuf};

use fallow_config::{
    AnalysisSelection, CommandKind, ConfigKey, EnvSnapshot, EnvVar, FallowConfig, PlannedAnalysis,
    ResolvedConfig, SkipReason, Surface, WorkspaceInfo,
};
use fallow_types::output_format::OutputFormat;
use fallow_types::sourced::Sourced;

pub use pick::{PerAnalysis, ProductionInputs, decide_production};

use crate::project_config::{DuplicationScope, ProductionModes, ResolveOptions};
use crate::session::AnalysisSession;
use crate::{EngineError, EngineResult};
use steps::ModeEntry;

/// Every decision of one run, with its cause. The fields are private and the
/// type has no `Default`, no `Clone` and no `&mut self` method, so a run
/// stays as the resolver made it.
#[derive(Debug)]
pub struct ResolvedRun {
    surface: Surface,
    command: CommandKind,
    root: Sourced<PathBuf>,
    config_file: Sourced<Option<PathBuf>>,
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
    /// The environment snapshot of the run, without the variables that the
    /// run does not read.
    env: EnvSnapshot,
    env_read: Vec<EnvVar>,
    env_ignored: Vec<EnvVar>,
    fell_back: bool,
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
    pub threads: Sourced<usize>,
    pub no_cache: Sourced<bool>,
    pub allow_remote_extends: Sourced<bool>,
    pub max_file_size_bytes: Sourced<Option<u64>>,
    pub cache_dir: Sourced<PathBuf>,
    pub cache_max_size_mb: Sourced<Option<u32>>,
    pub package_baselines: Sourced<bool>,
    pub include_entry_exports: Sourced<bool>,
    pub output: OutputFormat,
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

    /// The set variables that the run reads.
    pub fn env_read(&self) -> &[EnvVar] {
        &self.env_read
    }

    /// The set variables that the run ignores on its surface and command.
    pub fn env_ignored(&self) -> &[EnvVar] {
        &self.env_ignored
    }

    /// Whether the run uses the default config because the discovered config
    /// failed to load, to validate or to give workspace metadata.
    pub const fn fell_back(&self) -> bool {
        self.fell_back
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

impl Runnable<'_> {
    /// The analysis.
    pub const fn analysis(&self) -> PlannedAnalysis {
        self.analysis
    }

    /// The production mode of the analysis.
    pub const fn production(&self) -> &Sourced<bool> {
        self.run.production(self.analysis)
    }

    fn entry(&self) -> &ModeEntry {
        let Some(entry) = self.run.entry(*self.production().value()) else {
            unreachable!("the resolver makes a mode entry for each analysis that runs");
        };
        entry
    }

    /// The resolved config of the production mode of the analysis.
    pub fn config(&self) -> &ResolvedConfig {
        &self.entry().config
    }

    /// The workspaces under the root.
    pub fn workspaces(&self) -> &[WorkspaceInfo] {
        &self.entry().workspaces
    }

    /// Whether the two analyses analyze the same files: both run in the same
    /// production mode.
    pub fn shares_files_with(&self, other: Runnable<'_>) -> bool {
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
        AnalysisSession::from_config(project)
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
            env: Box::new(self.run.env.clone()),
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
            format!("{:?}", decided.ignore_patterns) == format!("{:?}", adjusted.ignore_patterns),
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
