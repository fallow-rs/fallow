use std::{path::PathBuf, time::Instant};

use fallow_config::WorkspaceInfo;
use fallow_engine::{
    dead_code::DeadCodeAnalysisArtifacts, project_analysis::ProjectAnalysisArtifactOptions,
    session::AnalysisSession,
};
use fallow_output::{CombinedNextStepsInput, build_combined_next_steps};
use rustc_hash::FxHashSet;

use crate::{
    AnalysisOptions, CombinedOptions, CombinedProgrammaticOutput, ComplexityOptions,
    DeadCodeFilters, DeadCodeOptions, DuplicationOptions, ProgrammaticError,
    analysis_context::{
        changed_files_for_run, resolve_programmatic_analysis_context_deferred_workspace,
    },
    next_steps::{
        default_workspace_ref, default_workspace_ref_for_workspaces, setup_pointer_applicable,
        suggestions_enabled,
    },
};

use super::{
    EffectiveProductionModes, ProgrammaticResult, health_may_consume_dead_code_artifacts,
    health_may_consume_duplication_report, resolve_effective_production_modes, root_envelope_mode,
    run_duplication, run_health, run_health_with_session_artifacts,
};

struct PreparedCombinedOptions {
    dead_code: DeadCodeOptions,
    duplication: DuplicationOptions,
    health: ComplexityOptions,
}

struct CombinedSectionRun {
    dead_code: Option<crate::DeadCodeProgrammaticOutput>,
    duplication: Option<crate::DuplicationProgrammaticOutput>,
    health: Option<crate::HealthProgrammaticOutput>,
    root: PathBuf,
    workspaces: Option<Vec<WorkspaceInfo>>,
}

struct DeadCodeSessionRun<'a> {
    options: &'a CombinedOptions,
    resolved: &'a crate::analysis_context::ProgrammaticAnalysisContext,
    prepared: &'a PreparedCombinedOptions,
    changed_files: Option<&'a FxHashSet<PathBuf>>,
    session: &'a AnalysisSession,
}

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
    let resolved = resolve_programmatic_analysis_context_deferred_workspace(&options.analysis)?;
    resolved.install(|| {
        let production_modes = resolve_effective_production_modes(&resolved, None, None, None)?;
        let prepared = prepare_combined_options(options, production_modes);
        let changed_files = changed_files_for_run(&resolved)?;
        let sections = run_combined_sections(
            options,
            &resolved,
            &prepared,
            changed_files.as_ref(),
            production_modes,
        )?;

        let next_steps = combined_next_steps(
            sections.dead_code.as_ref(),
            sections.duplication.as_ref(),
            sections.health.as_ref(),
            &sections.root,
            sections.workspaces.as_deref(),
        );

        Ok(CombinedProgrammaticOutput {
            dead_code: sections.dead_code,
            duplication: sections.duplication,
            health: sections.health,
            root: sections.root,
            elapsed: start.elapsed(),
            explain: options.analysis.explain,
            next_steps,
            envelope_mode: root_envelope_mode(),
            telemetry_analysis_run_id: None,
        })
    })
}

fn prepare_combined_options(
    options: &CombinedOptions,
    production_modes: EffectiveProductionModes,
) -> PreparedCombinedOptions {
    PreparedCombinedOptions {
        dead_code: combined_dead_code_options(options, production_modes.dead_code),
        duplication: combined_duplication_options(options, production_modes.dupes),
        health: combined_health_options(options, production_modes.health),
    }
}

fn run_combined_sections(
    options: &CombinedOptions,
    resolved: &crate::analysis_context::ProgrammaticAnalysisContext,
    prepared: &PreparedCombinedOptions,
    changed_files: Option<&FxHashSet<PathBuf>>,
    production_modes: EffectiveProductionModes,
) -> ProgrammaticResult<CombinedSectionRun> {
    let share_health = options.dead_code
        && options.health
        && production_modes.dead_code == production_modes.health;
    let share_dupes = options.dead_code
        && options.duplication
        && production_modes.dead_code == production_modes.dupes;
    if share_health || share_dupes {
        return run_combined_with_dead_code_session(
            options,
            resolved,
            prepared,
            changed_files,
            share_health,
            share_dupes,
        );
    }
    run_combined_sections_isolated(options, resolved, prepared)
}

