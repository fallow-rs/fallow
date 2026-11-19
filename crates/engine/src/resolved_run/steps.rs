//! The steps of a config load: validate, flatten the production mode,
//! resolve with the environment, and collect the workspace metadata.
//!
//! [`super::resolve::resolve_run`] and the `config_for_project*` entry
//! points of [`crate::project_config`] call the same steps, so each step has
//! one implementation.

use std::path::{Path, PathBuf};

use fallow_config::{
    CommandKind, ConfigInputs, ConfigInputsSnapshot, EnvSnapshot, EnvVar, FallowConfig,
    ResolvedConfig, WorkspaceDiagnostic, WorkspaceInfo,
};
use rustc_hash::FxHashSet;

use crate::project_config::ResolveOptions;
use crate::{EngineError, EngineResult};

/// The resolved config of one production mode, with its workspace metadata.
#[derive(Debug)]
pub struct ModeEntry {
    /// The resolved config.
    pub config: ResolvedConfig,
    /// The plugin files, rule packs and `autoDiscover` directories that
    /// resolution read.
    pub inputs: ConfigInputs,
    /// The content of [`Self::inputs`] just before resolution read them.
    pub inputs_before_resolve: ConfigInputsSnapshot,
    /// The workspaces under the root.
    pub workspaces: Vec<WorkspaceInfo>,
    /// The diagnostics of workspace discovery.
    pub workspace_diagnostics: Vec<WorkspaceDiagnostic>,
    /// The wall time of workspace discovery, in milliseconds.
    pub workspace_discovery_ms: f64,
}

/// The run settings that a surface gives and that win over the environment.
#[derive(Debug, Clone, Default)]
pub struct GivenSettings {
    /// The max file size in megabytes.
    pub max_file_size_mb: Option<u32>,
    /// The cache directory. A relative path resolves from the project root.
    pub cache_dir: Option<PathBuf>,
    /// Whether dead code reports the exports of entry files.
    pub include_entry_exports: Option<bool>,
    /// Whether the run reads `workspaces.changedSince`.
    pub package_baselines: Option<bool>,
}

impl GivenSettings {
    /// The snapshot without the variables that a given setting replaces. A
    /// given `package_baselines` must remove `FALLOW_PACKAGE_BASELINES`,
    /// because [`apply_env_overrides_from`] clears the baselines and
    /// [`Self::apply`] cannot get them back.
    #[must_use]
    pub(crate) fn env_for_steps(&self, env: &EnvSnapshot) -> EnvSnapshot {
        env.clone().retain(|var| match var {
            EnvVar::MaxFileSize => self.max_file_size_mb.is_none(),
            EnvVar::CacheDir => self.cache_dir.is_none(),
            EnvVar::PackageBaselines => self.package_baselines.is_none(),
            _ => true,
        })
    }

    /// Write the given settings on a config that the steps resolved.
    pub fn apply(&self, config: &mut ResolvedConfig) {
        if let Some(mb) = self.max_file_size_mb {
            config.max_file_size_bytes = fallow_config::resolve_max_file_size_bytes(Some(mb));
        }
        if let Some(dir) = &self.cache_dir {
            config.override_cache_dir(dir.clone());
        }
        // A given `true` wins over each workspace setting, as the
        // `--include-entry-exports` flag does.
        match self.include_entry_exports {
            Some(true) => config.force_include_entry_exports(),
            Some(false) => config.include_entry_exports = false,
            None => {}
        }
        if self.package_baselines == Some(false) {
            config.workspace_changed_since.clear();
        }
    }
}

/// The workspace metadata: the workspaces, their diagnostics and the wall time
/// of the discovery in milliseconds.
type WorkspaceMetadata = (Vec<WorkspaceInfo>, Vec<WorkspaceDiagnostic>, f64);

/// Set the `production` setting of `config` to `on` for every analysis. A
/// config from a file keeps its production `ignore` globs. A config without a
/// file gets the bool form.
pub(crate) fn flatten_production(config: &mut FallowConfig, from_file: bool, on: bool) {
    config.production = if from_file {
        config.production.for_run(on)
    } else {
        on.into()
    };
}

/// Put a cache directory outside the root in a subdirectory of the root, as
/// [`ResolvedConfig::scope_shared_cache_dir_to_root`] does. Returns whether
/// the directory changed.
pub(crate) fn scope_cache_dir_to_root(config: &mut ResolvedConfig) -> bool {
    let shared = config.cache_dir.clone();
    config.scope_shared_cache_dir_to_root();
    config.cache_dir != shared
}

/// Resolve `config` for `root` and collect its workspace metadata. A
/// workspace discovery error is an error.
pub(crate) fn mode_entry(
    root: &Path,
    config: FallowConfig,
    options: ResolveOptions,
    env: &EnvSnapshot,
) -> EngineResult<ModeEntry> {
    let (inputs, inputs_before_resolve, config) = resolve_with_inputs(root, config, options, env);
    let (workspaces, workspace_diagnostics, workspace_discovery_ms) =
        collect_workspace_metadata(&config)?;
    Ok(ModeEntry {
        config,
        inputs,
        inputs_before_resolve,
        workspaces,
        workspace_diagnostics,
        workspace_discovery_ms,
    })
}

