use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

use fallow_config::AuditGate;
use fallow_engine::clear_ambient_git_env;
use fallow_output::build_audit_next_steps;
use fallow_types::output::NextStep;

use crate::{
    AnalysisOptions, AuditAttribution, AuditOptions, AuditProgrammaticOutput, AuditSummary,
    AuditVerdict, ComplexityOptions, DeadCodeFilters, DeadCodeOptions, DuplicationOptions,
    ProgrammaticError,
    analysis_context::{changed_files_for_run, resolve_programmatic_analysis_context},
};

use super::{ProgrammaticResult, root_envelope_mode, run_dead_code, run_duplication, run_health};

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
    let resolved = resolve_programmatic_analysis_context(&analysis)?;
    let changed_files = changed_files_for_run(&resolved)?.unwrap_or_default();
    let changed_files_count = changed_files.len();

    if changed_files.is_empty() {
        return Ok(empty_audit_output(
            options,
            resolved_base,
            changed_files_count,
            start.elapsed(),
        ));
    }

    let dead_code_options = DeadCodeOptions {
        analysis: analysis_with_production(&analysis, options.production_dead_code),
        filters: DeadCodeFilters::default(),
        files: Vec::new(),
        include_entry_exports: options.include_entry_exports,
    };
    let duplication_options = DuplicationOptions {
        analysis: analysis_with_production(&analysis, options.production_dupes),
        ..DuplicationOptions::default()
    };
    let complexity_options = ComplexityOptions {
        analysis: analysis_with_production(&analysis, options.production_health),
        max_crap: options.max_crap,
        complexity: true,
        coverage: options.coverage.clone(),
        coverage_root: options.coverage_root.clone(),
        ..ComplexityOptions::default()
    };

    let dead_code = run_dead_code(&dead_code_options)?;
    let duplication = run_duplication(&duplication_options)?;
    let complexity = run_health(&complexity_options)?;
    let summary = build_programmatic_audit_summary(&dead_code, &duplication, &complexity);
    let attribution = AuditAttribution {
        gate: options.gate,
        dead_code_introduced: summary.dead_code_issues,
        complexity_introduced: summary.complexity_findings,
        duplication_introduced: summary.duplication_clone_groups,
        ..AuditAttribution::default()
    };
    let verdict = compute_programmatic_audit_verdict(&summary, &duplication);
    let next_steps = audit_next_steps(&dead_code, &complexity);

    Ok(AuditProgrammaticOutput {
        verdict,
        summary,
        attribution,
        changed_files_count,
        base_ref: resolved_base.git_ref,
        base_description: resolved_base.description,
        head_sha: get_head_sha(resolved.root()),
        elapsed: start.elapsed(),
        base_snapshot_skipped: Some(false),
        dead_code: Some(dead_code),
        duplication: Some(duplication),
        complexity: Some(complexity),
        next_steps,
        envelope_mode: root_envelope_mode(options.analysis.legacy_envelope),
        telemetry_analysis_run_id: None,
    })
}

