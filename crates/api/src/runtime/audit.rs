use std::path::{Path, PathBuf};
use std::time::Instant;

use fallow_config::AuditGate;
use fallow_engine::{
    dead_code::DeadCodeAnalysisArtifacts,
    project_analysis::ProjectAnalysisArtifactOptions,
    repo_refs::{self, ResolvedAuditBase, TemporaryBaseWorktree},
    session::AnalysisSession,
};
use fallow_output::build_audit_next_steps;
use fallow_types::output::NextStep;
use rustc_hash::FxHashSet;

use crate::{
    AnalysisOptions, AuditAttribution, AuditOptions, AuditProgrammaticKeySnapshot,
    AuditProgrammaticOutput, AuditSummary, AuditVerdict, ComplexityOptions, DeadCodeFilters,
    DeadCodeOptions, DuplicationOptions, ProgrammaticError,
    analysis_context::{
        ProgrammaticAnalysisContext, changed_files_for_run,
        resolve_programmatic_analysis_context_deferred_workspace,
    },
};

use super::{
    ProgrammaticResult, health_may_consume_dead_code_artifacts,
    health_may_consume_duplication_report, resolve_effective_production_modes, root_envelope_mode,
    run_dead_code, run_duplication, run_health, run_health_with_session_artifacts,
};

/// Run changed-code audit through typed programmatic runners.
///
/// # Errors
///
/// Returns a structured error for invalid options, base-ref discovery failures,
/// unsupported CLI-only audit surfaces, or analysis failures.
pub fn run_audit(options: &AuditOptions) -> ProgrammaticResult<AuditProgrammaticOutput> {
    validate_audit_api_options(options)?;
    let start = Instant::now();
    let resolved_base = resolve_audit_base_ref(options)?;
    let analysis = analysis_options_for_audit(options, &resolved_base.git_ref);
    let resolved = resolve_programmatic_analysis_context_deferred_workspace(&analysis)?;
    let changed_files = changed_files_for_run(&resolved)?.unwrap_or_default();
    let changed_files_count = changed_files.len();

    if changed_files.is_empty() {
        return Ok(empty_audit_output(
            options,
            resolved_base,
            resolved.root(),
            changed_files_count,
            start.elapsed(),
        ));
    }

    let head =
        run_audit_subanalyses_with_context(options, &analysis, &resolved, Some(&changed_files))?;
    let current_snapshot = snapshot_from_analyses(&head);
    let base_snapshot = if matches!(options.gate, AuditGate::NewOnly) {
        Some(compute_base_snapshot(options, &resolved_base.git_ref)?)
    } else {
        None
    };
    let summary = build_programmatic_audit_summary(&head);
    let attribution = compute_programmatic_audit_attribution(
        options.gate,
        &current_snapshot,
        base_snapshot.as_ref(),
    );
    let verdict = compute_programmatic_audit_verdict(
        options.gate,
        &summary,
        &head.duplication,
        &current_snapshot,
        base_snapshot.as_ref(),
    );
    let next_steps = audit_next_steps(&head.dead_code, &head.complexity);

    Ok(AuditProgrammaticOutput {
        verdict,
        summary,
        attribution,
        changed_files_count,
        base_ref: resolved_base.git_ref,
        base_description: resolved_base.description,
        head_sha: repo_refs::short_head_sha(resolved.root()),
        elapsed: start.elapsed(),
        base_snapshot_skipped: None,
        base_snapshot,
        dead_code: Some(head.dead_code),
        duplication: Some(head.duplication),
        complexity: Some(head.complexity),
        next_steps,
        envelope_mode: root_envelope_mode(),
        telemetry_analysis_run_id: None,
    })
}

fn validate_audit_api_options(options: &AuditOptions) -> ProgrammaticResult<()> {
    if let Err(err) =
        fallow_engine::health::validate_coverage_root_absolute(options.coverage_root.as_deref())
    {
        return Err(ProgrammaticError::new(err, 2)
            .with_code("FALLOW_INVALID_COVERAGE_ROOT")
            .with_context("audit.coverageRoot"));
    }
    if options.runtime_coverage.is_some() {
        return Err(ProgrammaticError::new(
            "programmatic audit does not yet support runtime coverage; use the CLI path",
            2,
        )
        .with_code("FALLOW_AUDIT_RUNTIME_COVERAGE_UNSUPPORTED")
        .with_context("audit.runtimeCoverage"));
    }
    Ok(())
}