/// [`mode_entry`], with no workspaces when the workspace discovery fails.
pub(crate) fn mode_entry_lossy(
    root: &Path,
    config: FallowConfig,
    options: ResolveOptions,
    env: &EnvSnapshot,
) -> ModeEntry {
    let (inputs, inputs_before_resolve, config) = resolve_with_inputs(root, config, options, env);
    let (workspaces, workspace_diagnostics, workspace_discovery_ms) =
        collect_workspace_metadata(&config).unwrap_or_default();
    ModeEntry {
        config,
        inputs,
        inputs_before_resolve,
        workspaces,
        workspace_diagnostics,
        workspace_discovery_ms,
    }
}

fn resolve_with_inputs(
    root: &Path,
    config: FallowConfig,
    options: ResolveOptions,
    env: &EnvSnapshot,
) -> (ConfigInputs, ConfigInputsSnapshot, ResolvedConfig) {
    let inputs = ConfigInputs::new(root, &config);
    let inputs_before_resolve = inputs.snapshot();
    let resolved = resolve_with_env_from(config, root, options, env);
    (inputs, inputs_before_resolve, resolved)
}

/// Resolve the config for `root`, then apply the environment variables that
/// `env` gives.
pub(crate) fn resolve_with_env_from(
    config: FallowConfig,
    root: &Path,
    options: ResolveOptions,
    env: &EnvSnapshot,
) -> ResolvedConfig {
    let mut resolved = config.resolve(
        root.to_path_buf(),
        options.output,
        options.threads,
        options.no_cache,
        options.quiet,
        None,
    );
    apply_env_overrides_from(&mut resolved, env);
    resolved
}

/// The `FALLOW_MAX_FILE_SIZE` value in megabytes, when it is a valid number.
pub(crate) fn max_file_size_env(env: &EnvSnapshot) -> Option<u32> {
    env.get(EnvVar::MaxFileSize)
        .and_then(|raw| raw.to_str().and_then(|raw| raw.trim().parse::<u32>().ok()))
}

/// Apply the environment variables that every host honors. The CLI, the LSP,
/// the MCP server and the Node bindings all load config through these steps,
/// so a variable applied here has the same meaning on each surface.
pub(crate) fn apply_env_overrides_from(config: &mut ResolvedConfig, env: &EnvSnapshot) {
    if let Some(max_file_size_mb) = max_file_size_env(env) {
        config.max_file_size_bytes =
            fallow_config::resolve_max_file_size_bytes(Some(max_file_size_mb));
    }
    if let Some(dir) = env
        .get(EnvVar::CacheDir)
        .map(std::ffi::OsStr::to_os_string)
        .and_then(fallow_config::cache_dir_from_env_value)
    {
        config.override_cache_dir(dir);
    }
    if let Some(mb) = env
        .get(EnvVar::CacheMaxSize)
        .and_then(fallow_config::cache_max_size_from_env_value)
    {
        config.cache_max_size_mb = Some(mb);
    }
    config.apply_package_baselines_env(env.get(EnvVar::PackageBaselines));
}

/// Discover the workspaces of a resolved config.
///
/// # Errors
///
/// Returns an error when the root manifest cannot be parsed.
pub(crate) fn collect_workspace_metadata(
    config: &ResolvedConfig,
) -> EngineResult<WorkspaceMetadata> {
    let start = std::time::Instant::now();
    let (workspaces, diagnostics) =
        fallow_config::discover_workspaces_with_diagnostics_and_patterns(
            &config.root,
            &config.ignore_patterns,
            &config.workspace_patterns,
        )
        .map_err(|err| EngineError::new(err.to_string()))?;
    let diagnostics = with_undeclared_workspace_diagnostics(config, &workspaces, diagnostics);
    let elapsed_ms = start.elapsed().as_secs_f64() * 1000.0;
    Ok((workspaces, diagnostics, elapsed_ms))
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

/// Validate the config of a run of `command`.
///
/// - `plugin-check` reports each external plugin, also a plugin that fails
///   validation, so it validates only the boundaries and the rule packs.
/// - A coverage upload reads the project as the config file gives it, so it
///   validates nothing.
/// - Each other command validates the external plugins, the boundaries and
///   the rule packs.
pub(crate) fn validate_command_config(
    command: CommandKind,
    root: &Path,
    config: &FallowConfig,
) -> EngineResult<()> {
    match command {
        CommandKind::PluginCheck => validate_boundaries_and_rule_packs(root, config),
        CommandKind::CoverageUpload => Ok(()),
        _ => validate_config(root, config),
    }
}

/// Validate the external plugins, the boundaries and the rule packs.
pub(crate) fn validate_config(root: &Path, config: &FallowConfig) -> EngineResult<()> {
    fallow_config::discover_and_validate_external_plugins(root, &config.plugins)
        .map_err(|errors| joined_config_errors("invalid external plugin definition", &errors))?;
    validate_boundaries_and_rule_packs(root, config)
}

/// Validate the boundaries and the rule packs.
pub(crate) fn validate_boundaries_and_rule_packs(
    root: &Path,
    config: &FallowConfig,
) -> EngineResult<()> {
    config
        .validate_resolved_boundaries(root)
        .map_err(|errors| joined_config_errors("invalid boundary configuration", &errors))?;
    let packs = fallow_config::load_rule_packs(root, &config.rule_packs)
        .map_err(|errors| joined_config_errors("invalid rule pack", &errors))?;
    let zone_errors = fallow_config::validate_rule_pack_zones(
        root,
        &config.boundaries,
        &config.rule_packs,
        &packs,
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
