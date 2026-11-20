//! Project config resolution owned by the engine boundary.

use std::path::{Path, PathBuf};

use fallow_config::{EnvSnapshot, FallowConfig, ProductionAnalysis, ResolvedConfig};
use fallow_types::output_format::OutputFormat;
use fallow_types::sourced::{Given, Origin};

use crate::EngineResult;
use crate::resolved_run::steps::{GivenSettings, ModeEntry, flatten_production, validate_config};
pub(crate) use crate::resolved_run::steps::{collect_workspace_metadata, resolve_with_env_from};
use crate::resolved_run::{PerAnalysis, ProductionInputs, decide_production};

#[cfg(any(test, feature = "testing"))]
mod direct_load;
mod loader;

#[cfg(any(test, feature = "testing"))]
pub(crate) use direct_load::default_project_config;
#[cfg(test)]
pub(crate) use direct_load::{
    ProjectConfigOptions, config_for_project_analysis, config_for_project_analysis_with_env,
    config_for_project_with_env,
};
#[cfg(any(test, feature = "testing"))]
pub use direct_load::{
    config_for_project, config_for_project_with_load_options, config_for_project_with_production,
};

#[cfg(any(test, feature = "testing"))]
pub use loader::load_count;
pub use loader::{
    ConfigChainSnapshot, ConfigFile, ConfigFileOptions, ConfigLoadError, find_config_file,
    load_config_file,
};

/// The production flags of a config load without a resolved run: the global
/// `--production` override and one override per analysis
/// (`--production-dead-code`, `--production-health`, `--production-dupes`).
///
/// The precedence is the precedence of the resolver without environment
/// variables: the flag of the analysis, then the global override, then the
/// config. A run of a surface decides the production mode in
/// [`crate::resolved_run`]; these flags serve the loads of
/// [`ConfigFile::for_analysis`] that have no run.
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

/// The resolved config of one mode entry, with the config file path and the
/// workspace metadata of the load. A session holds it. Outside this crate
/// only tests and benchmarks (feature `testing`) see the type.
mod project {
    use std::path::PathBuf;

    use fallow_config::{ResolvedConfig, WorkspaceDiagnostic, WorkspaceInfo};

    use super::DuplicationScope;

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
}

#[cfg(any(test, feature = "testing"))]
pub use project::ProjectConfig;
#[cfg(not(any(test, feature = "testing")))]
pub(crate) use project::ProjectConfig;

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

/// Resolve the parse-cache size limit for a resolved config.
#[must_use]
pub(crate) fn resolve_cache_max_size_bytes(config: &ResolvedConfig) -> usize {
    config
        .cache_max_size_mb
        .map_or(fallow_extract::cache::DEFAULT_CACHE_MAX_SIZE, |mb| {
            (mb as usize).saturating_mul(1024 * 1024)
        })
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
