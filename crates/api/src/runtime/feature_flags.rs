use std::path::Path;
use std::time::Instant;

use fallow_engine::{AnalysisSession, ProjectConfig};
use fallow_output::{
    CHECK_SCHEMA_VERSION, FeatureFlagsOutputInput, build_feature_flags_output, feature_flags_meta,
    relative_to_diff_path,
};
use fallow_types::output_format::OutputFormat;
use fallow_types::results::FeatureFlag;

use crate::{
    FeatureFlagsOptions, FeatureFlagsProgrammaticOutput, ProgrammaticError,
    analysis_context::{
        ProgrammaticAnalysisContext, changed_files_for_run, resolve_programmatic_analysis_context,
    },
};

use super::{ProgrammaticResult, root_envelope_mode};

/// Run feature-flag analysis and return typed API output before JSON.
///
/// # Errors
///
/// Returns a structured programmatic error for invalid options, config load
/// failures, git changed-file failures, or analysis failures.
pub fn run_feature_flags(
    options: &FeatureFlagsOptions,
) -> ProgrammaticResult<FeatureFlagsProgrammaticOutput> {
    let resolved = resolve_programmatic_analysis_context(&options.analysis)?;
    resolved.install(|| run_feature_flags_inner(options, &resolved))
}

fn run_feature_flags_inner(
    options: &FeatureFlagsOptions,
    resolved: &ProgrammaticAnalysisContext,
) -> ProgrammaticResult<FeatureFlagsProgrammaticOutput> {
    let start = Instant::now();
    let session = load_feature_flags_session(resolved)?;
    let analysis = fallow_engine::analyze_feature_flags(session.config());
    if analysis.files_scanned == 0 {
        return Err(ProgrammaticError::new("no files discovered", 2)
            .with_code("FALLOW_NO_FILES_DISCOVERED")
            .with_context("feature-flags"));
    }

    let mut flags = analysis.flags;
    apply_feature_flags_scope(&mut flags, resolved, session.root())?;
    sort_and_limit_feature_flags(&mut flags, options.top);

    let output = build_feature_flags_output(FeatureFlagsOutputInput {
        schema_version: CHECK_SCHEMA_VERSION,
        version: env!("CARGO_PKG_VERSION").to_string(),
        elapsed: start.elapsed(),
        flags: &flags,
        root: session.root(),
        meta: resolved.explain_enabled().then(feature_flags_meta),
    });

    Ok(FeatureFlagsProgrammaticOutput {
        output,
        envelope_mode: root_envelope_mode(resolved.legacy_envelope),
        telemetry_analysis_run_id: None,
    })
}

fn load_feature_flags_session(
    resolved: &ProgrammaticAnalysisContext,
) -> ProgrammaticResult<AnalysisSession> {
    let project_config =
        fallow_engine::config_for_project(&resolved.root, resolved.config_path.as_deref())
            .map_err(|err| {
                ProgrammaticError::new(format!("failed to load config: {err}"), 2)
                    .with_code("FALLOW_CONFIG_LOAD_FAILED")
                    .with_context("analysis.configPath")
            })?;
    Ok(AnalysisSession::from_config(
        configure_project_for_feature_flags(project_config, resolved),
    ))
}

fn configure_project_for_feature_flags(
    mut project_config: ProjectConfig,
    resolved: &ProgrammaticAnalysisContext,
) -> ProjectConfig {
    project_config.config.output = OutputFormat::Json;
    project_config.config.no_cache = resolved.no_cache;
    project_config.config.threads = resolved.threads;
    project_config.config.production = resolved
        .production_override
        .unwrap_or(project_config.config.production);
    project_config
}

fn apply_feature_flags_scope(
    flags: &mut Vec<FeatureFlag>,
    resolved: &ProgrammaticAnalysisContext,
    root: &Path,
) -> ProgrammaticResult<()> {
    if let Some(workspace_roots) = resolved.workspace_roots.as_ref() {
        flags.retain(|flag| {
            workspace_roots
                .iter()
                .any(|root| flag.path.starts_with(root))
        });
    }
    if let Some(changed_files) = changed_files_for_run(resolved)? {
        flags.retain(|flag| changed_files.contains(&flag.path));
    }
    if let Some(diff) = resolved.diff.as_ref() {
        flags.retain(|flag| {
            relative_to_diff_path(&flag.path, root).is_none_or(|rel| diff.touches_file(&rel))
        });
    }
    Ok(())
}

fn sort_and_limit_feature_flags(flags: &mut Vec<FeatureFlag>, top: Option<usize>) {
    flags.sort_by(|a, b| {
        a.path
            .cmp(&b.path)
            .then(a.line.cmp(&b.line))
            .then(a.flag_name.cmp(&b.flag_name))
    });

    if let Some(top) = top {
        flags.truncate(top);
    }
}
