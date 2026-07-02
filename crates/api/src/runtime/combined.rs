use std::time::Instant;

use fallow_output::{CombinedNextStepsInput, build_combined_next_steps};

use crate::{
    CombinedOptions, CombinedProgrammaticOutput, ComplexityOptions, DeadCodeFilters,
    DeadCodeOptions, DuplicationOptions, ProgrammaticError,
    analysis_context::{changed_files_for_run, resolve_programmatic_analysis_context},
    next_steps::{default_workspace_ref, setup_pointer_applicable, suggestions_enabled},
};

use super::{ProgrammaticResult, root_envelope_mode, run_health_with_session_artifacts};

/// Run bare combined analysis through one programmatic analysis session.
///
/// # Errors
///
/// Returns a structured programmatic error for invalid options, config load
/// failures, or analysis failures.
pub fn run_combined(options: &CombinedOptions) -> ProgrammaticResult<CombinedProgrammaticOutput> {
    if !(options.dead_code || options.duplication || options.health) {
        return Err(ProgrammaticError::new(
            "combined analysis requires at least one enabled section",
            2,
        )
        .with_code("FALLOW_COMBINED_EMPTY")
        .with_context("combined"));
    }

    let start = Instant::now();
    let resolved = resolve_programmatic_analysis_context(&options.analysis)?;
    resolved.install(|| {
        let dead_code_options = combined_dead_code_options(options);
        let duplication_options = combined_duplication_options(options);
        let health_options = combined_health_options(options);
        let changed_files = changed_files_for_run(&resolved)?;
        let changed_files_ref = changed_files.as_ref();
        let session = super::dead_code::load_dead_code_session(&dead_code_options, &resolved)?;

        let dead_code = if options.dead_code {
            Some(super::dead_code::run_dead_code_with_session(
                &dead_code_options,
                &resolved,
                &session,
                changed_files_ref,
                |_| {},
                Instant::now(),
            )?)
        } else {
            None
        };

        let duplication = if options.duplication {
            Some(super::duplication::run_duplication_with_session(
                &duplication_options,
                &resolved,
                &session,
                changed_files_ref,
                Instant::now(),
            )?)
        } else {
            None
        };

        let health = if options.health {
            Some(run_health_with_session_artifacts(
                &health_options,
                &resolved,
                &session,
                changed_files_ref,
                None,
            )?)
        } else {
            None
        };

        let root = session.root().to_path_buf();
        let next_steps = combined_next_steps(
            dead_code.as_ref(),
            duplication.as_ref(),
            health.as_ref(),
            &root,
        );

        Ok(CombinedProgrammaticOutput {
            dead_code,
            duplication,
            health,
            root,
            elapsed: start.elapsed(),
            explain: options.analysis.explain,
            next_steps,
            envelope_mode: root_envelope_mode(),
            telemetry_analysis_run_id: None,
        })
    })
}

fn combined_dead_code_options(options: &CombinedOptions) -> DeadCodeOptions {
    DeadCodeOptions {
        analysis: options.analysis.clone(),
        filters: DeadCodeFilters::default(),
        files: Vec::new(),
        include_entry_exports: options.include_entry_exports,
    }
}

fn combined_duplication_options(options: &CombinedOptions) -> DuplicationOptions {
    let mut duplication = options.duplication_options.clone();
    duplication.analysis = options.analysis.clone();
    duplication
}

fn combined_health_options(options: &CombinedOptions) -> ComplexityOptions {
    let mut health = options.health_options.clone();
    health.analysis = options.analysis.clone();
    health
}

fn combined_next_steps(
    dead_code: Option<&crate::DeadCodeProgrammaticOutput>,
    duplication: Option<&crate::DuplicationProgrammaticOutput>,
    health: Option<&crate::HealthProgrammaticOutput>,
    root: &std::path::Path,
) -> Vec<fallow_types::output::NextStep> {
    let clone_fingerprints = duplication
        .map(|duplication| {
            duplication
                .output
                .report
                .clone_groups
                .iter()
                .map(|group| group.fingerprint.as_str())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let audit_changed = fallow_engine::churn::is_git_repo(root);
    let workspace_ref = audit_changed.then(|| default_workspace_ref(root)).flatten();
    build_combined_next_steps(&CombinedNextStepsInput {
        suggestions_enabled: suggestions_enabled(),
        has_dead_code_findings: dead_code
            .is_some_and(|dead_code| dead_code.output.results.total_issues() > 0),
        trace_unused_export: dead_code.and_then(|dead_code| {
            fallow_output::trace_unused_export_input(&dead_code.output.results, root)
        }),
        workspace_ref: workspace_ref.as_deref(),
        clone_fingerprints: &clone_fingerprints,
        has_complexity_findings: health.is_some_and(|health| !health.report.findings.is_empty()),
        offer_setup: setup_pointer_applicable(root),
        impact_digest: None,
        audit_changed,
    })
}