pub(super) fn resolve_audit_base_ref(
    options: &AuditOptions,
) -> ProgrammaticResult<ResolvedAuditBase> {
    if let Some(ref_str) = options
        .base
        .as_deref()
        .or(options.analysis.changed_since.as_deref())
    {
        validate_git_ref(ref_str, "audit.base")?;
        return Ok(ResolvedAuditBase {
            git_ref: (*ref_str).to_string(),
            description: None,
        });
    }
    if let Some(env_ref) = audit_base_env_override() {
        validate_git_ref(&env_ref, "FALLOW_AUDIT_BASE")?;
        return Ok(ResolvedAuditBase {
            description: Some(format!("FALLOW_AUDIT_BASE={env_ref}")),
            git_ref: env_ref,
        });
    }
    let root = options
        .analysis
        .root
        .clone()
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
    repo_refs::auto_detect_audit_base_ref(&root).ok_or_else(|| {
        ProgrammaticError::new(
            "could not detect base branch. Set audit.base to specify the comparison target",
            2,
        )
        .with_code("FALLOW_AUDIT_BASE_NOT_FOUND")
        .with_context("audit.base")
    })
}

fn analysis_options_for_audit(options: &AuditOptions, base_ref: &str) -> AnalysisOptions {
    AnalysisOptions {
        changed_since: Some(base_ref.to_string()),
        production: options.production,
        production_override: options.production.then_some(true),
        ..options.analysis.clone()
    }
}

fn analysis_with_production(
    analysis: &AnalysisOptions,
    production_override: Option<bool>,
) -> AnalysisOptions {
    AnalysisOptions {
        production: production_override.unwrap_or(analysis.production),
        production_override: production_override.or(analysis.production_override),
        ..analysis.clone()
    }
}

fn empty_audit_output(
    options: &AuditOptions,
    base: ResolvedAuditBase,
    root: &Path,
    changed_files_count: usize,
    elapsed: std::time::Duration,
) -> AuditProgrammaticOutput {
    AuditProgrammaticOutput {
        verdict: AuditVerdict::Pass,
        summary: AuditSummary {
            dead_code_issues: 0,
            dead_code_has_errors: false,
            complexity_findings: 0,
            max_cyclomatic: None,
            duplication_clone_groups: 0,
        },
        attribution: AuditAttribution {
            gate: options.gate,
            ..AuditAttribution::default()
        },
        changed_files_count,
        base_ref: base.git_ref,
        base_description: base.description,
        head_sha: repo_refs::short_head_sha(root),
        elapsed,
        base_snapshot_skipped: None,
        base_snapshot: None,
        dead_code: None,
        duplication: None,
        complexity: None,
        next_steps: Vec::new(),
        envelope_mode: root_envelope_mode(),
        telemetry_analysis_run_id: None,
    }
}

struct AuditSubanalyses {
    dead_code: crate::DeadCodeProgrammaticOutput,
    duplication: crate::DuplicationProgrammaticOutput,
    complexity: crate::HealthProgrammaticOutput,
}

struct AuditSubanalysisOptions {
    dead_code: DeadCodeOptions,
    duplication: DuplicationOptions,
    complexity: ComplexityOptions,
}

fn audit_subanalysis_options(
    options: &AuditOptions,
    analysis: &AnalysisOptions,
) -> AuditSubanalysisOptions {
    AuditSubanalysisOptions {
        dead_code: DeadCodeOptions {
            analysis: analysis_with_production(analysis, options.production_dead_code),
            filters: DeadCodeFilters::default(),
            files: Vec::new(),
            include_entry_exports: options.include_entry_exports,
        },
        duplication: DuplicationOptions {
            analysis: analysis_with_production(analysis, options.production_dupes),
            ..DuplicationOptions::default()
        },
        complexity: ComplexityOptions {
            analysis: analysis_with_production(analysis, options.production_health),
            max_crap: options.max_crap,
            complexity: true,
            css: options.css.unwrap_or(true),
            css_deep: options.css.unwrap_or(true) && options.css_deep.unwrap_or(true),
            coverage: options.coverage.clone(),
            coverage_root: options.coverage_root.clone(),
            ..ComplexityOptions::default()
        },
    }
}

