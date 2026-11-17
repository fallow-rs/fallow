use std::path::PathBuf;
use std::process::ExitCode;

use fallow_config::{OutputFormat, ProductionAnalysis, ProductionConfig};

use super::Cli;
use crate::cli_format::bool_from_env;
use crate::error::emit_input_error;

#[derive(Clone, Copy)]
pub struct ProductionModes {
    pub dead_code: bool,
    pub health: bool,
    pub dupes: bool,
}

impl ProductionModes {
    pub const fn for_analysis(self, analysis: ProductionAnalysis) -> bool {
        match analysis {
            ProductionAnalysis::DeadCode => self.dead_code,
            ProductionAnalysis::Health => self.health,
            ProductionAnalysis::Dupes => self.dupes,
        }
    }
}

/// Load the config of `root`, or the file at `config_path`. `None` when the
/// project has no config file.
fn load_config_file(
    root: &std::path::Path,
    config_path: Option<&PathBuf>,
    output: OutputFormat,
    allow_remote_extends: bool,
) -> Result<Option<fallow_config::FallowConfig>, ExitCode> {
    let load_options = fallow_config::ConfigLoadOptions {
        allow_remote_extends,
    };
    if let Some(path) = config_path {
        fallow_config::FallowConfig::load_with_options(path, load_options)
            .map(Some)
            .map_err(|e| {
                emit_input_error(
                    &format!("failed to load config '{}': {e}", path.display()),
                    output,
                )
            })
    } else {
        fallow_config::FallowConfig::find_and_load_with_options(root, load_options)
            .map(|found| found.map(|(config, _)| config))
            .map_err(|e| emit_input_error(&e, output))
    }
}

fn load_config_production(
    root: &std::path::Path,
    config_path: Option<&PathBuf>,
    output: OutputFormat,
    allow_remote_extends: bool,
) -> Result<ProductionConfig, ExitCode> {
    Ok(
        load_config_file(root, config_path, output, allow_remote_extends)?
            .map_or_else(ProductionConfig::default, |config| config.production),
    )
}

/// The decision of the project config for a duplication `request`. A project
/// without a config file has the default config. The config loads only when
/// the decision depends on it.
pub fn duplication_selection(
    cli: &Cli,
    root: &std::path::Path,
    output: OutputFormat,
    request: fallow_config::DuplicationRequest,
) -> Result<fallow_config::DuplicationSelection, ExitCode> {
    let duplicates = if request.reads_duplicates_enabled() {
        load_config_file(root, cli.config.as_ref(), output, cli.allow_remote_extends)?
            .map(|config| config.duplicates)
    } else {
        None
    };
    Ok(duplicates.unwrap_or_default().select(request))
}

pub fn resolve_production_modes(
    cli: &Cli,
    root: &std::path::Path,
    output: OutputFormat,
    production_dead_code: bool,
    production_health: bool,
    production_dupes: bool,
) -> Result<ProductionModes, ExitCode> {
    let config =
        load_config_production(root, cli.config.as_ref(), output, cli.allow_remote_extends)?;
    let env_global = bool_from_env("FALLOW_PRODUCTION");

    let resolve_one = |analysis: ProductionAnalysis, cli_specific: bool, env_name: &str| {
        if cli.production || cli_specific {
            true
        } else if cli.no_production {
            false
        } else if let Some(value) = bool_from_env(env_name) {
            value
        } else if let Some(value) = env_global {
            value
        } else {
            config.for_analysis(analysis)
        }
    };

    Ok(ProductionModes {
        dead_code: resolve_one(
            ProductionAnalysis::DeadCode,
            production_dead_code,
            "FALLOW_PRODUCTION_DEAD_CODE",
        ),
        health: resolve_one(
            ProductionAnalysis::Health,
            production_health,
            "FALLOW_PRODUCTION_HEALTH",
        ),
        dupes: resolve_one(
            ProductionAnalysis::Dupes,
            production_dupes,
            "FALLOW_PRODUCTION_DUPES",
        ),
    })
}

#[cfg(test)]
mod tests {
    /// `DuplicatesConfig::select` is the one rule that decides whether a run
    /// executes the duplication analysis. A command that reads the
    /// `duplicates.enabled` field itself can apply another precedence.
    #[test]
    fn only_the_config_crate_reads_the_duplicates_enabled_field() {
        use crate::architecture_boundaries::{
            read_source_without_line_comments, rust_sources_under,
        };
        let needle = concat!(".duplicates", ".enabled");
        let offenders: Vec<String> = rust_sources_under(["crates"])
            .into_iter()
            .filter(|source_path| !source_path.starts_with("crates/config/"))
            .filter(|source_path| {
                read_source_without_line_comments(source_path)
                    .unwrap_or_else(|error| panic!("read {source_path}: {error}"))
                    .split_whitespace()
                    .collect::<String>()
                    .contains(needle)
            })
            .collect();
        assert!(
            offenders.is_empty(),
            "decide the duplication analysis with DuplicatesConfig::select, not with the enabled field: {offenders:?}"
        );
    }
}
