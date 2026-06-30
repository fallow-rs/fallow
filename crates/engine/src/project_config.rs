//! Project config resolution owned by the engine boundary.

use std::path::{Path, PathBuf};

use fallow_config::{FallowConfig, ProductionAnalysis, ResolvedConfig, WorkspaceDiagnostic};
use fallow_types::output_format::OutputFormat;

use crate::{EngineError, EngineResult, engine_error};

/// Resolved project config plus the config file path when one was loaded.
#[derive(Debug)]
pub struct ProjectConfig {
    pub config: ResolvedConfig,
    pub path: Option<PathBuf>,
    pub workspace_diagnostics: Vec<WorkspaceDiagnostic>,
}

/// Scalar config-loading knobs for one analysis family.
#[derive(Debug, Clone, Copy)]
pub struct ProjectConfigOptions {
    pub output: OutputFormat,
    pub no_cache: bool,
    pub threads: usize,
    pub production_override: Option<bool>,
    pub quiet: bool,
    pub analysis: ProductionAnalysis,
}

/// Resolve the analysis config for a project.
///
/// # Errors
///
/// Returns an error when an explicit config cannot be loaded or automatic
/// config discovery finds an invalid config.
pub fn config_for_project(root: &Path, config_path: Option<&Path>) -> EngineResult<ProjectConfig> {
    fallow_core::config_for_project(root, config_path)
        .map(|(config, path)| ProjectConfig {
            workspace_diagnostics: collect_workspace_diagnostics(&config),
            config,
            path,
        })
        .map_err(engine_error)
}

/// Resolve the parse-cache size limit for a resolved config.
#[must_use]
pub fn resolve_cache_max_size_bytes(config: &ResolvedConfig) -> usize {
    fallow_core::resolve_cache_max_size_bytes(config)
}

pub fn default_project_config(root: &Path) -> ProjectConfig {
    let threads = std::thread::available_parallelism().map_or(1, std::num::NonZeroUsize::get);
    ProjectConfig {
        config: FallowConfig::default().resolve(
            root.to_path_buf(),
            OutputFormat::Human,
            threads,
            false,
            true,
            None,
        ),
        path: None,
        workspace_diagnostics: Vec::new(),
    }
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
    let user_config = load_user_config(root, config_path)?;
    let loaded_user_config = user_config.is_some();
    let (mut config, path) = match user_config {
        Some((config, path)) => (config, Some(path)),
        None => (
            FallowConfig {
                production: options.production_override.unwrap_or(false).into(),
                ..FallowConfig::default()
            },
            None,
        ),
    };

    if loaded_user_config {
        let production = options
            .production_override
            .unwrap_or_else(|| config.production.for_analysis(options.analysis));
        config.production = production.into();
    }
    validate_config(root, &config)?;
    let resolved = config.resolve(
        root.to_path_buf(),
        options.output,
        options.threads,
        options.no_cache,
        options.quiet,
        None,
    );
    Ok(ProjectConfig {
        workspace_diagnostics: collect_workspace_diagnostics(&resolved),
        config: resolved,
        path,
    })
}

fn collect_workspace_diagnostics(config: &ResolvedConfig) -> Vec<WorkspaceDiagnostic> {
    fallow_config::discover_workspaces_with_diagnostics(&config.root, &config.ignore_patterns)
        .map(|(_, diagnostics)| diagnostics)
        .unwrap_or_default()
}

fn load_user_config(
    root: &Path,
    config_path: Option<&Path>,
) -> EngineResult<Option<(FallowConfig, PathBuf)>> {
    if let Some(path) = config_path {
        let config = FallowConfig::load(path)
            .map_err(|err| EngineError::new(format!("invalid config: {err:#}")))?;
        return Ok(Some((config, path.to_path_buf())));
    }
    FallowConfig::find_and_load(root)
        .map_err(|err| EngineError::new(format!("invalid config: {err}")))
}

fn validate_config(root: &Path, config: &FallowConfig) -> EngineResult<()> {
    fallow_config::discover_and_validate_external_plugins(root, &config.plugins)
        .map_err(|errors| joined_config_errors("invalid external plugin definition", &errors))?;
    config
        .validate_resolved_boundaries(root)
        .map_err(|errors| joined_config_errors("invalid boundary configuration", &errors))?;
    fallow_config::load_rule_packs(root, &config.rule_packs)
        .map_err(|errors| joined_config_errors("invalid rule pack", &errors))?;
    Ok(())
}

fn joined_config_errors(label: &str, errors: &[impl ToString]) -> EngineError {
    let joined = errors
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("\n  - ");
    EngineError::new(format!("{label}:\n  - {joined}"))
}
