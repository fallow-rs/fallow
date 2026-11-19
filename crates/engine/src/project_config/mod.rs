//! Project config resolution owned by the engine boundary.

use std::path::{Path, PathBuf};

use fallow_config::{
    ConfigLoadOptions, EnvSnapshot, FallowConfig, ProductionAnalysis, ResolvedConfig,
    WorkspaceDiagnostic, WorkspaceInfo,
};
use fallow_types::output_format::OutputFormat;
use fallow_types::sourced::{Given, Origin};

use crate::EngineResult;
use crate::resolved_run::steps::{
    GivenSettings, ModeEntry, flatten_production, mode_entry, mode_entry_lossy,
    validate_boundaries_and_rule_packs, validate_config,
};
pub(crate) use crate::resolved_run::steps::{collect_workspace_metadata, resolve_with_env_from};
use crate::resolved_run::{PerAnalysis, ProductionInputs, decide_production};

mod loader;

#[cfg(any(test, feature = "testing"))]
pub use loader::load_count;
pub use loader::{
    ConfigChainSnapshot, ConfigFile, ConfigFileOptions, ConfigLoadError, find_config_file,
    load_config_file,
};

/// The production flags of one run: the global `--production` override and one
/// override per analysis (`--production-dead-code`, `--production-health`,
/// `--production-dupes`).
///
/// Every command and surface that runs more than one analysis reads the
/// production mode of each analysis from here, so the precedence has one
/// implementation: the flag of the analysis, then the global override, then
/// the config.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ProductionFlags {
    /// The global override. The CLI sets `Some(true)` for `--production` and
    /// `None` without it. The programmatic API can also pass `Some(false)`.
    pub global: Option<bool>,
    /// Override for the dead-code analysis.
    pub dead_code: Option<bool>,
    /// Override for the health analysis.
    pub health: Option<bool>,
    /// Override for the duplication analysis.
    pub dupes: Option<bool>,
}

impl ProductionFlags {
    /// The flags of a CLI run, where `--production` can only switch the mode
    /// on.
    #[must_use]
    pub const fn from_cli(
        production: bool,
        dead_code: Option<bool>,
        health: Option<bool>,
        dupes: Option<bool>,
    ) -> Self {
        Self {
            global: if production { Some(true) } else { None },
            dead_code,
            health,
            dupes,
        }
    }

    /// The production override of a command that runs one analysis
    /// (`dead-code`, `dupes`, `health`): its own override, else
    /// `--production`. `None` means that the config decides.
    #[must_use]
    pub const fn single_analysis_override(production: bool, own: Option<bool>) -> Option<bool> {
        Self::from_cli(production, own, None, None).override_for(ProductionAnalysis::DeadCode)
    }

    /// The override flag of one analysis, without the global override.
    #[must_use]
    const fn own(self, analysis: ProductionAnalysis) -> Option<bool> {
        match analysis {
            ProductionAnalysis::DeadCode => self.dead_code,
            ProductionAnalysis::Health => self.health,
            ProductionAnalysis::Dupes => self.dupes,
        }
    }

    /// The production override of one analysis: its own flag, else the global
    /// override. `None` means that the config decides.
    #[must_use]
    pub const fn override_for(self, analysis: ProductionAnalysis) -> Option<bool> {
        match self.own(analysis) {
            Some(value) => Some(value),
            None => self.global,
        }
    }

    /// The production mode of one analysis as far as the flags decide, before
    /// the config is loaded. A missing flag reads as `false`.
    #[must_use]
    pub const fn mode(self, analysis: ProductionAnalysis) -> bool {
        match self.override_for(analysis) {
            Some(value) => value,
            None => false,
        }
    }

    /// The production mode of each analysis as far as the flags decide.
    #[must_use]
    pub const fn modes(self) -> ProductionModes {
        ProductionModes {
            dead_code: self.mode(ProductionAnalysis::DeadCode),
            health: self.mode(ProductionAnalysis::Health),
            dupes: self.mode(ProductionAnalysis::Dupes),
        }
    }

    /// The effective production mode of one analysis: the flags first, then
    /// the `production` setting of the config.
    #[must_use]
    pub fn effective(
        self,
        analysis: ProductionAnalysis,
        config: &fallow_config::ProductionConfig,
    ) -> bool {
        *decide_production(&self.as_inputs(), config, None)
            .get(analysis)
            .value()
    }