fn run_audit_subanalyses(
    options: &AuditOptions,
    analysis: &AnalysisOptions,
    changed_files: Option<&FxHashSet<PathBuf>>,
) -> ProgrammaticResult<AuditSubanalyses> {
    let resolved = resolve_programmatic_analysis_context_deferred_workspace(analysis)?;
    run_audit_subanalyses_with_context(options, analysis, &resolved, changed_files)
}

fn run_audit_subanalyses_with_context(
    options: &AuditOptions,
    analysis: &AnalysisOptions,
    resolved: &ProgrammaticAnalysisContext,
    changed_files: Option<&FxHashSet<PathBuf>>,
) -> ProgrammaticResult<AuditSubanalyses> {
    let subanalysis_options = audit_subanalysis_options(options, analysis);
    let production_modes = resolve_effective_production_modes(
        resolved,
        options.production_dead_code,
        options.production_health,
        options.production_dupes,
    )?;

    if production_modes.dead_code == production_modes.dupes
        && production_modes.dead_code == production_modes.health
    {
        return run_shared_project_audit_subanalyses(&subanalysis_options, changed_files);
    }

    if production_modes.dead_code == production_modes.health {
        return run_shared_dead_code_health_audit_subanalyses(&subanalysis_options, changed_files);
    }

    if production_modes.dead_code == production_modes.dupes {
        return run_shared_dead_code_dupes_audit_subanalyses(&subanalysis_options, changed_files);
    }

    Ok(AuditSubanalyses {
        dead_code: run_dead_code(&subanalysis_options.dead_code)?,
        duplication: run_duplication(&subanalysis_options.duplication)?,
        complexity: run_health(&subanalysis_options.complexity)?,
    })
}

fn run_shared_project_audit_subanalyses(
    options: &AuditSubanalysisOptions,
    changed_files: Option<&FxHashSet<PathBuf>>,
) -> ProgrammaticResult<AuditSubanalyses> {
    let resolved =
        resolve_programmatic_analysis_context_deferred_workspace(&options.dead_code.analysis)?;
    resolved.install(|| {
        let session = super::dead_code::load_dead_code_session(&options.dead_code, &resolved)?;
        run_all_audit_subanalyses_with_project_artifacts(
            &options.dead_code,
            &options.duplication,
            &options.complexity,
            &resolved,
            &session,
            changed_files,
        )
    })
}

fn run_shared_dead_code_health_audit_subanalyses(
    options: &AuditSubanalysisOptions,
    changed_files: Option<&FxHashSet<PathBuf>>,
) -> ProgrammaticResult<AuditSubanalyses> {
    let resolved =
        resolve_programmatic_analysis_context_deferred_workspace(&options.dead_code.analysis)?;
    resolved.install(|| {
        let dead_code_options = &options.dead_code;
        let duplication_options = &options.duplication;
        let complexity_options = &options.complexity;
        let session = super::dead_code::load_dead_code_session(dead_code_options, &resolved)?;
        let (dead_code, complexity) = run_dead_code_and_health_with_session(
            dead_code_options,
            complexity_options,
            &resolved,
            &session,
            changed_files,
        )?;
        Ok(AuditSubanalyses {
            dead_code,
            duplication: run_duplication(duplication_options)?,
            complexity,
        })
    })
}

fn run_shared_dead_code_dupes_audit_subanalyses(
    options: &AuditSubanalysisOptions,
    changed_files: Option<&FxHashSet<PathBuf>>,
) -> ProgrammaticResult<AuditSubanalyses> {
    let resolved =
        resolve_programmatic_analysis_context_deferred_workspace(&options.dead_code.analysis)?;
    resolved.install(|| {
        let session = super::dead_code::load_dead_code_session(&options.dead_code, &resolved)?;
        let (dead_code, duplication, _, _) =
            run_dead_code_and_duplication_with_project_artifacts(ProjectArtifactAuditInput {
                dead_code_options: &options.dead_code,
                duplication_options: &options.duplication,
                resolved: &resolved,
                session: &session,
                changed_files,
                retain_dead_code_artifacts: false,
                retain_duplication_artifacts: false,
            })?;
        Ok(AuditSubanalyses {
            dead_code,
            duplication,
            complexity: run_health(&options.complexity)?,
        })
    })
}

