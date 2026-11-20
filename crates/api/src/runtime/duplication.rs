use std::time::Instant;

use fallow_config::{DetectionMode, DuplicatesConfig, PlannedAnalysis};
use fallow_engine::{change_scope::ChangeScope, session::AnalysisSession};
use fallow_output::{
    DUPES_PROGRAMMATIC_SCHEMA_VERSION, DupesNextStepsInput, DupesOutput, DupesOutputInput,
    build_dupes_next_steps, build_dupes_output, dupes_meta,
};
use rustc_hash::FxHashSet;

use crate::{
    DupesReportPayload, DuplicationGroup, DuplicationMode, DuplicationOptions,
    DuplicationProgrammaticOutput,
    analysis_context::{
        ProgrammaticAnalysisContext, changed_files_for_run, resolve_programmatic_analysis_context,
        workspace_roots_for_session,
    },
    next_steps::{audit_changed_applicable, setup_pointer_applicable, suggestions_enabled},
    run::{ProgrammaticEntry, ResolvedRun, resolve_programmatic_run},
};

use super::ProgrammaticResult;

/// Run duplication analysis and return typed API output before serialization.
///
/// # Errors
///
/// Returns a structured programmatic error for invalid options, config load
/// failures, or git changed-file failures.
pub fn run_duplication(
    options: &DuplicationOptions,
) -> ProgrammaticResult<DuplicationProgrammaticOutput> {
    let resolved = resolve_programmatic_analysis_context(&options.analysis)?;
    resolved.install(|| {
        resolved.ensure_not_cancelled("config load and file discovery")?;
        let run = resolve_programmatic_run(&options.analysis, &ProgrammaticEntry::Duplication)?;
        run_duplication_in_context(options, &resolved, &run)
    })
}

/// Run duplication analysis in a context and a run that the caller resolved.
/// Audit uses this to hand the change scope of every section to itself.
pub(super) fn run_duplication_in_context(
    options: &DuplicationOptions,
    resolved: &ProgrammaticAnalysisContext,
    run: &ResolvedRun,
) -> ProgrammaticResult<DuplicationProgrammaticOutput> {
    resolved.install(|| run_duplication_inner(options, resolved, run))
}

fn run_duplication_inner(
    options: &DuplicationOptions,
    resolved: &ProgrammaticAnalysisContext,
    run: &ResolvedRun,
) -> ProgrammaticResult<DuplicationProgrammaticOutput> {
    let start = Instant::now();
    resolved.ensure_not_cancelled("config load and file discovery")?;
    let session = load_duplication_session(options, resolved, run)?;
    super::dead_code::resolve_package_map_before_analysis(resolved, &session)?;
    run_duplication_with_session(options, resolved, &session, None, start)
}

pub(super) fn run_duplication_with_session(
    options: &DuplicationOptions,
    resolved: &ProgrammaticAnalysisContext,
    session: &AnalysisSession,
    changed_files: Option<&FxHashSet<std::path::PathBuf>>,
    start: Instant,
) -> ProgrammaticResult<DuplicationProgrammaticOutput> {
    resolved.ensure_not_cancelled("duplication detection")?;
    let dupes_config = build_dupes_config(options, &session.config().duplicates);
    let resolved_changed_files = if changed_files.is_some() {
        None
    } else {
        changed_files_for_run(resolved)?
    };
    let global_files = changed_files.or(resolved_changed_files.as_ref());
    let change_scope =
        resolved.change_scope(global_files, session.config(), session.workspaces())?;
    let cache_dir = (!resolved.no_cache).then_some(session.config().cache_dir.as_path());
    let report = if let Some(changed_files) = global_files {
        resolved
            .measure_changed_since_scope(session.files().iter().map(|file| file.path.as_path()));
        let changed_files = changed_files.iter().cloned().collect::<Vec<_>>();
        session
            .find_duplicates_touching_files_with_defaults(&dupes_config, &changed_files, cache_dir)
            .report
    } else {
        session
            .find_duplicates_with_defaults(&dupes_config, cache_dir)
            .report
    };

    // Duplication detection cannot fail, so a token set while it ran has to be
    // reported here rather than dressed up as a complete report.
    resolved.ensure_not_cancelled("the duplication report")?;
    run_duplication_report_with_session(options, resolved, session, report, &change_scope, start)
}