    /// The effective production mode of each analysis.
    #[cfg(test)]
    #[must_use]
    pub(crate) fn effective_modes(
        self,
        config: &fallow_config::ProductionConfig,
    ) -> ProductionModes {
        let decided = decide_production(&self.as_inputs(), config, None);
        ProductionModes {
            dead_code: decided.dead_code.into_value(),
            health: decided.health.into_value(),
            dupes: decided.dupes.into_value(),
        }
    }

    /// The override of each analysis from `self`, else from `fallback`. The
    /// parity tests use it to put the flags before the environment
    /// variables. The config decides only when neither source sets a
    /// value. `fallback` has the rank of the environment variables in
    /// `decide_production`.
    #[cfg(test)]
    #[must_use]
    pub(crate) fn with_fallback(self, fallback: Self) -> Self {
        use fallow_types::sourced::{Layer, Sourced};

        let inputs = ProductionInputs {
            global_env: fallback.global,
            own_env: PerAnalysis {
                dead_code: fallback.dead_code,
                health: fallback.health,
                dupes: fallback.dupes,
            },
            ..self.as_inputs()
        };
        let decided = decide_production(&inputs, &fallow_config::ProductionConfig::default(), None);
        let given = |decision: &Sourced<bool>| {
            (decision.source().layer() != Layer::Default).then_some(*decision.value())
        };
        Self {
            global: None,
            dead_code: given(&decided.dead_code),
            health: given(&decided.health),
            dupes: given(&decided.dupes),
        }
    }

    /// The flags as caller-given production inputs, without environment
    /// variables.
    fn as_inputs(self) -> ProductionInputs {
        let given = |value: Option<bool>, name| {
            value.map(|value| Given {
                value,
                origin: Origin::ApiOption(name),
            })
        };
        ProductionInputs {
            global_flag: given(self.global, "ProductionFlags.global"),
            own_flag: PerAnalysis {
                dead_code: given(self.dead_code, "ProductionFlags.dead_code"),
                health: given(self.health, "ProductionFlags.health"),
                dupes: given(self.dupes, "ProductionFlags.dupes"),
            },
            global_env: None,
            own_env: PerAnalysis::default(),
        }
    }
}

/// The production mode of each analysis of one run.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ProductionModes {
    /// Production mode of the dead-code analysis.
    pub dead_code: bool,
    /// Production mode of the health analysis.
    pub health: bool,
    /// Production mode of the duplication analysis.
    pub dupes: bool,
}

impl ProductionModes {
    /// The production mode of one analysis.
    #[must_use]
    pub const fn for_analysis(self, analysis: ProductionAnalysis) -> bool {
        match analysis {
            ProductionAnalysis::DeadCode => self.dead_code,
            ProductionAnalysis::Health => self.health,
            ProductionAnalysis::Dupes => self.dupes,
        }
    }

    /// Whether health can reuse the dead-code parse: both run in the same mode.
    #[must_use]
    pub const fn dead_code_matches_health(self) -> bool {
        self.dead_code == self.health
    }

    /// Whether duplication can reuse the dead-code files: both run in the same
    /// mode.
    #[must_use]
    pub const fn dead_code_matches_dupes(self) -> bool {
        self.dead_code == self.dupes
    }

    /// Whether all three analyses run in the same mode.
    #[must_use]
    pub const fn all_match(self) -> bool {
        self.dead_code_matches_health() && self.dead_code_matches_dupes()
    }
}

/// Resolved project config plus the config file path when one was loaded.
#[derive(Debug)]
pub struct ProjectConfig {
    /// Fully resolved config for the project.
    pub config: ResolvedConfig,
    /// Path of the loaded config file; `None` when defaults were used.
    pub path: Option<PathBuf>,
    /// Workspace metadata discovered under the project root.
    pub workspaces: Vec<WorkspaceInfo>,
    /// Diagnostics from workspace discovery (undeclared or invalid members).
    pub workspace_diagnostics: Vec<WorkspaceDiagnostic>,
    /// Workspace discovery wall time in milliseconds, when measured.
    pub(crate) workspace_discovery_ms: Option<f64>,
    /// The plugin files, rule packs and `autoDiscover` directories that
    /// config resolution read.
    pub inputs: fallow_config::ConfigInputs,
    /// The content of [`Self::inputs`] just before resolution read them.
    pub(crate) inputs_before_resolve: fallow_config::ConfigInputsSnapshot,
    /// The files that duplication analyzes.
    pub(crate) duplication: DuplicationScope,
}

