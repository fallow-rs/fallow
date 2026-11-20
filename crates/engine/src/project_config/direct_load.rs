//! The project config loads that do not resolve a run. Tests and benchmarks
//! build a session from such a config. A release build does not have them:
//! each session of a release build comes from a resolved run
//! ([`crate::resolved_run`]).

use std::path::Path;

use fallow_config::{ConfigLoadOptions, EnvSnapshot, FallowConfig, ProductionAnalysis};
use fallow_types::output_format::OutputFormat;

use super::{
    ConfigFile, ConfigFileOptions, DuplicationScope, ProductionFlags, ProjectConfig,
    ResolveOptions, load_config_file,
};
use crate::EngineResult;
use crate::resolved_run::steps::{
    mode_entry, mode_entry_lossy, validate_boundaries_and_rule_packs,
};

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

/// Scalar config-loading knobs for one analysis family.
#[cfg(test)]
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
pub fn config_for_project_with_env(
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
        ConfigFileOptions::new(load_options.allow_remote_extends, env),
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

pub fn default_project_config(root: &Path) -> ProjectConfig {
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
#[cfg(test)]
pub fn config_for_project_analysis(
    root: &Path,
    config_path: Option<&Path>,
    options: ProjectConfigOptions,
) -> EngineResult<ProjectConfig> {
    resolve_project_config_analysis(root, config_path, options, &EnvSnapshot::capture())
}

/// [`config_for_project_analysis`] with the environment variables of `env`
/// in place of the process environment.
#[cfg(test)]
pub fn config_for_project_analysis_with_env(
    root: &Path,
    config_path: Option<&Path>,
    options: ProjectConfigOptions,
    env: &EnvSnapshot,
) -> EngineResult<ProjectConfig> {
    resolve_project_config_analysis(root, config_path, options, env)
}

#[cfg(test)]
fn resolve_project_config_analysis(
    root: &Path,
    config_path: Option<&Path>,
    options: ProjectConfigOptions,
    env: &EnvSnapshot,
) -> EngineResult<ProjectConfig> {
    let file = load_config_file(
        root,
        config_path,
        ConfigFileOptions::new(options.allow_remote_extends, env),
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
    Ok(entry.into_project_config(path, DuplicationScope::SameAsSession))
}

#[cfg(test)]
#[path = "direct_load_tests.rs"]
mod tests;
