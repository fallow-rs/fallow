//! Project config resolution owned by the engine boundary.

use std::path::{Path, PathBuf};

use fallow_config::{
    FallowConfig, ProductionAnalysis, ResolvedConfig, WorkspaceDiagnostic, WorkspaceInfo,
};
use fallow_types::output_format::OutputFormat;
use rustc_hash::FxHashSet;

use crate::{EngineError, EngineResult};

/// Resolved project config plus the config file path when one was loaded.
#[derive(Debug)]
pub struct ProjectConfig {
    pub config: ResolvedConfig,
    pub path: Option<PathBuf>,
    pub workspaces: Vec<WorkspaceInfo>,
    pub workspace_diagnostics: Vec<WorkspaceDiagnostic>,
    pub workspace_discovery_ms: Option<f64>,
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
    let user_config = load_user_config(root, config_path)?;
    let (mut config, path) = match user_config {
        Some((config, path)) => (config, Some(path)),
        None => (FallowConfig::default(), None),
    };
    if path.is_some() {
        config.production = config
            .production
            .for_analysis(ProductionAnalysis::DeadCode)
            .into();
        validate_boundaries_and_rule_packs(root, &config)?;
    }
    let threads = std::thread::available_parallelism().map_or(1, std::num::NonZeroUsize::get);
    let resolved = config.resolve(
        root.to_path_buf(),
        OutputFormat::Human,
        threads,
        false,
        true,
        None,
    );
    let (workspaces, workspace_diagnostics, workspace_discovery_ms) =
        collect_workspace_metadata(&resolved)?;
    Ok(ProjectConfig {
        config: resolved,
        path,
        workspaces,
        workspace_diagnostics,
        workspace_discovery_ms: Some(workspace_discovery_ms),
    })
}

/// Resolve the parse-cache size limit for a resolved config.
#[must_use]
pub fn resolve_cache_max_size_bytes(config: &ResolvedConfig) -> usize {
    config
        .cache_max_size_mb
        .map_or(fallow_extract::cache::DEFAULT_CACHE_MAX_SIZE, |mb| {
            (mb as usize).saturating_mul(1024 * 1024)
        })
}

pub fn default_project_config(root: &Path) -> ProjectConfig {
    let threads = std::thread::available_parallelism().map_or(1, std::num::NonZeroUsize::get);
    let config = FallowConfig::default().resolve(
        root.to_path_buf(),
        OutputFormat::Human,
        threads,
        false,
        true,
        None,
    );
    let (workspaces, workspace_diagnostics, workspace_discovery_ms) =
        collect_workspace_metadata_lossy(&config);
    ProjectConfig {
        config,
        path: None,
        workspaces,
        workspace_diagnostics,
        workspace_discovery_ms: Some(workspace_discovery_ms),
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
    let (workspaces, workspace_diagnostics, workspace_discovery_ms) =
        collect_workspace_metadata(&resolved)?;
    Ok(ProjectConfig {
        config: resolved,
        path,
        workspaces,
        workspace_diagnostics,
        workspace_discovery_ms: Some(workspace_discovery_ms),
    })
}

fn collect_workspace_metadata(
    config: &ResolvedConfig,
) -> EngineResult<(Vec<WorkspaceInfo>, Vec<WorkspaceDiagnostic>, f64)> {
    let start = std::time::Instant::now();
    let (workspaces, diagnostics) =
        fallow_config::discover_workspaces_with_diagnostics(&config.root, &config.ignore_patterns)
            .map_err(|err| EngineError::new(err.to_string()))?;
    let diagnostics = with_undeclared_workspace_diagnostics(config, &workspaces, diagnostics);
    let elapsed_ms = start.elapsed().as_secs_f64() * 1000.0;
    Ok((workspaces, diagnostics, elapsed_ms))
}

fn collect_workspace_metadata_lossy(
    config: &ResolvedConfig,
) -> (Vec<WorkspaceInfo>, Vec<WorkspaceDiagnostic>, f64) {
    collect_workspace_metadata(config).unwrap_or_default()
}

fn with_undeclared_workspace_diagnostics(
    config: &ResolvedConfig,
    workspaces: &[WorkspaceInfo],
    mut diagnostics: Vec<WorkspaceDiagnostic>,
) -> Vec<WorkspaceDiagnostic> {
    let mut existing: FxHashSet<PathBuf> = diagnostics
        .iter()
        .map(|diagnostic| {
            dunce::canonicalize(&diagnostic.path).unwrap_or_else(|_| diagnostic.path.clone())
        })
        .collect();
    for diagnostic in fallow_config::find_undeclared_workspaces_with_ignores(
        &config.root,
        workspaces,
        &config.ignore_patterns,
    ) {
        let canonical =
            dunce::canonicalize(&diagnostic.path).unwrap_or_else(|_| diagnostic.path.clone());
        if existing.insert(canonical) {
            diagnostics.push(diagnostic);
        }
    }
    diagnostics
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
    validate_boundaries_and_rule_packs(root, config)
}

fn validate_boundaries_and_rule_packs(root: &Path, config: &FallowConfig) -> EngineResult<()> {
    config
        .validate_resolved_boundaries(root)
        .map_err(|errors| joined_config_errors("invalid boundary configuration", &errors))?;
    let packs = fallow_config::load_rule_packs(root, &config.rule_packs)
        .map_err(|errors| joined_config_errors("invalid rule pack", &errors))?;
    let boundaries =
        fallow_config::resolve_boundaries_for_rule_pack_validation(config.boundaries.clone(), root);
    let zone_errors = fallow_config::validate_rule_pack_zone_references(
        root,
        &config.rule_packs,
        &packs,
        &boundaries,
    );
    if !zone_errors.is_empty() {
        return Err(joined_config_errors("invalid rule pack", &zone_errors));
    }
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