fn run_dead_code_and_duplication_with_project_artifacts(
    input: ProjectArtifactAuditInput<'_>,
) -> ProgrammaticResult<(
    crate::DeadCodeProgrammaticOutput,
    crate::DuplicationProgrammaticOutput,
    Option<DeadCodeAnalysisArtifacts>,
    Option<fallow_engine::duplicates::DuplicationReport>,
)> {
    let dupes_config = super::duplication::build_dupes_config(
        input.duplication_options,
        &input.session.config().duplicates,
    );
    let section_start = Instant::now();
    let project = input
        .session
        .analyze_project_with_artifacts(
            &dupes_config,
            ProjectAnalysisArtifactOptions {
                retain_complexity_artifacts: input.retain_dead_code_artifacts,
                retain_graph: input.retain_dead_code_artifacts,
                changed_files: input.changed_files.cloned(),
                collect_source_fingerprints: false,
            },
        )
        .map_err(|err| {
            ProgrammaticError::new(format!("audit analysis failed: {err}"), 2)
                .with_code("FALLOW_AUDIT_FAILED")
                .with_context("audit")
        })?;
    let duplication_artifacts = input
        .retain_duplication_artifacts
        .then(|| project.duplication.clone());
    let dead_code = super::dead_code::run_dead_code_from_artifacts(
        input.dead_code_options,
        input.resolved,
        input.session,
        input.changed_files,
        project.dead_code,
        section_start,
    )?;
    let duplication = super::duplication::run_duplication_report_with_session(
        input.duplication_options,
        input.resolved,
        input.session,
        project.duplication,
        section_start,
    )?;
    let super::dead_code::DeadCodeProgrammaticRunWithArtifacts {
        output: dead_code,
        artifacts,
    } = dead_code;
    let dead_code_artifacts = input.retain_dead_code_artifacts.then_some(artifacts);
    Ok((
        dead_code,
        duplication,
        dead_code_artifacts,
        duplication_artifacts,
    ))
}

#[derive(Clone, Copy)]
struct ProjectArtifactAuditInput<'a> {
    dead_code_options: &'a DeadCodeOptions,
    duplication_options: &'a DuplicationOptions,
    resolved: &'a ProgrammaticAnalysisContext,
    session: &'a AnalysisSession,
    changed_files: Option<&'a FxHashSet<PathBuf>>,
    retain_dead_code_artifacts: bool,
    retain_duplication_artifacts: bool,
}

fn run_all_audit_subanalyses_with_project_artifacts(
    dead_code_options: &DeadCodeOptions,
    duplication_options: &DuplicationOptions,
    complexity_options: &ComplexityOptions,
    resolved: &ProgrammaticAnalysisContext,
    session: &AnalysisSession,
    changed_files: Option<&FxHashSet<PathBuf>>,
) -> ProgrammaticResult<AuditSubanalyses> {
    let retain_dead_code_artifacts =
        health_may_consume_dead_code_artifacts(complexity_options, session.config());
    let retain_duplication_artifacts = health_may_consume_duplication_report(complexity_options);
    let (dead_code, duplication, dead_code_artifacts, duplication_artifacts) =
        run_dead_code_and_duplication_with_project_artifacts(ProjectArtifactAuditInput {
            dead_code_options,
            duplication_options,
            resolved,
            session,
            changed_files,
            retain_dead_code_artifacts,
            retain_duplication_artifacts,
        })?;
    let complexity = run_health_with_session_artifacts(
        complexity_options,
        resolved,
        session,
        changed_files,
        dead_code_artifacts,
        duplication_artifacts,
    )?;
    Ok(AuditSubanalyses {
        dead_code,
        duplication,
        complexity,
    })
}

