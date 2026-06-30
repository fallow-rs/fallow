use std::time::Instant;

use fallow_config::{DetectionMode, DuplicatesConfig};
use fallow_engine::{AnalysisSession, ProjectConfig};
use fallow_output::{
    DupesNextStepsInput, DupesOutput, DupesOutputInput, build_dupes_next_steps, build_dupes_output,
    dupes_meta,
};
use fallow_types::output_format::OutputFormat;

use crate::{
    DupesReportPayload, DuplicationGroup, DuplicationMode, DuplicationOptions,
    DuplicationProgrammaticOutput, ProgrammaticError,
    analysis_context::{
        ProgrammaticAnalysisContext, changed_files_for_run, resolve_programmatic_analysis_context,
    },
    duplication_filters::{apply_top, filter_by_diff, filter_by_workspaces},
    next_steps::{setup_pointer_applicable, suggestions_enabled},
};

use super::{ProgrammaticResult, root_envelope_mode};

pub(super) const SCHEMA_VERSION: u32 = 1;

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
    resolved.install(|| run_duplication_inner(options, &resolved))
}

fn run_duplication_inner(
    options: &DuplicationOptions,
    resolved: &ProgrammaticAnalysisContext,
) -> ProgrammaticResult<DuplicationProgrammaticOutput> {
    let start = Instant::now();
    let session = load_duplication_session(options, resolved)?;
    let dupes_config = build_dupes_config(options, &session.config().duplicates);
    let changed_files = changed_files_for_run(resolved)?;
    let cache_dir = (!resolved.no_cache).then_some(session.config().cache_dir.as_path());
    let mut report = if let Some(changed_files) = changed_files.as_ref() {
        let changed_files = changed_files.iter().cloned().collect::<Vec<_>>();
        session
            .find_duplicates_touching_files_with_defaults(&dupes_config, &changed_files, cache_dir)
            .report
    } else {
        session
            .find_duplicates_with_defaults(&dupes_config, cache_dir)
            .report
    };

    if let Some(diff) = resolved.diff.as_ref() {
        filter_by_diff(&mut report, diff, session.root());
    }
    if let Some(workspace_roots) = resolved.workspace_roots.as_ref() {
        filter_by_workspaces(&mut report, workspace_roots, session.root());
    }
    if let Some(top) = options.top {
        apply_top(&mut report, top, session.root());
    }

    let root = session.root();
    let payload = DupesReportPayload::from_report(&report);
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
        audit_changed: fallow_engine::is_git_repo(root),
    });
    let output: DupesOutput<DupesReportPayload, DuplicationGroup> =
        build_dupes_output(DupesOutputInput {
            schema_version: SCHEMA_VERSION,
            version: env!("CARGO_PKG_VERSION").to_string(),
            elapsed: start.elapsed(),
            report: payload,
            grouped_by: None,
            total_issues: None,
            groups: None,
            meta: resolved.explain_enabled().then(dupes_meta),
            workspace_diagnostics: session.workspace_diagnostics().to_vec(),
            next_steps,
        });
    Ok(DuplicationProgrammaticOutput {
        output,
        root: session.root().to_path_buf(),
        envelope_mode: root_envelope_mode(resolved.legacy_envelope),
        telemetry_analysis_run_id: None,
    })
}

pub(super) fn load_duplication_session(
    options: &DuplicationOptions,
    resolved: &ProgrammaticAnalysisContext,
) -> ProgrammaticResult<AnalysisSession> {
    let project_config =
        fallow_engine::config_for_project(&resolved.root, resolved.config_path.as_deref())
            .map_err(|err| {
                ProgrammaticError::new(format!("failed to load config: {err}"), 2)
                    .with_code("FALLOW_CONFIG_LOAD_FAILED")
                    .with_context("analysis.configPath")
            })?;
    let project_config = configure_project_for_duplication(project_config, options, resolved);
    Ok(AnalysisSession::from_config(project_config))
}

fn configure_project_for_duplication(
    mut project_config: ProjectConfig,
    options: &DuplicationOptions,
    resolved: &ProgrammaticAnalysisContext,
) -> ProjectConfig {
    let production = resolved
        .production_override
        .unwrap_or(project_config.config.production);
    project_config.config.production = production;
    project_config.config.output = OutputFormat::Json;
    project_config.config.threads = resolved.threads;
    project_config.config.no_cache = resolved.no_cache;
    project_config.config.duplicates =
        build_dupes_config(options, &project_config.config.duplicates);
    project_config
}

pub(super) fn build_dupes_config(
    options: &DuplicationOptions,
    config: &DuplicatesConfig,
) -> DuplicatesConfig {
    DuplicatesConfig {
        enabled: true,
        mode: options.mode.map_or(config.mode, duplication_mode_to_config),
        min_tokens: options.min_tokens.unwrap_or(config.min_tokens),
        min_lines: options.min_lines.unwrap_or(config.min_lines),
        min_occurrences: options.min_occurrences.unwrap_or(config.min_occurrences),
        threshold: options.threshold.unwrap_or(config.threshold),
        ignore: config.ignore.clone(),
        ignore_defaults: config.ignore_defaults,
        skip_local: options.skip_local.unwrap_or(config.skip_local),
        cross_language: options.cross_language.unwrap_or(config.cross_language),
        ignore_imports: options.ignore_imports.unwrap_or(config.ignore_imports),
        normalization: config.normalization.clone(),
        min_corpus_size_for_shingle_filter: config.min_corpus_size_for_shingle_filter,
        min_corpus_size_for_token_cache: config.min_corpus_size_for_token_cache,
    }
}

const fn duplication_mode_to_config(mode: DuplicationMode) -> DetectionMode {
    match mode {
        DuplicationMode::Strict => DetectionMode::Strict,
        DuplicationMode::Mild => DetectionMode::Mild,
        DuplicationMode::Weak => DetectionMode::Weak,
        DuplicationMode::Semantic => DetectionMode::Semantic,
    }
}