fn validate_audit_api_options(options: &AuditOptions) -> ProgrammaticResult<()> {
    if !matches!(options.gate, AuditGate::All) {
        return Err(ProgrammaticError::new(
            "programmatic audit currently supports gate=all; new-only attribution still uses the CLI path",
            2,
        )
        .with_code("FALLOW_AUDIT_GATE_UNSUPPORTED")
        .with_context("audit.gate"));
    }
    if let Err(err) =
        fallow_engine::validate_coverage_root_absolute(options.coverage_root.as_deref())
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

#[derive(Debug, Clone)]
struct ResolvedAuditBase {
    git_ref: String,
    description: Option<String>,
}

fn resolve_audit_base_ref(options: &AuditOptions) -> ProgrammaticResult<ResolvedAuditBase> {
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
    auto_detect_base_ref(&root).ok_or_else(|| {
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
        head_sha: options.analysis.root.as_deref().and_then(get_head_sha),
        elapsed,
        base_snapshot_skipped: Some(false),
        dead_code: None,
        duplication: None,
        complexity: None,
        next_steps: Vec::new(),
        envelope_mode: root_envelope_mode(options.analysis.legacy_envelope),
        telemetry_analysis_run_id: None,
    }
}

fn build_programmatic_audit_summary(
    dead_code: &crate::DeadCodeProgrammaticOutput,
    duplication: &crate::DuplicationProgrammaticOutput,
    complexity: &crate::HealthProgrammaticOutput,
) -> AuditSummary {
    let dead_code_issues = dead_code.output.results.total_issues();
    AuditSummary {
        dead_code_issues,
        dead_code_has_errors: dead_code_issues > 0,
        complexity_findings: complexity.report.findings.len(),
        max_cyclomatic: complexity
            .report
            .findings
            .iter()
            .map(|finding| finding.cyclomatic)
            .max(),
        duplication_clone_groups: duplication.output.report.clone_groups.len(),
    }
}

fn compute_programmatic_audit_verdict(
    summary: &AuditSummary,
    duplication: &crate::DuplicationProgrammaticOutput,
) -> AuditVerdict {
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

fn auto_detect_base_ref(root: &Path) -> Option<ResolvedAuditBase> {
    if let Some(upstream) = git_upstream_ref(root) {
        if let Some(sha) = git_merge_base(root, &upstream, "HEAD") {
            return Some(ResolvedAuditBase {
                git_ref: sha,
                description: Some(format!("merge-base with {upstream}")),
            });
        }
        return Some(ResolvedAuditBase {
            description: Some(format!("{upstream} (tip)")),
            git_ref: upstream,
        });
    }

    if let Some(remote_ref) = detect_remote_default_ref(root) {
        if let Some(sha) = git_merge_base(root, &remote_ref, "HEAD") {
            return Some(ResolvedAuditBase {
                git_ref: sha,
                description: Some(format!("merge-base with {remote_ref}")),
            });
        }
        return Some(ResolvedAuditBase {
            description: Some(format!("{remote_ref} (tip)")),
            git_ref: remote_ref,
        });
    }

    for candidate in ["main", "master"] {
        if git_ref_exists(root, candidate) {
            return Some(ResolvedAuditBase {
                git_ref: candidate.to_string(),
                description: Some(format!("local {candidate}")),
            });
        }
    }

    None
}

fn git_stdout(root: &Path, args: &[&str]) -> Option<String> {
    let mut command = Command::new("git");
    command.args(args).current_dir(root);
    clear_ambient_git_env(&mut command);
    let output = command.output().ok()?;
    if !output.status.success() {
        return None;
    }
    let trimmed = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (!trimmed.is_empty()).then_some(trimmed)
}

fn git_ref_exists(root: &Path, git_ref: &str) -> bool {
    git_stdout(root, &["rev-parse", "--verify", "--quiet", git_ref]).is_some()
}

fn git_upstream_ref(root: &Path) -> Option<String> {
    git_stdout(
        root,
        &[
            "rev-parse",
            "--abbrev-ref",
            "--symbolic-full-name",
            "@{upstream}",
        ],
    )
}

fn git_merge_base(root: &Path, a: &str, b: &str) -> Option<String> {
    git_stdout(root, &["merge-base", a, b])
}

fn detect_remote_default_ref(root: &Path) -> Option<String> {
    if let Some(full_ref) = git_stdout(root, &["symbolic-ref", "refs/remotes/origin/HEAD"])
        && let Some(branch) = full_ref.strip_prefix("refs/remotes/origin/")
    {
        return Some(format!("origin/{branch}"));
    }
    ["origin/main", "origin/master"]
        .into_iter()
        .find(|candidate| git_ref_exists(root, candidate))
        .map(str::to_string)
}

fn get_head_sha(root: &Path) -> Option<String> {
    git_stdout(root, &["rev-parse", "--short", "HEAD"])
}