/// The files that duplication analyzes.
#[derive(Debug, Clone)]
pub(crate) enum DuplicationScope {
    /// The production mode of duplication equals the mode of the session
    /// config, so duplication analyzes the files of the session.
    SameAsSession,
    /// The production mode of duplication differs from the mode of the
    /// session config. The session resolves this config, which has the
    /// `production` setting of duplication, and walks its files on the first
    /// duplication call.
    Own {
        config: Box<FallowConfig>,
        options: ResolveOptions,
        env: Box<EnvSnapshot>,
        /// The settings that win over `env`. The session writes them on the
        /// config after it resolves the config with `env`.
        given: Box<GivenSettings>,
    },
}

impl DuplicationScope {
    /// The scope of a session config that is flattened for dead code. When
    /// the flags and the config give duplication another production mode,
    /// duplication gets a copy of the config flattened for its own mode.
    fn for_dead_code_session(
        file: &ConfigFile,
        flags: ProductionFlags,
        options: ResolveOptions,
        env: &EnvSnapshot,
    ) -> Self {
        let mode = |analysis| {
            if file.path.is_some() {
                flags.effective(analysis, &file.config.production)
            } else {
                flags.mode(analysis)
            }
        };
        if mode(ProductionAnalysis::Dupes) == mode(ProductionAnalysis::DeadCode) {
            return Self::SameAsSession;
        }
        let dupes = ConfigFile {
            config: file.config.clone(),
            path: file.path.clone(),
            chain: file.chain.clone(),
        }
        .for_analysis(ProductionAnalysis::Dupes, flags);
        Self::Own {
            config: Box::new(dupes.config),
            options,
            env: Box::new(env.clone()),
            given: Box::default(),
        }
    }
}

/// The run settings that resolution records on the config.
#[derive(Debug, Clone, Copy)]
pub struct ResolveOptions {
    /// Output format the resolved config carries into rendering.
    pub output: OutputFormat,
    /// Worker thread count recorded on the resolved config.
    pub threads: usize,
    /// Bypass the parse cache for this run.
    pub no_cache: bool,
    /// Suppress progress notes on stderr.
    pub quiet: bool,
}

impl ResolveOptions {
    /// The settings of a host that resolves a config without a CLI run.
    fn host_default() -> Self {
        Self {
            output: OutputFormat::Human,
            threads: std::thread::available_parallelism().map_or(1, std::num::NonZeroUsize::get),
            no_cache: false,
            quiet: true,
        }
    }
}

impl ConfigFile {
    /// Flatten the `production` setting of the config for one analysis: the
    /// flags decide first, then the config. A project without a config file
    /// gets the mode of the flags.
    #[must_use]
    pub fn for_analysis(mut self, analysis: ProductionAnalysis, flags: ProductionFlags) -> Self {
        let from_file = self.path.is_some();
        let on = if from_file {
            flags.effective(analysis, &self.config.production)
        } else {
            flags.mode(analysis)
        };
        flatten_production(&mut self.config, from_file, on);
        self
    }

    /// Validate the external plugins, the boundaries and the rule packs of
    /// the config.
    ///
    /// # Errors
    ///
    /// Returns an error that names the first invalid part and lists each of
    /// its problems.
    pub fn validate(&self, root: &Path) -> EngineResult<()> {
        validate_config(root, &self.config)
    }

    /// Resolve the config for `root`, then apply the environment variables
    /// that every host honors.
    #[must_use]
    pub fn resolve(self, root: &Path, options: ResolveOptions) -> ResolvedConfig {
        resolve_with_env(self.config, root, options)
    }
}

fn resolve_with_env(config: FallowConfig, root: &Path, options: ResolveOptions) -> ResolvedConfig {
    resolve_with_env_from(config, root, options, &EnvSnapshot::capture())
}