fn run_dead_code_and_health_with_session(
    dead_code_options: &DeadCodeOptions,
    complexity_options: &ComplexityOptions,
    resolved: &ProgrammaticAnalysisContext,
    session: &AnalysisSession,
    changed_files: Option<&FxHashSet<PathBuf>>,
) -> ProgrammaticResult<(
    crate::DeadCodeProgrammaticOutput,
    crate::HealthProgrammaticOutput,
)> {
    let reuse_dead_code_artifacts =
        health_may_consume_dead_code_artifacts(complexity_options, session.config());
    let (dead_code, dead_code_artifacts) = if reuse_dead_code_artifacts {
        let dead_code = super::dead_code::run_dead_code_with_session_artifacts(
            dead_code_options,
            resolved,
            session,
            changed_files,
            |_| {},
            Instant::now(),
        )?;
        (dead_code.output, Some(dead_code.artifacts))
    } else {
        (
            super::dead_code::run_dead_code_with_session(
                dead_code_options,
                resolved,
                session,
                changed_files,
                |_| {},
                Instant::now(),
            )?,
            None,
        )
    };
    let complexity = run_health_with_session_artifacts(
        complexity_options,
        resolved,
        session,
        changed_files,
        dead_code_artifacts,
        None,
    )?;
    Ok((dead_code, complexity))
}

fn build_programmatic_audit_summary(analyses: &AuditSubanalyses) -> AuditSummary {
    let dead_code_issues = analyses.dead_code.output.results.total_issues();
    AuditSummary {
        dead_code_issues,
        dead_code_has_errors: dead_code_issues > 0,
        complexity_findings: analyses.complexity.report.findings.len(),
        max_cyclomatic: analyses
            .complexity
            .report
            .findings
            .iter()
            .map(|finding| finding.cyclomatic)
            .max(),
        duplication_clone_groups: analyses.duplication.output.report.clone_groups.len(),
    }
}

fn compute_programmatic_audit_verdict(
    gate: AuditGate,
    summary: &AuditSummary,
    duplication: &crate::DuplicationProgrammaticOutput,
    current: &AuditProgrammaticKeySnapshot,
    base: Option<&AuditProgrammaticKeySnapshot>,
) -> AuditVerdict {
    if matches!(gate, AuditGate::NewOnly) {
        return compute_programmatic_introduced_verdict(summary, duplication, current, base);
    }
    if summary.dead_code_has_errors || summary.complexity_findings > 0 {
        return AuditVerdict::Fail;
    }
    if summary.duplication_clone_groups > 0 {
        let pct = duplication.output.report.stats.duplication_percentage;
        if duplication.threshold > 0.0 && pct > duplication.threshold {
            return AuditVerdict::Fail;
        }
        return AuditVerdict::Warn;
    }
    AuditVerdict::Pass
}

fn compute_programmatic_introduced_verdict(
    summary: &AuditSummary,
    duplication: &crate::DuplicationProgrammaticOutput,
    current: &AuditProgrammaticKeySnapshot,
    base: Option<&AuditProgrammaticKeySnapshot>,
) -> AuditVerdict {
    let attribution = compute_programmatic_audit_attribution(AuditGate::NewOnly, current, base);
    if attribution.dead_code_introduced > 0 || attribution.complexity_introduced > 0 {
        return AuditVerdict::Fail;
    }
    if attribution.duplication_introduced > 0 {
        let pct = duplication.output.report.stats.duplication_percentage;
        if duplication.threshold > 0.0 && pct > duplication.threshold {
            return AuditVerdict::Fail;
        }
        return AuditVerdict::Warn;
    }
    if summary.dead_code_issues == 0
        && summary.complexity_findings == 0
        && summary.duplication_clone_groups == 0
    {
        return AuditVerdict::Pass;
    }
    AuditVerdict::Pass
}

fn compute_programmatic_audit_attribution(
    gate: AuditGate,
    current: &AuditProgrammaticKeySnapshot,
    base: Option<&AuditProgrammaticKeySnapshot>,
) -> AuditAttribution {
    let dead_code = count_introduced(&current.dead_code, base.map(|snapshot| &snapshot.dead_code));
    let complexity = count_introduced(&current.health, base.map(|snapshot| &snapshot.health));
    let duplication = count_introduced(&current.dupes, base.map(|snapshot| &snapshot.dupes));
    AuditAttribution {
        gate,
        dead_code_introduced: dead_code.0,
        dead_code_inherited: dead_code.1,
        complexity_introduced: complexity.0,
        complexity_inherited: complexity.1,
        duplication_introduced: duplication.0,
        duplication_inherited: duplication.1,
    }
}

fn count_introduced(
    keys: &rustc_hash::FxHashSet<String>,
    base: Option<&rustc_hash::FxHashSet<String>>,
) -> (usize, usize) {
    let Some(base) = base else {
        return (0, 0);
    };
    keys.iter().fold((0, 0), |(introduced, inherited), key| {
        if base.contains(key) {
            (introduced, inherited + 1)
        } else {
            (introduced + 1, inherited)
        }
    })
}

