use std::path::Path;
use std::time::Instant;

use fallow_engine::flag_report::{RetirementRequest, build_retirement_report};
use fallow_engine::flag_retirement::{RetirementFacts, RetirementOptions};
use fallow_engine::flag_vendor::{VendorExport, load_flag_state};

use fallow_engine::{project_config::ProjectConfig, session::AnalysisSession};
use fallow_output::{
    DiffIndex, FEATURE_FLAGS_SCHEMA_VERSION, FeatureFlagsOutputInput, build_feature_flags_output,
    feature_flags_meta,
};
use fallow_types::output_format::OutputFormat;
use fallow_types::results::FeatureFlag;

use crate::{
    FeatureFlagsOptions, FeatureFlagsProgrammaticOutput, ProgrammaticError,
    analysis_context::{
        ProgrammaticAnalysisContext, changed_files_for_run,
        resolve_programmatic_analysis_context_deferred_workspace, workspace_roots_for_session,
    },
};

use super::ProgrammaticResult;

/// Run feature-flag analysis and return typed API output before JSON.
///
/// # Errors
///
/// Returns a structured programmatic error for invalid options, config load
/// failures, git changed-file failures, or analysis failures, and
/// `FALLOW_CANCELLED` when the caller's cancellation token is set. The scan
/// observes the token at its entry, on both sides of the parse loop, and at
/// the stage boundaries of the dead-code correlation behind it.
pub fn run_feature_flags(
    options: &FeatureFlagsOptions,
) -> ProgrammaticResult<FeatureFlagsProgrammaticOutput> {
    let resolved = resolve_programmatic_analysis_context_deferred_workspace(&options.analysis)?;
    resolved.install(|| run_feature_flags_inner(options, &resolved))
}

fn run_feature_flags_inner(
    options: &FeatureFlagsOptions,
    resolved: &ProgrammaticAnalysisContext,
) -> ProgrammaticResult<FeatureFlagsProgrammaticOutput> {
    let start = Instant::now();
    let vendor_export = load_vendor_export(options, &resolved.root)?;
    resolved.ensure_not_cancelled("config load and file discovery")?;
    let session = load_feature_flags_session(resolved)?;
    let scan = if options.retirement.is_some() {
        fallow_engine::flags::analyze_feature_flags_for_retirement(&session)
    } else {
        fallow_engine::flags::analyze_feature_flags_with_session(&session)
            .map(|analysis| (analysis, RetirementFacts::default()))
    };
    let (analysis, retirement_facts) = scan.map_err(|err| {
        super::dead_code::map_engine_error(
            &err,
            "feature-flag analysis failed",
            "FALLOW_FEATURE_FLAGS_FAILED",
            "feature-flags",
        )
    })?;
    if analysis.files_scanned == 0 {
        return Err(ProgrammaticError::new("no files discovered", 2)
            .with_code("FALLOW_NO_FILES_DISCOVERED")
            .with_context("feature-flags"));
    }

    let scope = feature_flags_scope(resolved, &session)?;
    let all_flags = if options.retirement.is_some() {
        analysis.flags.clone()
    } else {
        Vec::new()
    };
    let mut flags = analysis.flags;
    flags.retain(|flag| scope.contains(&flag.path, session.root()));
    let mut workspace_diagnostics = session.current_workspace_diagnostics();
    let retirement = options.retirement.as_ref().map(|retirement| {
        let build = build_retirement_report(RetirementRequest {
            root: session.root(),
            workspaces: session.workspaces(),
            sites: retirement_facts.sites_for(&all_flags),
            in_scope: &|path| scope.contains(path, session.root()),
            whole_project: scope.is_whole_project(),
            age_mode: retirement.flag_age,
            cache_dir: (!resolved.no_cache).then_some(session.config().cache_dir.as_path()),
            progress: None,
            vendor_export: vendor_export.as_ref(),
            vendor_key_prefix: session.config().flags.vendor_key_prefix.as_deref(),
            max_flag_age: retirement.max_flag_age,
            options: RetirementOptions {
                sort: retirement.sort,
                min_age_days: retirement.min_age_days,
                reasons: retirement.reasons.clone(),
                top: options.top,
            },
        });
        workspace_diagnostics.extend(build.diagnostics.into_iter().map(|kind| {
            fallow_config::WorkspaceDiagnostic::new(
                session.root(),
                session.root().to_path_buf(),
                kind,
            )
        }));
        build.report
    });
    sort_and_limit_feature_flags(&mut flags, options.top);

    let output = build_feature_flags_output(FeatureFlagsOutputInput {
        schema_version: FEATURE_FLAGS_SCHEMA_VERSION,
        version: env!("CARGO_PKG_VERSION").to_string(),
        elapsed: start.elapsed(),
        flags: &flags,
        root: session.root(),
        // Read live, like the dead-code route: the parse stage records
        // `source-read-failure` and `source-parse-degraded` after the session
        // captured its walk snapshot, and both are reasons a flag is missing.
        workspace_diagnostics,
        // The diff this route resolved and applied above, or the reason it
        // stood down. This route filters flags by the diff, unlike the CLI
        // `flags` command, so an applied entry states a real narrowing.
        request_outcomes: resolved.request_outcomes(),
        meta: resolved.explain_enabled().then(feature_flags_meta),
        retirement,
    });

    Ok(FeatureFlagsProgrammaticOutput {
        output,
        telemetry_analysis_run_id: None,
    })
}