fn run_combined_with_dead_code_session(
    options: &CombinedOptions,
    resolved: &crate::analysis_context::ProgrammaticAnalysisContext,
    prepared: &PreparedCombinedOptions,
    changed_files: Option<&FxHashSet<PathBuf>>,
    share_health: bool,
    share_dupes: bool,
) -> ProgrammaticResult<CombinedSectionRun> {
    let session = super::dead_code::load_dead_code_session(&prepared.dead_code, resolved)?;
    if share_dupes {
        return run_combined_with_project_artifacts(
            options,
            resolved,
            prepared,
            changed_files,
            share_health,
            &session,
        );
    }
    let ctx = DeadCodeSessionRun {
        options,
        resolved,
        prepared,
        changed_files,
        session: &session,
    };
    let (dead_code, dead_code_artifacts) =
        run_dead_code_with_optional_artifacts(&ctx, options.health && share_health)?;
    let duplication = run_combined_duplication(&ctx, share_dupes)?;
    let health = run_combined_health(&ctx, share_health, dead_code_artifacts, None)?;
    Ok(CombinedSectionRun {
        dead_code,
        duplication,
        health,
        root: session.root().to_path_buf(),
        workspaces: Some(session.workspaces().to_vec()),
    })
}

fn run_combined_with_project_artifacts(
    options: &CombinedOptions,
    resolved: &crate::analysis_context::ProgrammaticAnalysisContext,
    prepared: &PreparedCombinedOptions,
    changed_files: Option<&FxHashSet<PathBuf>>,
    share_health: bool,
    session: &AnalysisSession,
) -> ProgrammaticResult<CombinedSectionRun> {
    let retain_dead_code_artifacts =
        share_health && health_may_consume_dead_code_artifacts(&prepared.health, session.config());
    let dupes_config =
        super::duplication::build_dupes_config(&prepared.duplication, &session.config().duplicates);
    let section_start = Instant::now();
    let project = session
        .analyze_project_with_artifacts(
            &dupes_config,
            ProjectAnalysisArtifactOptions {
                retain_complexity_artifacts: retain_dead_code_artifacts,
                retain_graph: retain_dead_code_artifacts,
                changed_files: changed_files.cloned(),
                collect_source_fingerprints: false,
            },
        )
        .map_err(|err| {
            ProgrammaticError::new(format!("combined analysis failed: {err}"), 2)
                .with_code("FALLOW_COMBINED_FAILED")
                .with_context("combined")
        })?;
    let dead_code = super::dead_code::run_dead_code_from_artifacts(
        &prepared.dead_code,
        resolved,
        session,
        changed_files,
        project.dead_code,
        section_start,
    )?;
    let pre_computed_duplication_for_health = (options.health
        && share_health
        && health_may_consume_duplication_report(&prepared.health)
        && duplication_options_preserve_health_config(&prepared.duplication))
    .then(|| project.duplication.clone());
    let duplication = options
        .duplication
        .then(|| {
            super::duplication::run_duplication_report_with_session(
                &prepared.duplication,
                resolved,
                session,
                project.duplication,
                section_start,
            )
        })
        .transpose()?;
    let super::dead_code::DeadCodeProgrammaticRunWithArtifacts {
        output: dead_code,
        artifacts,
    } = dead_code;
    let dead_code_artifacts = retain_dead_code_artifacts.then_some(artifacts);
    let health = run_combined_health(
        &DeadCodeSessionRun {
            options,
            resolved,
            prepared,
            changed_files,
            session,
        },
        share_health,
        dead_code_artifacts,
        pre_computed_duplication_for_health,
    )?;

    Ok(CombinedSectionRun {
        dead_code: Some(dead_code),
        duplication,
        health,
        root: session.root().to_path_buf(),
        workspaces: Some(session.workspaces().to_vec()),
    })
}

fn run_dead_code_with_optional_artifacts(
    ctx: &DeadCodeSessionRun<'_>,
    share_health: bool,
) -> ProgrammaticResult<(
    Option<crate::DeadCodeProgrammaticOutput>,
    Option<DeadCodeAnalysisArtifacts>,
)> {
    let retain_artifacts = share_health
        && health_may_consume_dead_code_artifacts(&ctx.prepared.health, ctx.session.config());
    if retain_artifacts {
        let dead_code = super::dead_code::run_dead_code_with_session_artifacts(
            &ctx.prepared.dead_code,
            ctx.resolved,
            ctx.session,
            ctx.changed_files,
            |_| {},
            Instant::now(),
        )?;
        return Ok((Some(dead_code.output), Some(dead_code.artifacts)));
    }
    let dead_code = super::dead_code::run_dead_code_with_session(
        &ctx.prepared.dead_code,
        ctx.resolved,
        ctx.session,
        ctx.changed_files,
        |_| {},
        Instant::now(),
    )?;
    Ok((Some(dead_code), None))
}

