//! Health coverage input resolution.

#![allow(
    clippy::print_stderr,
    reason = "human stderr note for auto-detected coverage is part of the CLI health contract"
)]

use std::process::ExitCode;

use fallow_config::ResolvedConfig;

use crate::error::emit_error;

use super::{HealthExecutionOptions, scoring};

pub(super) struct HealthCoverageSettings {
    pub(super) report_coverage_gaps: bool,
    pub(super) enforce_coverage_gaps: bool,
    pub(super) istanbul_coverage: Option<scoring::IstanbulCoverage>,
}

pub(super) fn prepare_health_coverage_settings(
    opts: &HealthExecutionOptions<'_>,
    config: &ResolvedConfig,
) -> Result<HealthCoverageSettings, ExitCode> {
    let config_coverage_enabled = config.rules.coverage_gaps != fallow_config::Severity::Off;
    let report_coverage_gaps =
        opts.coverage_gaps || (opts.config_activates_coverage_gaps && config_coverage_enabled);
    let enforce_coverage_gaps = opts.enforce_coverage_gap_gate
        && config.rules.coverage_gaps == fallow_config::Severity::Error;
    let istanbul_coverage = load_health_coverage(opts, config)?;

    Ok(HealthCoverageSettings {
        report_coverage_gaps,
        enforce_coverage_gaps,
        istanbul_coverage,
    })
}

fn load_health_coverage(
    opts: &HealthExecutionOptions<'_>,
    config: &ResolvedConfig,
) -> Result<Option<scoring::IstanbulCoverage>, ExitCode> {
    if let Some(coverage_path) = opts.coverage_inputs.coverage {
        return scoring::load_istanbul_coverage(
            coverage_path,
            opts.coverage_inputs.coverage_root,
            Some(&config.root),
        )
        .map(Some)
        .map_err(|e| {
            emit_error(&format!("coverage: {e}"), 2, opts.output);
            ExitCode::from(2)
        });
    }

    let Some(auto_path) = scoring::auto_detect_coverage(&config.root) else {
        return Ok(None);
    };
    if std::env::var("CI").is_ok_and(|v| !v.is_empty()) {
        eprintln!(
            "note: using auto-detected coverage at {}; pass --coverage explicitly for deterministic CI scores",
            auto_path.display()
        );
    }
    Ok(scoring::load_istanbul_coverage(
        &auto_path,
        opts.coverage_inputs.coverage_root,
        Some(&config.root),
    )
    .ok())
}
