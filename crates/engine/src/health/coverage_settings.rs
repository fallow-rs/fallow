//! Health coverage input resolution.

#![allow(
    clippy::print_stderr,
    reason = "human stderr note for auto-detected coverage is part of the CLI health contract"
)]

use fallow_config::ResolvedConfig;

use super::{HealthError, HealthExecutionOptions, scoring};

pub(super) struct HealthCoverageSettings {
    pub(super) report_coverage_gaps: bool,
    pub(super) enforce_coverage_gaps: bool,
    pub(super) istanbul_coverage: Option<scoring::IstanbulCoverage>,
}

pub(super) fn prepare_health_coverage_settings(
    opts: &HealthExecutionOptions<'_>,
    config: &ResolvedConfig,
) -> Result<HealthCoverageSettings, HealthError> {
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
) -> Result<Option<scoring::IstanbulCoverage>, HealthError> {
    if let Some(coverage_path) = opts.coverage_inputs.coverage {
        return match scoring::load_istanbul_coverage(
            coverage_path,
            opts.coverage_inputs.coverage_root,
            Some(&config.root),
            opts.coverage_inputs.coverage_relocated,
        ) {
            Ok(coverage) => Ok(Some(coverage)),
            // The relocated (audit base-worktree) pass may have been handed a
            // file the head pass only auto-detected and loads leniently below;
            // failing hard here would turn lenient auto-detection into an
            // audit error. Explicit user coverage still fails loudly: the
            // head pass loads the same file strictly.
            Err(_) if opts.coverage_inputs.coverage_relocated => Ok(None),
            Err(e) => Err(HealthError::message(format!("coverage: {e}"), 2)),
        };
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
        opts.coverage_inputs.coverage_relocated,
    )
    .ok())
}
