//! Config resolution for the tests of this crate. Hosts resolve config through
//! `fallow_engine::project_config`.

use std::path::Path;

use fallow_config::ResolvedConfig;

use crate::errors::FallowError;

/// Resolve the analysis config for a project, mirroring the CLI's `--config`
/// behavior when `config_path` is provided.
///
/// # Errors
///
/// Returns an error when an explicit config cannot be loaded or automatic
/// config discovery finds an invalid config.
fn config_for_project(
    root: &Path,
    config_path: Option<&Path>,
) -> Result<(ResolvedConfig, Option<std::path::PathBuf>), FallowError> {
    let user_config = if let Some(path) = config_path {
        Some((
            fallow_config::FallowConfig::load(path)
                .map_err(|e| FallowError::config(format!("{e:#}")))?,
            path.to_path_buf(),
        ))
    } else {
        fallow_config::FallowConfig::find_and_load(root).map_err(FallowError::config)?
    };

    let config = match user_config {
        Some((config, path)) => resolve_user_config(config, path, root)?,
        None => (
            fallow_config::FallowConfig::default().resolve(
                root.to_path_buf(),
                fallow_config::OutputFormat::Human,
                num_cpus(),
                false,
                true,
                None,
            ),
            None,
        ),
    };

    Ok(config)
}

/// Flatten the dead-code production flag, validate boundaries and rule packs,
/// then resolve a user-supplied config.
fn resolve_user_config(
    mut config: fallow_config::FallowConfig,
    path: std::path::PathBuf,
    root: &Path,
) -> Result<(ResolvedConfig, Option<std::path::PathBuf>), FallowError> {
    let dead_code_production = config
        .production
        .for_analysis(fallow_config::ProductionAnalysis::DeadCode);
    config.production = config.production.for_run(dead_code_production);
    config
        .validate_resolved_boundaries(root)
        .map_err(|errors| {
            let joined = errors
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join("\n  - ");
            FallowError::config(format!("invalid boundary configuration:\n  - {joined}"))
        })?;
    let packs = fallow_config::load_rule_packs(root, &config.rule_packs).map_err(|errors| {
        let joined = errors
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n  - ");
        FallowError::config(format!("invalid rule pack:\n  - {joined}"))
    })?;
    let zone_errors = fallow_config::validate_rule_pack_zones(
        root,
        &config.boundaries,
        &config.rule_packs,
        &packs,
    );
    if !zone_errors.is_empty() {
        let joined = zone_errors
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n  - ");
        return Err(FallowError::config(format!(
            "invalid rule pack:\n  - {joined}"
        )));
    }
    Ok((
        config.resolve(
            root.to_path_buf(),
            fallow_config::OutputFormat::Human,
            num_cpus(),
            false,
            true,
            None,
        ),
        Some(path),
    ))
}

/// The config of a project root for a dead-code test. Defaults apply when the
/// project config does not load.
///
/// When the loaded config uses the per-analysis production form
/// (`production: { deadCode: true, ... }`), the production flag is flattened
/// to the dead-code analysis with `for_run`. Otherwise
/// `ResolvedConfig::resolve` calls `.global()`, which is true for the
/// per-analysis variant only when every analysis is on, and the
/// production-mode rule overrides (`unused_dev_dependencies: off`, etc.) plus
/// `resolved.production = true` do not apply.
pub fn default_config(root: &Path) -> ResolvedConfig {
    config_for_project(root, None).map_or_else(
        |_| {
            fallow_config::FallowConfig::default().resolve(
                root.to_path_buf(),
                fallow_config::OutputFormat::Human,
                num_cpus(),
                false,
                true,
                None,
            )
        },
        |(config, _)| config,
    )
}

fn num_cpus() -> usize {
    std::thread::available_parallelism().map_or(4, std::num::NonZeroUsize::get)
}
