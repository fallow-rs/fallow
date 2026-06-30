use std::path::PathBuf;
use std::process::ExitCode;

use fallow_config::{OutputFormat, ProductionAnalysis, ProductionConfig};

use super::Cli;
use crate::cli_format::bool_from_env;
use crate::error::emit_error;

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

fn load_config_production(
    root: &std::path::Path,
    config_path: Option<&PathBuf>,
    output: OutputFormat,
) -> Result<ProductionConfig, ExitCode> {
    let loaded = if let Some(path) = config_path {
        fallow_config::FallowConfig::load(path)
            .map(Some)
            .map_err(|e| {
                emit_error(
                    &format!("failed to load config '{}': {e}", path.display()),
                    2,
                    output,
                )
            })?
    } else {
        fallow_config::FallowConfig::find_and_load(root)
            .map(|found| found.map(|(config, _)| config))
            .map_err(|e| emit_error(&e, 2, output))?
    };

    Ok(match loaded {
        Some(config) => config.production,
        None => ProductionConfig::default(),
    })
}

pub fn resolve_production_modes(
    cli: &Cli,
    root: &std::path::Path,
    output: OutputFormat,
    production_dead_code: bool,
    production_health: bool,
    production_dupes: bool,
) -> Result<ProductionModes, ExitCode> {
    let config = load_config_production(root, cli.config.as_ref(), output)?;
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