pub(super) fn run_duplication_report_with_session(
    options: &DuplicationOptions,
    resolved: &ProgrammaticAnalysisContext,
    session: &AnalysisSession,
    mut report: fallow_engine::duplicates::DuplicationReport,
    change_scope: &ChangeScope,
    start: Instant,
) -> ProgrammaticResult<DuplicationProgrammaticOutput> {
    let dupes_config = build_dupes_config(options, &session.config().duplicates);
    let workspace_roots = workspace_roots_for_session(resolved, session.workspaces())?;
    fallow_engine::duplicates::apply_scope(
        &mut report,
        &fallow_engine::duplicates::DuplicationScope {
            changes: Some(change_scope),
            diff: resolved.diff.as_ref(),
            workspace_roots: workspace_roots.as_ref(),
        },
        session.root(),
    );
    if let Some(top) = options.top {
        fallow_engine::duplicates::apply_top(&mut report, top, session.root());
    }

    let root = session.root();
    let payload = DupesReportPayload::from_report_with_fragments(
        &report,
        options.include_fragments.unwrap_or(true),
    );
    let clone_fingerprints = payload
        .clone_groups
        .iter()
        .map(|group| group.fingerprint.as_str())
        .collect::<Vec<_>>();
    let next_steps = build_dupes_next_steps(DupesNextStepsInput {
        suggestions_enabled: suggestions_enabled(),
        clone_fingerprints: &clone_fingerprints,
        offer_setup: setup_pointer_applicable(root),
        impact_digest: None,
        audit_changed: audit_changed_applicable(root),
        baseline_recheck: None,
    });
    let mut output: DupesOutput<DupesReportPayload, DuplicationGroup> =
        build_dupes_output(DupesOutputInput {
            gate_outcomes: None,
            request_outcomes: resolved.request_outcomes(),
            schema_version: DUPES_PROGRAMMATIC_SCHEMA_VERSION,
            version: env!("CARGO_PKG_VERSION").to_string(),
            elapsed: start.elapsed(),
            report: payload,
            clone_groups_shown: report.clone_groups_shown(),
            clone_groups_omitted: report.clone_groups_omitted(),
            clone_families_shown: report.clone_families_shown(),
            clone_families_omitted: report.clone_families_omitted(),
            grouped_by: None,
            total_issues: None,
            groups: None,
            // The programmatic duplication route loads no baseline, so there is
            // no staleness to report.
            baseline_staleness: None,
            meta: resolved.explain_enabled().then(dupes_meta),
            workspace_diagnostics: session.workspace_diagnostics().to_vec(),
            next_steps,
        });
    output.package_baselines = change_scope.package_baselines();
    Ok(DuplicationProgrammaticOutput {
        output,
        root: session.root().to_path_buf(),
        threshold: dupes_config.threshold,
        telemetry_analysis_run_id: None,
    })
}

/// Open the duplication session of `run`, with the detector options of
/// `options`.
pub(super) fn load_duplication_session(
    options: &DuplicationOptions,
    resolved: &ProgrammaticAnalysisContext,
    run: &ResolvedRun,
) -> ProgrammaticResult<AnalysisSession> {
    let duplication = crate::run::runnable(run, PlannedAnalysis::Duplication)?;
    let session = crate::run::open_session_adjusted(&duplication, |config| {
        config.duplicates = build_dupes_config(options, &config.duplicates);
    })?;
    Ok(super::dead_code::attach_cancellation(session, resolved))
}

/// The duplication config of a programmatic run: each option over the
/// config, through [`DuplicatesConfig::merged_for_selected_run`].
pub(super) fn build_dupes_config(
    options: &DuplicationOptions,
    config: &DuplicatesConfig,
) -> DuplicatesConfig {
    config.merged_for_selected_run(&fallow_config::DupesInputs {
        mode: options.mode.map(duplication_mode_to_config),
        near: options.near,
        min_tokens: options.min_tokens,
        min_lines: options.min_lines,
        min_occurrences: options.min_occurrences,
        threshold: options.threshold,
        skip_local: options.skip_local,
        cross_language: options.cross_language,
        ignore_imports: options.ignore_imports,
        ignore_symlinks: options.ignore_symlinks,
    })
}

const fn duplication_mode_to_config(mode: DuplicationMode) -> DetectionMode {
    match mode {
        DuplicationMode::Strict => DetectionMode::Strict,
        DuplicationMode::Mild => DetectionMode::Mild,
        DuplicationMode::Weak => DetectionMode::Weak,
        DuplicationMode::Semantic => DetectionMode::Semantic,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn duplication_options_override_near_and_preserve_reviewed_clones() {
        let config = DuplicatesConfig {
            near: true,
            ignored_clones: vec!["dup:12345678:2".to_string()],
            ..DuplicatesConfig::default()
        };
        let options = DuplicationOptions {
            near: Some(false),
            ..DuplicationOptions::default()
        };

        let merged = build_dupes_config(&options, &config);

        assert!(!merged.near);
        assert_eq!(merged.ignored_clones, config.ignored_clones);
    }
}