fn load_feature_flags_session(
    resolved: &ProgrammaticAnalysisContext,
) -> ProgrammaticResult<AnalysisSession> {
    let project_config = fallow_engine::project_config::config_for_project_with_load_options(
        &resolved.root,
        resolved.config_path.as_deref(),
        fallow_config::ConfigLoadOptions {
            allow_remote_extends: resolved.allow_remote_extends(),
        },
    )
    .map_err(|err| {
        ProgrammaticError::new(format!("failed to load config: {err}"), 2)
            .with_code("FALLOW_CONFIG_LOAD_FAILED")
            .with_context("analysis.configPath")
    })?;
    Ok(super::dead_code::attach_cancellation(
        AnalysisSession::from_config(configure_project_for_feature_flags(
            project_config,
            resolved,
        )),
        resolved,
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

/// The files a flags run reports on, from the workspace, changed-since and
/// diff options.
struct FeatureFlagsScope<'a> {
    workspace_roots: Option<Vec<std::path::PathBuf>>,
    changed_files: Option<rustc_hash::FxHashSet<std::path::PathBuf>>,
    diff: Option<&'a DiffIndex>,
}

impl FeatureFlagsScope<'_> {
    fn is_whole_project(&self) -> bool {
        self.workspace_roots.is_none() && self.changed_files.is_none() && self.diff.is_none()
    }

    fn contains(&self, path: &Path, root: &Path) -> bool {
        self.workspace_roots
            .as_ref()
            .is_none_or(|roots| roots.iter().any(|workspace| path.starts_with(workspace)))
            && self
                .changed_files
                .as_ref()
                .is_none_or(|changed| changed.contains(path))
            && self.diff.as_ref().is_none_or(|diff| {
                diff.key_for(path, root)
                    .is_none_or(|rel| diff.touches_file(&rel))
            })
    }
}

fn feature_flags_scope<'a>(
    resolved: &'a ProgrammaticAnalysisContext,
    session: &AnalysisSession,
) -> ProgrammaticResult<FeatureFlagsScope<'a>> {
    let workspace_roots = workspace_roots_for_session(resolved, session.workspaces())?;
    let changed_files = changed_files_for_run(resolved)?;
    if changed_files.is_some() {
        resolved
            .measure_changed_since_scope(session.files().iter().map(|file| file.path.as_path()));
    }
    Ok(FeatureFlagsScope {
        workspace_roots,
        changed_files,
        diff: resolved.diff_index(),
    })
}

/// Read the vendor export before the analysis, so an invalid file fails
/// fast.
fn load_vendor_export(
    options: &FeatureFlagsOptions,
    root: &Path,
) -> ProgrammaticResult<Option<VendorExport>> {
    let Some(path) = options
        .retirement
        .as_ref()
        .and_then(|retirement| retirement.flag_state.as_deref())
    else {
        return Ok(None);
    };
    load_flag_state(path, root).map(Some).map_err(|error| {
        ProgrammaticError::new(error.message, 2)
            .with_code("FALLOW_FLAG_STATE_INVALID")
            .with_help(error.help)
            .with_context("feature-flags.retirement.flagState")
    })
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
