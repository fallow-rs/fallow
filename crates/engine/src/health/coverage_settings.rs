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
    file_paths: &rustc_hash::FxHashMap<crate::discover::FileId, &std::path::PathBuf>,
) -> Result<HealthCoverageSettings, HealthError> {
    let config_coverage_enabled = config.rules.coverage_gaps != fallow_config::Severity::Off;
    let report_coverage_gaps =
        opts.coverage_gaps || (opts.config_activates_coverage_gaps && config_coverage_enabled);
    let enforce_coverage_gaps = opts.enforce_coverage_gap_gate
        && config.rules.coverage_gaps == fallow_config::Severity::Error;
    let istanbul_coverage = load_health_coverage(opts, config, file_paths)?;

    Ok(HealthCoverageSettings {
        report_coverage_gaps,
        enforce_coverage_gaps,
        istanbul_coverage,
    })
}

fn load_health_coverage(
    opts: &HealthExecutionOptions<'_>,
    config: &ResolvedConfig,
    file_paths: &rustc_hash::FxHashMap<crate::discover::FileId, &std::path::PathBuf>,
) -> Result<Option<scoring::IstanbulCoverage>, HealthError> {
    if let Some(coverage_path) = opts.coverage_inputs.coverage {
        let discovered_sources = (!opts.coverage_inputs.coverage_relocated)
            .then(|| discovered_regular_sources(file_paths, &config.root));
        return match scoring::load_istanbul_coverage_for_sources(
            coverage_path,
            opts.coverage_inputs.coverage_root,
            Some(&config.root),
            discovered_sources.as_ref(),
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
    let discovered_sources = (!opts.coverage_inputs.coverage_relocated)
        .then(|| discovered_regular_sources(file_paths, &config.root));
    // Quiet-gated like every other health note, rather than gated on `CI`
    // being set. The old guard printed the line only where stderr is discarded
    // by the consumer and hid it from the human who could act on it; the
    // machine surface is the diagnostic below (issue #2689).
    if !opts.quiet {
        eprintln!(
            "note: using auto-detected coverage at {}; pass --coverage explicitly for deterministic CI scores",
            auto_path.display()
        );
    }
    super::diagnostics::record_health_diagnostic(
        &config.root,
        Some(&auto_path),
        fallow_types::workspace::WorkspaceDiagnosticKind::CoverageAutoDetected,
    );
    Ok(scoring::load_istanbul_coverage_for_sources(
        &auto_path,
        opts.coverage_inputs.coverage_root,
        Some(&config.root),
        discovered_sources.as_ref(),
        opts.coverage_inputs.coverage_relocated,
    )
    .ok())
}

fn discovered_regular_sources(
    file_paths: &rustc_hash::FxHashMap<crate::discover::FileId, &std::path::PathBuf>,
    project_root: &std::path::Path,
) -> rustc_hash::FxHashSet<std::path::PathBuf> {
    let Ok(canonical_root) = dunce::canonicalize(project_root) else {
        return rustc_hash::FxHashSet::default();
    };
    file_paths
        .values()
        .filter_map(|path| {
            let canonical = dunce::canonicalize(path).ok()?;
            if !canonical.starts_with(&canonical_root) {
                return None;
            }
            std::fs::metadata(&canonical)
                .ok()?
                .is_file()
                .then_some(canonical)
        })
        .collect()
}