impl ModeEntry {
    /// The project config of this mode entry.
    pub(crate) fn into_project_config(
        self,
        path: Option<PathBuf>,
        duplication: DuplicationScope,
    ) -> ProjectConfig {
        ProjectConfig {
            config: self.config,
            path,
            workspaces: self.workspaces,
            workspace_diagnostics: self.workspace_diagnostics,
            workspace_discovery_ms: Some(self.workspace_discovery_ms),
            inputs: self.inputs,
            inputs_before_resolve: self.inputs_before_resolve,
            duplication,
        }
    }
}

/// Scalar config-loading knobs for one analysis family.
#[derive(Debug, Clone, Copy)]
pub struct ProjectConfigOptions {
    /// Output format the resolved config carries into rendering.
    pub output: OutputFormat,
    /// Bypass the parse cache for this run.
    pub no_cache: bool,
    /// Worker thread count recorded on the resolved config.
    pub threads: usize,
    /// Tri-state production override: `Some` forces production-only analysis
    /// on or off regardless of config, `None` defers to the config value.
    pub production_override: Option<bool>,
    /// Suppress progress notes on stderr.
    pub quiet: bool,
    /// Which analysis family's per-analysis production config to flatten.
    pub analysis: ProductionAnalysis,
    /// Permit `extends` config inheritance from remote URLs.
    pub allow_remote_extends: bool,
}

/// Resolved project configuration plus doctor-specific plugin diagnostics.
#[derive(Debug)]
#[non_exhaustive]
pub struct ProjectConfigReadiness {
    /// The same resolved configuration returned by [`config_for_project_analysis`].
    pub project: ProjectConfig,
    /// Unresolved resources named explicitly by the `plugins` config field.
    pub configured_plugin_diagnostics: Vec<fallow_config::ConfiguredPluginDiagnostic>,
}

/// Resolve the analysis config for a project.
///
/// # Errors
///
/// Returns an error when an explicit config cannot be loaded or automatic
/// config discovery finds an invalid config.
pub fn config_for_project(root: &Path, config_path: Option<&Path>) -> EngineResult<ProjectConfig> {
    config_for_project_with_load_options(root, config_path, ConfigLoadOptions::default())
}

/// Resolve project config with an explicit inheritance trust policy.
///
/// # Errors
///
/// Returns an error when config loading or validation fails.
pub fn config_for_project_with_load_options(
    root: &Path,
    config_path: Option<&Path>,
    load_options: ConfigLoadOptions,
) -> EngineResult<ProjectConfig> {
    config_for_project_with_production(root, config_path, load_options, ProductionFlags::default())
}

/// Resolve project config with an explicit inheritance trust policy and the
/// production flags of the run. The flags decide the production mode before
/// resolution, so the rules that production mode turns off are off, as with
/// the CLI `--production` flag. The config holds the production mode of dead
/// code. Duplication in a session of this config uses its own production mode,
/// as `fallow dupes` does.
///
/// # Errors
///
/// Returns an error when config loading or validation fails.
pub fn config_for_project_with_production(
    root: &Path,
    config_path: Option<&Path>,
    load_options: ConfigLoadOptions,
    production: ProductionFlags,
) -> EngineResult<ProjectConfig> {
    load_project_config(
        root,
        config_path,
        load_options,
        production,
        &EnvSnapshot::capture(),
    )
}

/// Load a project config as [`config_for_project_with_production`] does, with
/// the environment variables of `env` in place of the process environment.
#[cfg(test)]
pub(crate) fn config_for_project_with_env(
    root: &Path,
    config_path: Option<&Path>,
    env: &EnvSnapshot,
) -> EngineResult<ProjectConfig> {
    load_project_config(
        root,
        config_path,
        ConfigLoadOptions::default(),
        ProductionFlags::default(),
        env,
    )
}

fn load_project_config(
    root: &Path,
    config_path: Option<&Path>,
    load_options: ConfigLoadOptions,
    production: ProductionFlags,
    env: &EnvSnapshot,
) -> EngineResult<ProjectConfig> {
    let file = load_config_file(
        root,
        config_path,
        ConfigFileOptions {
            allow_remote_extends: load_options.allow_remote_extends,
        },
    )?;
    let options = ResolveOptions::host_default();
    let duplication = DuplicationScope::for_dead_code_session(&file, production, options, env);
    let ConfigFile { config, path, .. } =
        file.for_analysis(ProductionAnalysis::DeadCode, production);
    if path.is_some() {
        validate_boundaries_and_rule_packs(root, &config)?;
    }
    Ok(mode_entry(root, config, options, env)?.into_project_config(path, duplication))
}