fn snapshot_from_analyses(analyses: &AuditSubanalyses) -> AuditProgrammaticKeySnapshot {
    AuditProgrammaticKeySnapshot {
        dead_code: crate::audit_keys::dead_code_keys(
            &analyses.dead_code.output.results,
            &analyses.dead_code.root,
        ),
        health: crate::audit_keys::health_keys(
            &analyses.complexity.report,
            &analyses.complexity.root,
        ),
        dupes: analyses
            .duplication
            .output
            .report
            .clone_groups
            .iter()
            .map(|group| {
                crate::audit_keys::dupe_group_key(&group.group, &analyses.duplication.root)
            })
            .collect(),
    }
}

fn compute_base_snapshot(
    options: &AuditOptions,
    base_ref: &str,
) -> ProgrammaticResult<AuditProgrammaticKeySnapshot> {
    let current_root = analysis_root_from_options(options)?;
    let worktree = TemporaryBaseWorktree::create(&current_root, base_ref).map_err(|err| {
        ProgrammaticError::new(err.to_string(), 2)
            .with_code("FALLOW_AUDIT_BASE_WORKTREE_FAILED")
            .with_context("audit.base")
    })?;
    let base_root = repo_refs::base_analysis_root(&current_root, worktree.path());
    let current_config_path = options
        .analysis
        .config_path
        .clone()
        .or_else(|| fallow_config::FallowConfig::find_config_path(&current_root));
    let base_analysis = AnalysisOptions {
        root: Some(base_root),
        config_path: current_config_path,
        changed_since: None,
        explain: false,
        ..options.analysis.clone()
    };
    let base = run_audit_subanalyses(options, &base_analysis, None)?;
    Ok(snapshot_from_analyses(&base))
}

fn analysis_root_from_options(options: &AuditOptions) -> ProgrammaticResult<PathBuf> {
    match options.analysis.root.clone() {
        Some(root) => Ok(root),
        None => std::env::current_dir().map_err(|err| {
            ProgrammaticError::new(
                format!("failed to resolve current working directory: {err}"),
                2,
            )
            .with_code("FALLOW_CWD_UNAVAILABLE")
            .with_context("analysis.root")
        }),
    }
}

fn audit_next_steps(
    dead_code: &crate::DeadCodeProgrammaticOutput,
    complexity: &crate::HealthProgrammaticOutput,
) -> Vec<NextStep> {
    let input = fallow_output::build_audit_next_steps_input(
        Some((&dead_code.output.results, dead_code.root.as_path())),
        Some(&complexity.report),
        crate::next_steps::suggestions_enabled(),
    );
    build_audit_next_steps(&input)
}

fn validate_git_ref(value: &str, context: &'static str) -> ProgrammaticResult<()> {
    fallow_engine::validate::validate_git_ref(value)
        .map(|_| ())
        .map_err(|err| {
            ProgrammaticError::new(format!("invalid git ref `{value}`: {err}"), 2)
                .with_code("FALLOW_INVALID_GIT_REF")
                .with_context(context)
        })
}