fn run_combined_duplication(
    ctx: &DeadCodeSessionRun<'_>,
    share_dupes: bool,
) -> ProgrammaticResult<Option<crate::DuplicationProgrammaticOutput>> {
    if !ctx.options.duplication {
        return Ok(None);
    }
    if !share_dupes {
        return run_duplication(&ctx.prepared.duplication).map(Some);
    }
    super::duplication::run_duplication_with_session(
        &ctx.prepared.duplication,
        ctx.resolved,
        ctx.session,
        ctx.changed_files,
        Instant::now(),
    )
    .map(Some)
}

fn run_combined_health(
    ctx: &DeadCodeSessionRun<'_>,
    share_health: bool,
    dead_code_artifacts: Option<DeadCodeAnalysisArtifacts>,
    pre_computed_duplication: Option<fallow_engine::duplicates::DuplicationReport>,
) -> ProgrammaticResult<Option<crate::HealthProgrammaticOutput>> {
    if !ctx.options.health {
        return Ok(None);
    }
    if !share_health {
        return run_health(&ctx.prepared.health).map(Some);
    }
    run_health_with_session_artifacts(
        &ctx.prepared.health,
        ctx.resolved,
        ctx.session,
        ctx.changed_files,
        dead_code_artifacts,
        pre_computed_duplication,
    )
    .map(Some)
}

fn run_combined_sections_isolated(
    options: &CombinedOptions,
    resolved: &crate::analysis_context::ProgrammaticAnalysisContext,
    prepared: &PreparedCombinedOptions,
) -> ProgrammaticResult<CombinedSectionRun> {
    Ok(CombinedSectionRun {
        dead_code: options
            .dead_code
            .then(|| super::dead_code::run_dead_code(&prepared.dead_code))
            .transpose()?,
        duplication: options
            .duplication
            .then(|| run_duplication(&prepared.duplication))
            .transpose()?,
        health: options
            .health
            .then(|| run_health(&prepared.health))
            .transpose()?,
        root: resolved.root().to_path_buf(),
        workspaces: None,
    })
}

fn combined_dead_code_options(options: &CombinedOptions, production: bool) -> DeadCodeOptions {
    DeadCodeOptions {
        analysis: analysis_with_effective_production(&options.analysis, production),
        filters: DeadCodeFilters::default(),
        files: Vec::new(),
        include_entry_exports: options.include_entry_exports,
    }
}

fn combined_duplication_options(options: &CombinedOptions, production: bool) -> DuplicationOptions {
    let mut duplication = options.duplication_options.clone();
    duplication.analysis = analysis_with_effective_production(&options.analysis, production);
    duplication
}

fn duplication_options_preserve_health_config(options: &DuplicationOptions) -> bool {
    options.mode.is_none()
        && options.min_tokens.is_none()
        && options.min_lines.is_none()
        && options.min_occurrences.is_none()
        && options.threshold.is_none()
        && options.skip_local.is_none()
        && options.cross_language.is_none()
        && options.ignore_imports.is_none()
}

fn combined_health_options(options: &CombinedOptions, production: bool) -> ComplexityOptions {
    let mut health = options.health_options.clone();
    health.analysis = analysis_with_effective_production(&options.analysis, production);
    health
}

fn analysis_with_effective_production(
    analysis: &AnalysisOptions,
    production: bool,
) -> AnalysisOptions {
    AnalysisOptions {
        production,
        production_override: Some(production),
        ..analysis.clone()
    }
}

fn combined_next_steps(
    dead_code: Option<&crate::DeadCodeProgrammaticOutput>,
    duplication: Option<&crate::DuplicationProgrammaticOutput>,
    health: Option<&crate::HealthProgrammaticOutput>,
    root: &std::path::Path,
    workspaces: Option<&[WorkspaceInfo]>,
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
    let workspace_ref = audit_changed
        .then(|| {
            workspaces.map_or_else(
                || default_workspace_ref(root),
                |workspaces| default_workspace_ref_for_workspaces(root, workspaces),
            )
        })
        .flatten();
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::DuplicationMode;

    #[test]
    fn health_reuses_combined_duplication_only_without_detector_overrides() {
        assert!(duplication_options_preserve_health_config(
            &DuplicationOptions::default()
        ));
        assert!(!duplication_options_preserve_health_config(
            &DuplicationOptions {
                min_tokens: Some(1),
                ..DuplicationOptions::default()
            }
        ));
        assert!(!duplication_options_preserve_health_config(
            &DuplicationOptions {
                mode: Some(DuplicationMode::Semantic),
                ..DuplicationOptions::default()
            }
        ));
    }
}