impl ProjectConfig {
    /// Drop the workspace metadata of the load, so that a session built from
    /// this config discovers the workspaces again. A caller that changes the
    /// resolved config after the load uses this.
    #[must_use]
    pub fn without_workspace_metadata(mut self) -> Self {
        self.workspaces.clear();
        self.workspace_diagnostics.clear();
        self.workspace_discovery_ms = None;
        self
    }
}

/// Resolve the parse-cache size limit for a resolved config.
#[must_use]
pub(crate) fn resolve_cache_max_size_bytes(config: &ResolvedConfig) -> usize {
    config
        .cache_max_size_mb
        .map_or(fallow_extract::cache::DEFAULT_CACHE_MAX_SIZE, |mb| {
            (mb as usize).saturating_mul(1024 * 1024)
        })
}

pub(crate) fn default_project_config(root: &Path) -> ProjectConfig {
    mode_entry_lossy(
        root,
        FallowConfig::default(),
        ResolveOptions::host_default(),
        &EnvSnapshot::capture(),
    )
    .into_project_config(None, DuplicationScope::SameAsSession)
}

/// Resolve config for a specific analysis without depending on the CLI crate.
///
/// This mirrors the CLI's core config semantics: explicit production overrides
/// are applied before resolution, per-analysis production config is flattened
/// for the requested analysis, and boundary / external plugin / rule-pack
/// validation happens before the resolved config reaches the engine.
///
/// # Errors
///
/// Returns an engine error when config loading or validation fails.
pub fn config_for_project_analysis(
    root: &Path,
    config_path: Option<&Path>,
    options: ProjectConfigOptions,
) -> EngineResult<ProjectConfig> {
    resolve_project_config_analysis(root, config_path, options, &EnvSnapshot::capture())
        .map(|(project, _)| project)
}

/// [`config_for_project_analysis`] with the environment variables of `env`
/// in place of the process environment.
#[cfg(test)]
pub(crate) fn config_for_project_analysis_with_env(
    root: &Path,
    config_path: Option<&Path>,
    options: ProjectConfigOptions,
    env: &EnvSnapshot,
) -> EngineResult<ProjectConfig> {
    resolve_project_config_analysis(root, config_path, options, env).map(|(project, _)| project)
}

/// Resolve project configuration and collect typed readiness diagnostics for
/// plugin resources named explicitly by the user configuration.
///
/// # Errors
///
/// Returns an engine error when config loading or validation fails.
pub fn config_for_project_readiness(
    root: &Path,
    config_path: Option<&Path>,
    options: ProjectConfigOptions,
) -> EngineResult<ProjectConfigReadiness> {
    let (project, configured_plugin_paths) =
        resolve_project_config_analysis(root, config_path, options, &EnvSnapshot::capture())?;
    let configured_plugin_diagnostics =
        fallow_config::diagnose_configured_external_plugins(root, &configured_plugin_paths);
    Ok(ProjectConfigReadiness {
        project,
        configured_plugin_diagnostics,
    })
}

fn resolve_project_config_analysis(
    root: &Path,
    config_path: Option<&Path>,
    options: ProjectConfigOptions,
    env: &EnvSnapshot,
) -> EngineResult<(ProjectConfig, Vec<String>)> {
    let file = load_config_file(
        root,
        config_path,
        ConfigFileOptions {
            allow_remote_extends: options.allow_remote_extends,
        },
    )?
    .for_analysis(
        options.analysis,
        ProductionFlags {
            global: options.production_override,
            ..ProductionFlags::default()
        },
    );
    file.validate(root)?;
    let ConfigFile { config, path, .. } = file;
    let configured_plugin_paths = config.plugins.clone();
    let options = ResolveOptions {
        output: options.output,
        threads: options.threads,
        no_cache: options.no_cache,
        quiet: options.quiet,
    };
    let entry = mode_entry(root, config, options, env)?;
    // The caller names one analysis and owns the production mode of each
    // other analysis, so a session of this config runs duplication only when
    // the caller found that the modes match.
    let project = entry.into_project_config(path, DuplicationScope::SameAsSession);
    Ok((project, configured_plugin_paths))
}