fn audit_base_env_override() -> Option<String> {
    std::env::var("FALLOW_AUDIT_BASE")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

#[cfg(test)]
mod tests {
    use std::process::Command;

    use fallow_config::{AuditGate, FallowConfig, HealthConfig};
    use fallow_types::output_format::OutputFormat;

    use super::*;

    fn resolved_config_with_max_crap(max_crap: f64) -> fallow_config::ResolvedConfig {
        FallowConfig {
            health: HealthConfig {
                max_crap,
                ..HealthConfig::default()
            },
            ..FallowConfig::default()
        }
        .resolve(
            std::env::temp_dir().join("fallow-api-runtime-test"),
            OutputFormat::Json,
            1,
            true,
            true,
            None,
        )
    }

    #[test]
    fn audit_complexity_only_health_does_not_retain_dead_code_artifacts() {
        let options = ComplexityOptions {
            complexity: true,
            ..ComplexityOptions::default()
        };
        let config = resolved_config_with_max_crap(0.0);

        assert!(!health_may_consume_dead_code_artifacts(&options, &config));
    }

    #[test]
    fn audit_health_artifact_reuse_tracks_config_max_crap() {
        let options = ComplexityOptions {
            complexity: true,
            ..ComplexityOptions::default()
        };
        let config = resolved_config_with_max_crap(30.0);

        assert!(health_may_consume_dead_code_artifacts(&options, &config));
    }

    #[test]
    fn audit_health_artifact_reuse_tracks_file_score_inputs() {
        let config = resolved_config_with_max_crap(0.0);
        for options in [
            ComplexityOptions {
                file_scores: true,
                ..ComplexityOptions::default()
            },
            ComplexityOptions {
                coverage_gaps: true,
                ..ComplexityOptions::default()
            },
            ComplexityOptions {
                targets: true,
                ..ComplexityOptions::default()
            },
            ComplexityOptions {
                score: true,
                ..ComplexityOptions::default()
            },
            ComplexityOptions {
                max_crap: Some(30.0),
                complexity: true,
                ..ComplexityOptions::default()
            },
        ] {
            assert!(health_may_consume_dead_code_artifacts(&options, &config));
        }
    }

    #[test]
    fn audit_health_duplication_reuse_tracks_score_and_targets() {
        for options in [
            ComplexityOptions {
                score: true,
                ..ComplexityOptions::default()
            },
            ComplexityOptions {
                targets: true,
                ..ComplexityOptions::default()
            },
        ] {
            assert!(health_may_consume_duplication_report(&options));
        }

        assert!(!health_may_consume_duplication_report(&ComplexityOptions {
            complexity: true,
            ..ComplexityOptions::default()
        }));
    }

    #[test]
    fn run_audit_default_new_only_marks_untracked_added_file_introduced() {
        let project = audit_fixture();
        let output = run_audit(&AuditOptions {
            analysis: AnalysisOptions {
                root: Some(project.path().to_path_buf()),
                no_cache: true,
                explain: true,
                ..AnalysisOptions::default()
            },
            base: Some("HEAD".to_string()),
            gate: AuditGate::NewOnly,
            ..AuditOptions::default()
        })
        .expect("audit output");

        assert_eq!(output.verdict, AuditVerdict::Fail);
        assert_eq!(output.summary.dead_code_issues, 1);
        assert_eq!(output.attribution.dead_code_introduced, 1);
        assert!(output.base_snapshot.is_some());

        let json = crate::serialize_audit_programmatic_json(output).expect("audit json");
        assert_eq!(
            json["dead_code"]["unused_files"][0]["path"],
            "src/feature.ts"
        );
        assert_eq!(json["dead_code"]["unused_files"][0]["introduced"], true);
    }

    #[test]
    fn audit_production_mode_branches_preserve_per_section_workspace_scope() {
        let project = audit_workspace_modes_fixture();

        for mask in 0_u8..8 {
            let production_dead_code = mask & 0b001 != 0;
            let production_health = mask & 0b010 != 0;
            let production_dupes = mask & 0b100 != 0;
            let output = run_audit(&AuditOptions {
                analysis: AnalysisOptions {
                    root: Some(project.path().to_path_buf()),
                    workspace: Some(vec!["@audit/a".to_string()]),
                    no_cache: true,
                    ..AnalysisOptions::default()
                },
                base: Some("HEAD".to_string()),
                gate: AuditGate::All,
                production_dead_code: Some(production_dead_code),
                production_health: Some(production_health),
                production_dupes: Some(production_dupes),
                include_entry_exports: true,
                ..AuditOptions::default()
            })
            .unwrap_or_else(|error| panic!("audit mask {mask:03b} failed: {error}"));
            let json = crate::serialize_audit_programmatic_json(output)
                .unwrap_or_else(|error| panic!("serialize mask {mask:03b}: {error}"));

            let dead_code = json["dead_code"].to_string();
            let complexity = json["complexity"].to_string();
            let duplication = json["duplication"].to_string();
            assert_eq!(
                dead_code.contains("mode-sentinel.test.ts"),
                !production_dead_code,
                "dead-code scope mismatch for mask {mask:03b}: {dead_code}"
            );
            assert_eq!(
                complexity.contains("mode-sentinel.test.ts"),
                !production_health,
                "health scope mismatch for mask {mask:03b}: {complexity}"
            );
            assert_eq!(
                duplication.contains("mode-sentinel.test.ts"),
                !production_dupes,
                "duplication scope mismatch for mask {mask:03b}: {duplication}"
            );

            let rendered = json.to_string();
            assert!(
                !rendered.contains("packages/b"),
                "workspace B leaked into mask {mask:03b}: {rendered}"
            );
        }
    }

    #[test]
    fn empty_audit_output_uses_resolved_root_for_head_sha() {
        let project = audit_fixture();
        let output = empty_audit_output(
            &AuditOptions {
                analysis: AnalysisOptions {
                    root: None,
                    ..AnalysisOptions::default()
                },
                base: Some("HEAD".to_string()),
                gate: AuditGate::NewOnly,
                ..AuditOptions::default()
            },
            ResolvedAuditBase {
                git_ref: "HEAD".to_string(),
                description: None,
            },
            project.path(),
            0,
            std::time::Duration::ZERO,
        );

        assert!(output.head_sha.is_some());
    }

    fn audit_fixture() -> tempfile::TempDir {
        let project = tempfile::tempdir().expect("project");
        std::fs::create_dir_all(project.path().join("src")).expect("create src");
        std::fs::write(
            project.path().join("package.json"),
            r#"{"name":"audit-api","type":"module","main":"src/index.ts"}"#,
        )
        .expect("write package");
        std::fs::write(
            project.path().join("src/index.ts"),
            "console.log('entry');\n",
        )
        .expect("write entry");
        git(project.path(), &["init"]);
        git(project.path(), &["add", "."]);
        git(
            project.path(),
            &[
                "-c",
                "user.email=test@example.com",
                "-c",
                "user.name=Test",
                "-c",
                "commit.gpgsign=false",
                "commit",
                "-m",
                "initial",
            ],
        );
        std::fs::write(
            project.path().join("src/feature.ts"),
            "export const unused = 1;\n",
        )
        .expect("write changed source");
        project
    }

    fn audit_workspace_modes_fixture() -> tempfile::TempDir {
        let project = tempfile::tempdir().expect("project");
        std::fs::write(
            project.path().join("package.json"),
            r#"{"name":"audit-root","private":true,"workspaces":["packages/*"]}"#,
        )
        .expect("write root package");
        std::fs::write(
            project.path().join(".fallowrc.json"),
            r#"{
  "duplicates": {
    "minTokens": 10,
    "minLines": 2,
    "ignoreDefaults": false
  },
  "health": {
    "maxCyclomatic": 2,
    "maxCognitive": 2,
    "maxCrap": 2.0,
    "maxUnitSize": 3
  }
}"#,
        )
        .expect("write config");

        for name in ["a", "b"] {
            let package = project.path().join("packages").join(name);
            std::fs::create_dir_all(package.join("src")).expect("create package source");
            std::fs::write(
                package.join("package.json"),
                format!(r#"{{"name":"@audit/{name}","type":"module","main":"src/index.ts"}}"#),
            )
            .expect("write package manifest");
            std::fs::write(
                package.join("src/index.ts"),
                format!("export const {name}Entry = true;\n"),
            )
            .expect("write package entry");
        }

        git(project.path(), &["init"]);
        git(project.path(), &["add", "."]);
        git(
            project.path(),
            &[
                "-c",
                "user.email=test@example.com",
                "-c",
                "user.name=Test",
                "-c",
                "commit.gpgsign=false",
                "commit",
                "-m",
                "initial",
            ],
        );

        let sentinel = r"export function auditModeSentinel(value: number) {
  let result = value;
  if (value > 0) result += 1;
  if (value > 1) result += 2;
  if (value > 2) result += 3;
  if (value > 3) result += 4;
  return result;
}
";
        for name in ["a", "b"] {
            let source = project.path().join("packages").join(name).join("src");
            std::fs::write(source.join("mode-sentinel.test.ts"), sentinel)
                .expect("write test sentinel");
            std::fs::write(source.join("mode-sentinel-copy.test.ts"), sentinel)
                .expect("write duplicate test sentinel");
        }

        project
    }

    fn git(root: &Path, args: &[&str]) {
        let status = Command::new("git")
            .args(args)
            .current_dir(root)
            .status()
            .expect("git command");
        assert!(status.success(), "git {args:?} failed");
    }
}