#[cfg(test)]
mod tests {
    use super::*;
    use fallow_config::EnvVar;

    use crate::resolved_run::steps::apply_env_overrides_from;

    fn lookup_from(vars: &[(&str, &str)]) -> EnvSnapshot {
        EnvSnapshot::from_pairs(vars, None)
    }

    #[test]
    fn env_overrides_reach_every_host_config() {
        let root = Path::new("/repo");
        let mut config = default_project_config(root).config;

        apply_env_overrides_from(
            &mut config,
            &lookup_from(&[
                ("FALLOW_MAX_FILE_SIZE", "7"),
                ("FALLOW_CACHE_DIR", ".cache/fallow"),
                ("FALLOW_CACHE_MAX_SIZE", "64"),
            ]),
        );

        assert_eq!(
            config.max_file_size_bytes,
            fallow_config::resolve_max_file_size_bytes(Some(7))
        );
        assert_eq!(config.cache_dir, root.join(".cache/fallow"));
        assert_eq!(config.cache_max_size_mb, Some(64));
    }

    /// `FALLOW_CACHE_MAX_SIZE` gives the same limit when a host passes it to
    /// `resolve` and when the engine applies it after `resolve`.
    #[test]
    fn cache_max_size_env_gives_one_limit_before_and_after_resolve() {
        let root = Path::new("/repo");
        let resolve = |configured: Option<u32>, argument: Option<u32>| {
            FallowConfig {
                cache: fallow_config::CacheConfig {
                    dir: None,
                    max_size_mb: configured,
                },
                ..FallowConfig::default()
            }
            .resolve(
                root.to_path_buf(),
                OutputFormat::Human,
                1,
                false,
                true,
                argument,
            )
        };
        let values: [&[(&str, &str)]; 5] = [
            &[],
            &[("FALLOW_CACHE_MAX_SIZE", "64")],
            &[("FALLOW_CACHE_MAX_SIZE", " 8 ")],
            &[("FALLOW_CACHE_MAX_SIZE", "0")],
            &[("FALLOW_CACHE_MAX_SIZE", "many")],
        ];
        for configured in [None, Some(5)] {
            for vars in values {
                let argument = lookup_from(vars)
                    .get(EnvVar::CacheMaxSize)
                    .and_then(fallow_config::cache_max_size_from_env_value);
                let before = resolve(configured, argument);
                let mut after = resolve(configured, None);
                apply_env_overrides_from(&mut after, &lookup_from(vars));
                assert_eq!(
                    before.cache_max_size_mb, after.cache_max_size_mb,
                    "{configured:?} {vars:?}"
                );
            }
        }
    }

    #[test]
    fn invalid_or_empty_env_values_keep_the_config() {
        let root = Path::new("/repo");
        let mut config = default_project_config(root).config;
        let before_dir = config.cache_dir.clone();
        let before_max = config.cache_max_size_mb;

        apply_env_overrides_from(
            &mut config,
            &lookup_from(&[("FALLOW_CACHE_DIR", ""), ("FALLOW_CACHE_MAX_SIZE", "0")]),
        );

        assert_eq!(config.cache_dir, before_dir);
        assert_eq!(config.cache_max_size_mb, before_max);
    }

    #[test]
    fn package_baselines_env_turns_the_map_off_only_for_a_false_value() {
        let root = Path::new("/repo");
        let with_map = || {
            let mut config = default_project_config(root).config;
            config
                .workspace_changed_since
                .insert("packages/web".to_owned(), "main".to_owned());
            config
        };

        let mut disabled = with_map();
        apply_env_overrides_from(
            &mut disabled,
            &lookup_from(&[("FALLOW_PACKAGE_BASELINES", " Off ")]),
        );
        assert!(disabled.workspace_changed_since.is_empty());

        for value in ["true", "1", "maybe", ""] {
            let mut kept = with_map();
            apply_env_overrides_from(
                &mut kept,
                &lookup_from(&[("FALLOW_PACKAGE_BASELINES", value)]),
            );
            assert_eq!(kept.workspace_changed_since.len(), 1, "{value:?}");
        }
    }
}
