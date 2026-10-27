use std::path::{Path, PathBuf};
use std::time::Instant;

use fallow_config::{ProductionAnalysis, ResolvedConfig};
use fallow_engine::change_scope::{ChangeScope, ChangeScopeOwner};
use fallow_engine::{
    dead_code::DeadCodeAnalysisArtifacts,
    project_analysis::ProjectAnalysisArtifactOptions,
    project_config::ProjectConfigOptions,
    repo_refs::{self, ResolvedAuditBase, TemporaryBaseWorktree},
    session::AnalysisSession,
};
use fallow_output::build_audit_next_steps;
use fallow_types::{
    envelope::AuditIntroduced, output::NextStep, output_format::OutputFormat,
    results::AnalysisResults,
};
use rustc_hash::FxHashSet;

use crate::{
    AnalysisOptions, AuditAttribution, AuditOptions, AuditProgrammaticOutput, AuditSummary,
    AuditVerdict, ComplexityOptions, DeadCodeFilters, DeadCodeOptions, DuplicationOptions,
    ProgrammaticError,
    analysis_context::{
        ProgrammaticAnalysisContext, changed_files_for_run, resolve_programmatic_analysis_context,
    },
    audit_run::{
        AuditAnalyses, AuditAnalysesView, AuditBackend, AuditRun, AuditRunInput, DeadCodeView,
        DuplicationView, HealthView,
    },
};

use super::{
    ProgrammaticResult, dead_code::run_dead_code_in_context,
    duplication::run_duplication_in_context, health_may_consume_dead_code_artifacts,
    health_may_consume_duplication_report, resolve_effective_production_modes,
    run_health_for_audit, run_health_with_session_artifacts, type_aware_config,
};

/// Error code of [`run_audit`] when the project config sets an `audit`
/// baseline key, which the typed audit cannot apply. Callers that can run
/// `fallow audit` instead, such as the MCP `audit` tool, fall back on it.
pub const AUDIT_CONFIG_BASELINE_UNSUPPORTED: &str = "FALLOW_AUDIT_BASELINE_UNSUPPORTED";

/// Run changed-code audit through typed programmatic runners.
///
/// The audit itself is [`crate::audit_run::run`], the same implementation as
/// `fallow audit`. This function supplies the typed runners and builds the
/// programmatic output.
///
/// # Errors
///
/// Returns a structured error for invalid options, base-ref discovery failures,
/// unsupported CLI-only audit surfaces (including the baseline keys of the
/// `audit` config section, see [`AUDIT_CONFIG_BASELINE_UNSUPPORTED`]), or
/// analysis failures. When the environment, `audit.typeAware` or
/// `typeAware.enabled` turns on type-aware analysis that the options leave
/// off, it returns [`super::TYPE_AWARE_CONFIG_UNSUPPORTED`].
pub fn run_audit(options: &AuditOptions) -> ProgrammaticResult<AuditProgrammaticOutput> {
    validate_audit_api_options(options)?;
    let start = Instant::now();
    let resolved_base = resolve_audit_base_ref(options)?;
    let analysis = analysis_options_for_audit(options, &resolved_base.git_ref);
    let resolved = audit_section_context(&analysis)?;
    let config = load_programmatic_audit_config(&resolved)?;
    let options = &options_with_audit_config(options, &config.audit);
    let changed_files = changed_files_for_run(&resolved)?.unwrap_or_default();
    let changed_files_count = changed_files.len();

    reject_empty_selection(options)?;

    if changed_files.is_empty() {
        return Ok(empty_audit_output(
            options,
            resolved_base,
            resolved.root(),
            changed_files_count,
            start.elapsed(),
        ));
    }

    reject_config_baselines(&config.audit, options)?;
    type_aware_config::reject_config_type_aware(
        &options.analysis.type_aware,
        &resolved,
        type_aware_config::TypeAwareCommand::Audit,
    )?;
    let backend = ProgrammaticAuditBackend {
        options,
        analysis: &analysis,
        resolved: &resolved,
        config: &config,
    };
    let Some(AuditRun {
        analyses,
        outcome,
        changed_files: _,
    }) = crate::audit_run::run(
        &backend,
        AuditRunInput {
            root: resolved.root(),
            gate: options.gate.unwrap_or_default(),
            worsened: options.worsened.unwrap_or_default(),
            base_ref: &resolved_base.git_ref,
            cache_dir: Some(&config.cache_dir),
            changed_files,
            inherited_base_refs: &resolved_base.inherited_refs,
        },
    )?
    else {
        return Ok(empty_audit_output(
            options,
            resolved_base,
            resolved.root(),
            changed_files_count,
            start.elapsed(),
        ));
    };

    let mut head = analyses.subanalyses;
    if outcome.base_snapshot.is_some()
        && let Some(duplication) = head.duplication.as_mut()
    {
        for ((group, introduced), demoted) in duplication
            .output
            .report
            .clone_groups
            .iter_mut()
            .zip(outcome.comparison.dupes.introduced())
            .zip(outcome.comparison.dupes.demoted())
        {
            group.introduced = Some(AuditIntroduced(introduced));
            group.demotion_reason = demoted.then_some(crate::CloneDemotionReason::NoAddedLines);
        }
    }
    let next_steps = audit_next_steps(head.dead_code.as_ref(), head.complexity.as_ref());
    let base_snapshot = crate::audit_run::programmatic_base_snapshot(&outcome);

    Ok(AuditProgrammaticOutput {
        verdict: outcome.verdict,
        summary: outcome.summary,
        attribution: outcome.attribution,
        changed_files_count,
        base_ref: resolved_base.git_ref,
        base_description: resolved_base.description,
        head_sha: repo_refs::short_head_sha(resolved.root()),
        elapsed: start.elapsed(),
        base_snapshot_skipped: None,
        base_snapshot,
        dead_code: head.dead_code,
        duplication: head.duplication,
        complexity: head.complexity,
        diff_risk: outcome.diff_risk,
        next_steps,
        telemetry_analysis_run_id: None,
    })
}

/// The typed runners of the programmatic audit.
struct ProgrammaticAuditBackend<'a> {
    options: &'a AuditOptions,
    analysis: &'a AnalysisOptions,
    resolved: &'a ProgrammaticAnalysisContext,
    config: &'a ResolvedConfig,
}

impl<'a> AuditBackend for ProgrammaticAuditBackend<'a> {
    type Analyses = ProgrammaticAuditAnalyses<'a>;
    type Checkout = TemporaryBaseWorktree;
    type CacheKey = ();
    type Error = ProgrammaticError;

    fn run_head(
        &self,
        changed_files: &FxHashSet<PathBuf>,
    ) -> ProgrammaticResult<ProgrammaticAuditAnalyses<'a>> {
        let subanalyses = run_audit_subanalyses_with_context(
            self.options,
            self.analysis,
            self.resolved,
            Some(changed_files),
        )?;
        Ok(ProgrammaticAuditAnalyses {
            subanalyses,
            config: self.config,
        })
    }

    fn create_base_checkout(
        &self,
        base_ref: &str,
        _base_sha: Option<&str>,
    ) -> ProgrammaticResult<TemporaryBaseWorktree> {
        TemporaryBaseWorktree::create(
            self.resolved.root(),
            base_ref,
            &self.config.workspace_patterns,
        )
        .map_err(|err| {
            ProgrammaticError::new(err.to_string(), 2)
                .with_code("FALLOW_AUDIT_BASE_WORKTREE_FAILED")
                .with_context("audit.base")
        })
    }

    fn run_base(
        &self,
        base_root: &Path,
        focus: Option<&FxHashSet<PathBuf>>,
    ) -> ProgrammaticResult<ProgrammaticAuditAnalyses<'a>> {
        let head_root = self.resolved.root();
        let config_path = self
            .options
            .analysis
            .config_path
            .clone()
            .or_else(|| fallow_config::FallowConfig::find_config_path(head_root));
        let base_analysis = AnalysisOptions {
            root: Some(base_root.to_path_buf()),
            config_path,
            changed_since: None,
            explain: false,
            ..self.options.analysis.clone()
        };
        let coverage = crate::audit_run::base_coverage_inputs(
            head_root,
            self.options.coverage.as_deref(),
            self.options.coverage_root.as_deref(),
        );
        let base_options = AuditOptions {
            coverage: coverage.coverage,
            coverage_root: coverage.coverage_root,
            ..self.options.clone()
        };
        let subanalyses = run_audit_subanalyses(&base_options, &base_analysis, focus, true)?;
        Ok(ProgrammaticAuditAnalyses {
            subanalyses,
            config: self.config,
        })
    }
}

/// The typed analyses of one audit side. `config` is the head config, which
/// decides severities and styling gates.
struct ProgrammaticAuditAnalyses<'a> {
    subanalyses: AuditSubanalyses,
    config: &'a ResolvedConfig,
}

impl AuditAnalyses for ProgrammaticAuditAnalyses<'_> {
    fn view(&self) -> AuditAnalysesView<'_> {
        let AuditSubanalyses {
            dead_code,
            duplication,
            complexity,
        } = &self.subanalyses;
        AuditAnalysesView {
            dead_code: dead_code.as_ref().map(|dead_code| DeadCodeView {
                results: &dead_code.output.results,
                config: self.config,
                root: &dead_code.root,
                type_aware: dead_code
                    .output
                    .meta
                    .as_ref()
                    .and_then(|meta| meta.type_aware.as_ref()),
                syntactic_keys: None,
                public_api: None,
            }),
            duplication: duplication.as_ref().map(|duplication| DuplicationView {
                clone_groups: duplication
                    .output
                    .report
                    .clone_groups
                    .iter()
                    .map(|group| &group.group)
                    .collect(),
                root: &duplication.root,
                duplication_percentage: duplication.output.report.stats.duplication_percentage,
                threshold: duplication.threshold,
            }),
            health: complexity.as_ref().map(|complexity| HealthView {
                report: &complexity.report,
                root: &complexity.root,
                rules: &self.config.rules,
                branching: None,
            }),
        }
    }

    fn dead_code_results_mut(&mut self) -> Option<&mut AnalysisResults> {
        self.subanalyses
            .dead_code
            .as_mut()
            .map(|dead_code| &mut dead_code.output.results)
    }

    fn health_report_mut(&mut self) -> Option<&mut fallow_output::HealthReport> {
        self.subanalyses
            .complexity
            .as_mut()
            .map(|complexity| &mut complexity.report)
    }

    fn record_type_aware_warning(&mut self, warning: &str) {
        if let Some(meta) = self
            .subanalyses
            .dead_code
            .as_mut()
            .and_then(|dead_code| dead_code.output.meta.as_mut())
            .and_then(|meta| meta.type_aware.as_mut())
        {
            meta.warnings.push(warning.to_owned());
            meta.warning_count = meta.warnings.len();
        }
    }
}

/// Resolve the options against the `audit` config section with the
/// precedence of `fallow audit`. `gate` and `css_deep` win over the config,
/// as `--gate` and `--css-deep` / `--no-css-deep` do. `css` can only turn
/// styling off, as `--no-css` does: the CLI has no flag that overrides
/// `audit.css: false`.
///
/// The destructure names every field, so a new `audit` key does not compile
/// until the typed route applies it, rejects it, or states why it does not
/// apply.
fn options_with_audit_config(
    options: &AuditOptions,
    config: &fallow_config::AuditConfig,
) -> AuditOptions {
    let fallow_config::AuditConfig {
        gate,
        worsened,
        dead_code,
        health,
        dupes,
        css,
        css_deep,
        // `reject_config_baselines` refuses these keys.
        dead_code_baseline: _,
        health_baseline: _,
        dupes_baseline: _,
        // The typed audit uses a temporary base worktree, not the persistent
        // snapshot cache that this key ages out.
        cache_max_age_days: _,
        // `type_aware_config::reject_config_type_aware` refuses this key when
        // it turns on type-aware analysis.
        type_aware: _,
    } = config;
    let css = options.css != Some(false) && css.unwrap_or(true);
    let css_deep = css && options.css_deep.or(*css_deep).unwrap_or(true);
    AuditOptions {
        gate: Some(options.gate.unwrap_or(*gate)),
        worsened: Some(options.worsened.unwrap_or(*worsened)),
        dead_code: Some(
            options
                .dead_code
                .unwrap_or_else(|| dead_code.unwrap_or(true)),
        ),
        health: Some(options.health.unwrap_or_else(|| health.unwrap_or(true))),
        duplication: Some(options.duplication.unwrap_or_else(|| dupes.unwrap_or(true))),
        css: Some(css),
        css_deep: Some(css_deep),
        ..options.clone()
    }
}

/// A selection without any domain is an error, as in `fallow audit`.
fn reject_empty_selection(options: &AuditOptions) -> ProgrammaticResult<()> {
    if options.dead_code == Some(false)
        && options.health == Some(false)
        && options.duplication == Some(false)
    {
        return Err(ProgrammaticError::new(
            "this audit selects no domain: the options and the audit.deadCode, audit.health and audit.dupes config keys leave nothing to run",
            2,
        )
        .with_code("FALLOW_AUDIT_NO_ANALYSIS")
        .with_context("audit"));
    }
    Ok(())
}

/// The typed audit cannot apply the health and duplication baselines of the
/// `audit` config section. Fail instead of ignoring them: an ignored baseline
/// reports suppressed findings again and can fail the gate. The MCP `audit`
/// tool reads this code and runs the CLI, which applies the baselines. The
/// baseline of a skipped domain has no effect, as in `fallow audit`.
fn reject_config_baselines(
    config: &fallow_config::AuditConfig,
    options: &AuditOptions,
) -> ProgrammaticResult<()> {
    let keys = [
        (
            "audit.deadCodeBaseline",
            &config.dead_code_baseline,
            options.dead_code,
        ),
        (
            "audit.healthBaseline",
            &config.health_baseline,
            options.health,
        ),
        (
            "audit.dupesBaseline",
            &config.dupes_baseline,
            options.duplication,
        ),
    ];
    let Some((key, _, _)) = keys
        .iter()
        .find(|(_, value, runs)| value.is_some() && *runs != Some(false))
    else {
        return Ok(());
    };
    Err(ProgrammaticError::new(
        format!("programmatic audit does not yet support the `{key}` config key; use the CLI path"),
        2,
    )
    .with_code(AUDIT_CONFIG_BASELINE_UNSUPPORTED)
    .with_context(*key))
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
    let explicit = options
        .base
        .as_deref()
        .or(options.analysis.changed_since.as_deref());
    let root = options
        .analysis
        .root
        .clone()
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
    crate::audit_run::resolve_audit_base(&root, explicit).map_err(|error| match error {
        crate::audit_run::AuditBaseError::InvalidRef {
            origin,
            value,
            reason,
        } => ProgrammaticError::new(format!("invalid git ref `{value}`: {reason}"), 2)
            .with_code("FALLOW_INVALID_GIT_REF")
            .with_context(match origin {
                crate::audit_run::AuditBaseOrigin::Environment => "FALLOW_AUDIT_BASE",
                crate::audit_run::AuditBaseOrigin::Explicit
                | crate::audit_run::AuditBaseOrigin::Detected => "audit.base",
            }),
        crate::audit_run::AuditBaseError::NotDetected => ProgrammaticError::new(
            "could not detect base branch. Set audit.base to specify the comparison target",
            2,
        )
        .with_code("FALLOW_AUDIT_BASE_NOT_FOUND")
        .with_context("audit.base"),
    })
}

fn analysis_options_for_audit(options: &AuditOptions, base_ref: &str) -> AnalysisOptions {
    let production_override = options
        .analysis
        .production_override
        .or_else(|| options.production.then_some(true));
    AnalysisOptions {
        changed_since: Some(base_ref.to_string()),
        production: production_override.unwrap_or(options.production),
        production_override,
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
            dead_code_skipped: options.dead_code == Some(false),
            complexity_skipped: options.health == Some(false),
            duplication_skipped: options.duplication == Some(false),
            complexity_findings: 0,
            max_cyclomatic: None,
            duplication_clone_groups: 0,
        },
        attribution: AuditAttribution {
            gate: options.gate.unwrap_or_default(),
            worsened: options.worsened.unwrap_or_default(),
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
        diff_risk: fallow_output::DiffRiskReport::default(),
        next_steps: Vec::new(),
        telemetry_analysis_run_id: None,
    }
}

struct AuditSubanalyses {
    /// `None` when the audit skips the dead-code domain.
    dead_code: Option<crate::DeadCodeProgrammaticOutput>,
    /// `None` when the audit skips the duplication domain.
    duplication: Option<crate::DuplicationProgrammaticOutput>,
    /// `None` when the audit skips the health domain.
    complexity: Option<crate::HealthProgrammaticOutput>,
}

struct AuditSubanalysisOptions {
    dead_code: DeadCodeOptions,
    duplication: DuplicationOptions,
    complexity: ComplexityOptions,
}

fn audit_subanalysis_options(
    options: &AuditOptions,
    analysis: &AnalysisOptions,
    coverage_relocated: bool,
) -> AuditSubanalysisOptions {
    AuditSubanalysisOptions {
        dead_code: DeadCodeOptions {
            finding_ids: Vec::new(),
            analysis: analysis_with_production(analysis, options.production_dead_code),
            filters: DeadCodeFilters::default(),
            files: Vec::new(),
            include_entry_exports: options.include_entry_exports,
            show_cascade: options.show_cascade,
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
            coverage_relocated,
            ..ComplexityOptions::default()
        },
    }
}

/// Resolve the context of one audit side or section.
///
/// Audit compares head and base findings against its own changed files, so it
/// owns the change scope. No section reads `workspaces.changedSince`: the base
/// snapshot may not be a Git repository, and a package map that hid a base
/// finding would report the head finding as introduced.
fn audit_section_context(
    analysis: &AnalysisOptions,
) -> ProgrammaticResult<ProgrammaticAnalysisContext> {
    Ok(resolve_programmatic_analysis_context(analysis)?
        .with_change_scope_owner(ChangeScopeOwner::Caller))
}

fn run_audit_subanalyses(
    options: &AuditOptions,
    analysis: &AnalysisOptions,
    changed_files: Option<&FxHashSet<PathBuf>>,
    coverage_relocated: bool,
) -> ProgrammaticResult<AuditSubanalyses> {
    let resolved = audit_section_context(analysis)?;
    run_audit_subanalyses_in_context(
        options,
        analysis,
        &resolved,
        changed_files,
        coverage_relocated,
    )
}

fn run_audit_subanalyses_with_context(
    options: &AuditOptions,
    analysis: &AnalysisOptions,
    resolved: &ProgrammaticAnalysisContext,
    changed_files: Option<&FxHashSet<PathBuf>>,
) -> ProgrammaticResult<AuditSubanalyses> {
    run_audit_subanalyses_in_context(options, analysis, resolved, changed_files, false)
}

fn run_audit_subanalyses_in_context(
    options: &AuditOptions,
    analysis: &AnalysisOptions,
    resolved: &ProgrammaticAnalysisContext,
    changed_files: Option<&FxHashSet<PathBuf>>,
    coverage_relocated: bool,
) -> ProgrammaticResult<AuditSubanalyses> {
    let subanalysis_options = audit_subanalysis_options(options, analysis, coverage_relocated);
    let production_modes = resolve_effective_production_modes(
        resolved,
        options.production_dead_code,
        options.production_health,
        options.production_dupes,
    )?;

    // A partial selection runs each chosen domain on its own; the shared
    // sessions below assume all three domains.
    let dead_code = options.dead_code != Some(false);
    let health = options.health != Some(false);
    let duplication = options.duplication != Some(false);
    if !(dead_code && health && duplication) {
        return Ok(AuditSubanalyses {
            dead_code: dead_code
                .then(|| {
                    run_dead_code_in_context(
                        &subanalysis_options.dead_code,
                        None,
                        &audit_section_context(&subanalysis_options.dead_code.analysis)?,
                    )
                })
                .transpose()?,
            duplication: duplication
                .then(|| {
                    run_duplication_in_context(
                        &subanalysis_options.duplication,
                        &audit_section_context(&subanalysis_options.duplication.analysis)?,
                    )
                })
                .transpose()?,
            complexity: health
                .then(|| run_health_for_audit(&subanalysis_options.complexity))
                .transpose()?,
        });
    }

    if production_modes.all_match() {
        return run_shared_project_audit_subanalyses(&subanalysis_options, changed_files);
    }

    if production_modes.dead_code_matches_health() {
        return run_shared_dead_code_health_audit_subanalyses(&subanalysis_options, changed_files);
    }

    if production_modes.dead_code_matches_dupes() {
        return run_shared_dead_code_dupes_audit_subanalyses(&subanalysis_options, changed_files);
    }

    Ok(AuditSubanalyses {
        dead_code: Some(run_dead_code_in_context(
            &subanalysis_options.dead_code,
            None,
            &audit_section_context(&subanalysis_options.dead_code.analysis)?,
        )?),
        duplication: Some(run_duplication_in_context(
            &subanalysis_options.duplication,
            &audit_section_context(&subanalysis_options.duplication.analysis)?,
        )?),
        complexity: Some(run_health_for_audit(&subanalysis_options.complexity)?),
    })
}

fn run_shared_project_audit_subanalyses(
    options: &AuditSubanalysisOptions,
    changed_files: Option<&FxHashSet<PathBuf>>,
) -> ProgrammaticResult<AuditSubanalyses> {
    let resolved = audit_section_context(&options.dead_code.analysis)?;
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
    let resolved = audit_section_context(&options.dead_code.analysis)?;
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
            dead_code: Some(dead_code),
            duplication: Some(run_duplication_in_context(
                duplication_options,
                &audit_section_context(&duplication_options.analysis)?,
            )?),
            complexity: Some(complexity),
        })
    })
}

fn run_shared_dead_code_dupes_audit_subanalyses(
    options: &AuditSubanalysisOptions,
    changed_files: Option<&FxHashSet<PathBuf>>,
) -> ProgrammaticResult<AuditSubanalyses> {
    let resolved = audit_section_context(&options.dead_code.analysis)?;
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
            dead_code: Some(dead_code),
            duplication: Some(duplication),
            complexity: Some(run_health_for_audit(&options.complexity)?),
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
        // Audit attributes clone groups against its own changed files.
        &ChangeScope::default(),
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
        dead_code: Some(dead_code),
        duplication: Some(duplication),
        complexity: Some(complexity),
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

fn load_programmatic_audit_config(
    resolved: &ProgrammaticAnalysisContext,
) -> ProgrammaticResult<fallow_config::ResolvedConfig> {
    fallow_engine::project_config::config_for_project_analysis(
        resolved.root(),
        resolved.config_path().as_deref(),
        ProjectConfigOptions {
            output: OutputFormat::Json,
            no_cache: resolved.no_cache(),
            threads: resolved.threads(),
            production_override: resolved.production_override(),
            quiet: true,
            analysis: ProductionAnalysis::DeadCode,
            allow_remote_extends: resolved.allow_remote_extends(),
        },
    )
    .map(|project| project.config)
    .map_err(|err| {
        ProgrammaticError::new(format!("failed to load config: {err}"), 2)
            .with_code("FALLOW_CONFIG_LOAD_FAILED")
            .with_context("analysis.configPath")
    })
}

fn audit_next_steps(
    dead_code: Option<&crate::DeadCodeProgrammaticOutput>,
    complexity: Option<&crate::HealthProgrammaticOutput>,
) -> Vec<NextStep> {
    let input = fallow_output::build_audit_next_steps_input(
        dead_code.map(|dead_code| (&dead_code.output.results, dead_code.root.as_path())),
        complexity.map(|complexity| &complexity.report),
        crate::next_steps::suggestions_enabled(),
    );
    build_audit_next_steps(&input)
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
    fn audit_analysis_preserves_explicit_false_production_override() {
        let options = AuditOptions {
            production: false,
            analysis: AnalysisOptions {
                production: true,
                production_override: Some(false),
                ..AnalysisOptions::default()
            },
            ..AuditOptions::default()
        };

        let analysis = analysis_options_for_audit(&options, "HEAD");

        assert_eq!(analysis.production_override, Some(false));
        assert!(!analysis.production);
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
            gate: Some(AuditGate::NewOnly),
            ..AuditOptions::default()
        })
        .expect("audit output");

        assert_eq!(output.verdict, AuditVerdict::Fail);
        assert_eq!(output.summary.dead_code_issues, 1);
        assert_eq!(output.attribution.dead_code_introduced, 1);
        assert!(output.base_snapshot.is_some());

        let json = crate::serialize_audit_programmatic_json(output).expect("audit json");
        assert_eq!(json["schema_version"], fallow_output::AUDIT_SCHEMA_VERSION);
        assert_eq!(
            json["dead_code"]["unused_files"][0]["path"],
            "src/feature.ts"
        );
        assert_eq!(json["dead_code"]["unused_files"][0]["introduced"], true);
    }

    #[test]
    fn run_audit_reports_diff_risk_findings_on_the_typed_route() {
        let project = audit_fixture();
        std::fs::write(
            project.path().join("src/index.ts"),
            "// eslint-disable-next-line no-console\nconsole.log('entry');\n",
        )
        .expect("write changed entry");
        let output = run_audit(&AuditOptions {
            analysis: AnalysisOptions {
                root: Some(project.path().to_path_buf()),
                no_cache: true,
                ..AnalysisOptions::default()
            },
            base: Some("HEAD".to_string()),
            gate: Some(AuditGate::NewOnly),
            ..AuditOptions::default()
        })
        .expect("audit output");

        assert_eq!(output.diff_risk.findings.len(), 1);
        let json = crate::serialize_audit_programmatic_json(output).expect("audit json");
        assert_eq!(
            json["diff_risk"]["findings"][0]["rule"],
            "added-suppression"
        );
        assert_eq!(json["diff_risk"]["findings"][0]["path"], "src/index.ts");
        assert_eq!(json["diff_risk"]["findings"][0]["line"], 1);
        assert_eq!(json["attribution"]["diff_risk_introduced"], 1);
    }

    #[test]
    fn run_audit_warn_only_dead_code_matches_cli_verdict_semantics() {
        let project = audit_fixture();
        std::fs::write(
            project.path().join(".fallowrc.json"),
            r#"{"rules":{"unused-files":"warn"}}"#,
        )
        .expect("write config");

        let output = run_audit(&AuditOptions {
            analysis: AnalysisOptions {
                root: Some(project.path().to_path_buf()),
                no_cache: true,
                ..AnalysisOptions::default()
            },
            base: Some("HEAD".to_string()),
            gate: Some(AuditGate::All),
            ..AuditOptions::default()
        })
        .expect("audit output");

        assert_eq!(output.verdict, AuditVerdict::Warn);
        assert!(!output.summary.dead_code_has_errors);
    }

    #[test]
    fn run_audit_styling_error_matches_cli_for_new_only_and_all_gates() {
        let project = audit_styling_fixture();
        let root = project.path();
        std::fs::write(
            root.join("src/styles.css"),
            "#app .legacy .title { color: red; }\n.plain { color: blue; }\n",
        )
        .expect("write inherited-only change");

        let all = run_audit(&AuditOptions {
            analysis: AnalysisOptions {
                root: Some(root.to_path_buf()),
                no_cache: true,
                ..AnalysisOptions::default()
            },
            base: Some("HEAD".to_string()),
            gate: Some(AuditGate::All),
            ..AuditOptions::default()
        })
        .expect("all-gate audit");
        assert_eq!(all.verdict, AuditVerdict::Fail);

        let inherited_only = run_audit(&AuditOptions {
            analysis: AnalysisOptions {
                root: Some(root.to_path_buf()),
                no_cache: true,
                ..AnalysisOptions::default()
            },
            base: Some("HEAD".to_string()),
            gate: Some(AuditGate::NewOnly),
            ..AuditOptions::default()
        })
        .expect("new-only inherited audit");
        assert_eq!(inherited_only.verdict, AuditVerdict::Pass);
        assert!(inherited_only.base_snapshot.is_some());
        let inherited_json =
            crate::serialize_audit_programmatic_json(inherited_only).expect("inherited audit JSON");
        assert_eq!(
            inherited_json["complexity"]["styling_findings"][0]["introduced"],
            false
        );

        std::fs::write(
            root.join("src/styles.css"),
            "#app .legacy .title { color: red; }\n.plain { color: blue; }\n#app .introduced .title { color: green; }\n",
        )
        .expect("write introduced styling change");
        let introduced = run_audit(&AuditOptions {
            analysis: AnalysisOptions {
                root: Some(root.to_path_buf()),
                no_cache: true,
                ..AnalysisOptions::default()
            },
            base: Some("HEAD".to_string()),
            gate: Some(AuditGate::NewOnly),
            ..AuditOptions::default()
        })
        .expect("new-only introduced audit");
        assert_eq!(introduced.verdict, AuditVerdict::Fail);
        let introduced_json =
            crate::serialize_audit_programmatic_json(introduced).expect("introduced audit JSON");
        let styling = introduced_json["complexity"]["styling_findings"]
            .as_array()
            .expect("styling findings");
        assert!(
            styling
                .iter()
                .any(|finding| finding["line"] == 1 && finding["introduced"] == false)
        );
        assert!(
            styling
                .iter()
                .any(|finding| finding["line"] == 3 && finding["introduced"] == true)
        );
    }

    /// #2347: a pre-existing high-CRAP function must stay `introduced: false`
    /// when Istanbul coverage is supplied and an unrelated edit touches its
    /// file. The base snapshot analyzes a temporary worktree, so the coverage
    /// entries (recorded against the HEAD checkout) must be rebased onto that
    /// worktree; otherwise the base side falls back to the reachability
    /// estimate, scores below threshold, and the unchanged finding flips the
    /// new-only gate.
    #[test]
    fn run_audit_coverage_keeps_unchanged_function_inherited() {
        let project = tempfile::tempdir().expect("project");
        let root = project.path();
        std::fs::create_dir_all(root.join("src")).expect("create src");
        std::fs::write(
            root.join("package.json"),
            r#"{"name":"audit-api-coverage","type":"module","main":"src/index.ts","devDependencies":{"vitest":"^3.0.0"}}"#,
        )
        .expect("write package");
        std::fs::write(root.join("src/index.ts"), "console.log('entry');\n").expect("write entry");
        std::fs::write(
            root.join("src/branchy.ts"),
            "export function branchy(n: number): number {\n\
             \x20 if (n < 0) return -1;\n\
             \x20 if (n === 0) return 0;\n\
             \x20 if (n < 10) return 1;\n\
             \x20 if (n < 100) return 2;\n\
             \x20 if (n < 1000) return 3;\n\
             \x20 if (n < 10000) return 4;\n\
             \x20 return 5;\n\
             }\n",
        )
        .expect("write branchy");
        std::fs::write(
            root.join("src/branchy.test.ts"),
            "import { branchy } from './branchy';\nbranchy(1);\n",
        )
        .expect("write test reference");
        git(root, &["init"]);
        git(root, &["add", "."]);
        git(
            root,
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
        let mut source = std::fs::read_to_string(root.join("src/branchy.ts")).expect("branchy");
        source.push_str("branchy(-1);\n");
        std::fs::write(root.join("src/branchy.ts"), source).expect("append unrelated statement");

        std::fs::create_dir_all(root.join("artifacts")).expect("create artifacts");
        let recorded = root.join("src/branchy.ts");
        let recorded = recorded.to_string_lossy().replace('\\', "\\\\");
        std::fs::write(
            root.join("artifacts/coverage-final.json"),
            format!(
                r#"{{"{recorded}":{{"path":"{recorded}","statementMap":{{}},"fnMap":{{"0":{{"name":"branchy","line":1,"decl":{{"start":{{"line":1,"column":16}},"end":{{"line":1,"column":23}}}},"loc":{{"start":{{"line":1,"column":44}},"end":{{"line":9,"column":1}}}}}}}},"branchMap":{{}},"s":{{}},"f":{{"0":0}},"b":{{}}}}}}"#
            ),
        )
        .expect("write coverage");

        let output = run_audit(&AuditOptions {
            analysis: AnalysisOptions {
                root: Some(root.to_path_buf()),
                no_cache: true,
                ..AnalysisOptions::default()
            },
            base: Some("HEAD".to_string()),
            gate: Some(AuditGate::NewOnly),
            max_crap: Some(10.0),
            coverage: Some(root.join("artifacts/coverage-final.json")),
            ..AuditOptions::default()
        })
        .expect("audit output");

        assert_eq!(output.attribution.complexity_introduced, 0);
        assert_eq!(output.attribution.complexity_inherited, 1);
        assert_eq!(output.verdict, AuditVerdict::Pass);
        let json = crate::serialize_audit_programmatic_json(output).expect("audit json");
        let findings = json["complexity"]["findings"]
            .as_array()
            .expect("complexity findings");
        let branchy = findings
            .iter()
            .find(|finding| finding["name"] == "branchy")
            .expect("branchy reported above the CRAP threshold with 0% measured coverage");
        assert_eq!(branchy["introduced"], false);
        assert_eq!(branchy["coverage_source"], "istanbul");
    }

    #[test]
    fn audit_production_mode_branches_preserve_per_section_workspace_scope() {
        let project = audit_workspace_modes_fixture("");

        for mask in 0_u8..8 {
            let modes = ProductionModesMask::from(mask);
            let output = run_audit(&AuditOptions {
                production_dead_code: Some(modes.dead_code),
                production_health: Some(modes.health),
                production_dupes: Some(modes.dupes),
                ..workspace_modes_audit_options(project.path())
            })
            .unwrap_or_else(|error| panic!("audit mask {mask:03b} failed: {error}"));
            assert_audit_sections_follow_modes(output, modes, mask);
        }
    }

    /// The per-section modes may also come only from the config file. Audit
    /// must compare the modes after config resolution: with no overrides the
    /// raw options are all `None` and look equal even when the sections
    /// differ.
    #[test]
    fn audit_config_production_modes_scope_each_section() {
        for mask in 0_u8..8 {
            let modes = ProductionModesMask::from(mask);
            let project = audit_workspace_modes_fixture(&format!(
                r#","production":{{"deadCode":{},"health":{},"dupes":{}}}"#,
                modes.dead_code, modes.health, modes.dupes
            ));
            let output = run_audit(&workspace_modes_audit_options(project.path()))
                .unwrap_or_else(|error| panic!("audit mask {mask:03b} failed: {error}"));
            assert_audit_sections_follow_modes(output, modes, mask);
        }
    }

    #[derive(Clone, Copy)]
    struct ProductionModesMask {
        dead_code: bool,
        health: bool,
        dupes: bool,
    }

    impl From<u8> for ProductionModesMask {
        fn from(mask: u8) -> Self {
            Self {
                dead_code: mask & 0b001 != 0,
                health: mask & 0b010 != 0,
                dupes: mask & 0b100 != 0,
            }
        }
    }

    fn workspace_modes_audit_options(root: &Path) -> AuditOptions {
        AuditOptions {
            analysis: AnalysisOptions {
                root: Some(root.to_path_buf()),
                workspace: Some(vec!["@audit/a".to_string()]),
                no_cache: true,
                ..AnalysisOptions::default()
            },
            base: Some("HEAD".to_string()),
            gate: Some(AuditGate::All),
            include_entry_exports: true,
            ..AuditOptions::default()
        }
    }

    fn assert_audit_sections_follow_modes(
        output: AuditProgrammaticOutput,
        modes: ProductionModesMask,
        mask: u8,
    ) {
        let json = crate::serialize_audit_programmatic_json(output)
            .unwrap_or_else(|error| panic!("serialize mask {mask:03b}: {error}"));

        let dead_code = json["dead_code"].to_string();
        let complexity = json["complexity"].to_string();
        let duplication = json["duplication"].to_string();
        assert_eq!(
            dead_code.contains("mode-sentinel.test.ts"),
            !modes.dead_code,
            "dead-code scope mismatch for mask {mask:03b}: {dead_code}"
        );
        assert_eq!(
            complexity.contains("mode-sentinel.test.ts"),
            !modes.health,
            "health scope mismatch for mask {mask:03b}: {complexity}"
        );
        assert_eq!(
            duplication.contains("mode-sentinel.test.ts"),
            !modes.dupes,
            "duplication scope mismatch for mask {mask:03b}: {duplication}"
        );

        for section in [&dead_code, &complexity] {
            assert!(
                !section.contains("packages/b"),
                "workspace B leaked into mask {mask:03b}: {section}"
            );
        }
        // A clone group is in scope when one of its instances is, and it
        // keeps every instance, so a copy in workspace B may show next to
        // the copy in workspace A. No group may be only in workspace B.
        for group in json["duplication"]["clone_groups"]
            .as_array()
            .into_iter()
            .flatten()
        {
            assert!(
                group["instances"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .any(|instance| instance["file"]
                        .as_str()
                        .is_some_and(|file| file.starts_with("packages/a/"))),
                "a clone group outside workspace A leaked into mask {mask:03b}: {group}"
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
                gate: Some(AuditGate::NewOnly),
                ..AuditOptions::default()
            },
            ResolvedAuditBase {
                git_ref: "HEAD".to_string(),
                description: None,
                inherited_refs: Vec::new(),
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

    fn audit_styling_fixture() -> tempfile::TempDir {
        let project = tempfile::tempdir().expect("project");
        std::fs::create_dir_all(project.path().join("src")).expect("create src");
        std::fs::write(
            project.path().join("package.json"),
            r#"{"name":"audit-api-styling","type":"module","main":"src/index.ts"}"#,
        )
        .expect("write package");
        std::fs::write(
            project.path().join(".fallowrc.json"),
            r#"{"rules":{"css-selector-complexity":"error"}}"#,
        )
        .expect("write config");
        std::fs::write(
            project.path().join("src/index.ts"),
            "console.log('entry');\n",
        )
        .expect("write entry");
        std::fs::write(
            project.path().join("src/styles.css"),
            "#app .legacy .title { color: red; }\n",
        )
        .expect("write inherited styling");
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
        project
    }

    /// `extra_config` is appended to the top-level `.fallowrc.json` object.
    fn audit_workspace_modes_fixture(extra_config: &str) -> tempfile::TempDir {
        let project = tempfile::tempdir().expect("project");
        std::fs::write(
            project.path().join("package.json"),
            r#"{"name":"audit-root","private":true,"workspaces":["packages/*"]}"#,
        )
        .expect("write root package");
        std::fs::write(
            project.path().join(".fallowrc.json"),
            format!(
                r#"{{
  "duplicates": {{
    "minTokens": 10,
    "minLines": 2,
    "ignoreDefaults": false
  }},
  "health": {{
    "maxCyclomatic": 2,
    "maxCognitive": 2,
    "maxCrap": 2.0,
    "maxUnitSize": 3
  }}{extra_config}
}}"#
            ),
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
        let status = fallow_engine::changed_files::clear_ambient_git_env(&mut Command::new("git"))
            .args(args)
            .current_dir(root)
            .status()
            .expect("git command");
        assert!(status.success(), "git {args:?} failed");
    }

    /// #2699: the auto-detect fallthrough gets the same ref validation as the
    /// explicit and environment paths, so a malformed detection surfaces as a
    /// base-ref error instead of a changed-files failure deeper in the run.
    #[test]
    fn resolve_audit_base_ref_validates_the_auto_detected_ref() {
        let project = tempfile::tempdir().expect("temp dir");
        let root = project.path();
        std::fs::write(root.join("index.ts"), "export const used = 1;\n").expect("write entry");
        git(root, &["init", "-b", "main"]);
        git(root, &["add", "."]);
        git(
            root,
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
        git(root, &["update-ref", "refs/remotes/origin/main", "main"]);
        git(
            root,
            &[
                "symbolic-ref",
                "refs/remotes/origin/HEAD",
                "refs/remotes/origin/main",
            ],
        );
        let options = AuditOptions {
            analysis: AnalysisOptions {
                root: Some(root.to_path_buf()),
                ..AnalysisOptions::default()
            },
            ..AuditOptions::default()
        };

        let resolved = resolve_audit_base_ref(&options).expect("base ref resolves");

        assert!(
            fallow_engine::validate::validate_git_ref(&resolved.git_ref).is_ok(),
            "auto-detected ref must be usable as a git ref: {:?}",
            resolved.git_ref
        );
        assert_eq!(
            resolved.description.as_deref(),
            Some("merge-base with origin/main")
        );
    }

    fn registered_worktree_count(root: &Path) -> usize {
        let output = fallow_engine::changed_files::clear_ambient_git_env(&mut Command::new("git"))
            .args(["worktree", "list", "--porcelain"])
            .current_dir(root)
            .output()
            .expect("git worktree list");
        assert!(output.status.success(), "git worktree list failed");
        String::from_utf8_lossy(&output.stdout)
            .lines()
            .filter(|line| line.starts_with("worktree "))
            .count()
    }

    /// #3885: the base snapshot of the programmatic audit, which MCP also
    /// uses, is not registered with Git while the base analysis reads it.
    #[test]
    fn programmatic_base_snapshot_is_unregistered_during_base_analysis() {
        use crate::audit_run::AuditBackend as _;

        let project = tempfile::tempdir().expect("temp dir");
        let root = project.path();
        std::fs::write(
            root.join("package.json"),
            r#"{"name":"base-snapshot","main":"index.ts"}"#,
        )
        .expect("write manifest");
        std::fs::write(root.join("index.ts"), "export const used = 1;\n").expect("write entry");
        git(root, &["init", "-b", "main"]);
        git(root, &["add", "."]);
        git(
            root,
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
        let options = AuditOptions {
            analysis: AnalysisOptions {
                root: Some(root.to_path_buf()),
                ..AnalysisOptions::default()
            },
            ..AuditOptions::default()
        };
        let analysis = analysis_options_for_audit(&options, "HEAD");
        let resolved = audit_section_context(&analysis).expect("context resolves");
        let config = load_programmatic_audit_config(&resolved).expect("config loads");
        let backend = ProgrammaticAuditBackend {
            options: &options,
            analysis: &analysis,
            resolved: &resolved,
            config: &config,
        };

        let checkout = backend
            .create_base_checkout("HEAD", None)
            .unwrap_or_else(|_| panic!("base checkout is created"));
        assert_eq!(
            registered_worktree_count(root),
            1,
            "only the main worktree may be registered while the base analysis runs"
        );
        let base = backend
            .run_base(checkout.path(), None)
            .unwrap_or_else(|_| panic!("base analysis runs on the unregistered snapshot"));
        assert!(base.subanalyses.dead_code.is_some());
        assert_eq!(registered_worktree_count(root), 1);

        let path = checkout.path().to_path_buf();
        drop(checkout);
        assert!(!path.exists(), "the base snapshot is removed on drop");
        assert_eq!(registered_worktree_count(root), 1);
    }

    fn commit_all(root: &Path, message: &str) {
        git(root, &["add", "-f", "."]);
        git(
            root,
            &[
                "-c",
                "user.email=test@example.com",
                "-c",
                "user.name=Test",
                "-c",
                "commit.gpgsign=false",
                "commit",
                "-m",
                message,
            ],
        );
    }

    /// The base commit is untrusted. A gitdir that it commits under the name
    /// of the snapshot stub, or as a nested bare repository, must not run its
    /// config during the audit.
    #[cfg(unix)]
    #[test]
    fn audit_never_runs_a_git_config_from_the_base_commit() {
        let project = tempfile::tempdir().expect("temp dir");
        let root = project.path().join("repo");
        let marker = project.path().join("fsmonitor-ran");
        std::fs::create_dir_all(&root).expect("create repo");
        std::fs::write(
            root.join("package.json"),
            r#"{"name":"crafted-base","main":"index.ts"}"#,
        )
        .expect("write manifest");
        std::fs::write(root.join("index.ts"), "export const used = 1;\n").expect("write entry");
        for dir in ["fallow-audit-unregistered", "pkg"] {
            let gitdir = root.join(dir);
            std::fs::create_dir_all(gitdir.join("objects")).expect("create objects");
            std::fs::create_dir_all(gitdir.join("refs")).expect("create refs");
            std::fs::write(gitdir.join("objects/.keep"), "").expect("write keep");
            std::fs::write(gitdir.join("refs/.keep"), "").expect("write keep");
            std::fs::write(gitdir.join("HEAD"), "ref: refs/heads/main\n").expect("write HEAD");
            std::fs::write(
                gitdir.join("config"),
                format!(
                    "[core]\n\trepositoryformatversion = 0\n\tfsmonitor = \"touch '{}'; false\"\n",
                    marker.display()
                ),
            )
            .expect("write config");
        }
        git(&root, &["init", "-b", "main"]);
        commit_all(&root, "base");
        std::fs::write(root.join("index.ts"), "export const used = 2;\n").expect("change entry");
        commit_all(&root, "head");
        let options = AuditOptions {
            analysis: AnalysisOptions {
                root: Some(root),
                ..AnalysisOptions::default()
            },
            base: Some("HEAD~1".to_owned()),
            ..AuditOptions::default()
        };

        let output = run_audit(&options).unwrap_or_else(|_| panic!("audit runs"));

        assert!(output.base_snapshot.is_some(), "the base pass must run");
        assert!(
            !marker.exists(),
            "a Git config from the base commit ran a command"
        );
    }

    /// #3885: a programmatic audit removes the snapshots of a dead owner,
    /// registered or not, and keeps the snapshot of a live owner.
    #[test]
    fn audit_removes_snapshots_of_dead_owners_only() {
        const DEAD_PID: u32 = 99_999_998;
        assert!(!fallow_engine::repo_refs::process_is_alive(DEAD_PID));
        let project = tempfile::tempdir().expect("temp dir");
        let root = project.path().join("repo");
        std::fs::create_dir_all(&root).expect("create repo");
        std::fs::write(
            root.join("package.json"),
            r#"{"name":"orphans","main":"index.ts"}"#,
        )
        .expect("write manifest");
        std::fs::write(root.join("index.ts"), "export const used = 1;\n").expect("write entry");
        git(&root, &["init", "-b", "main"]);
        commit_all(&root, "base");
        std::fs::write(root.join("index.ts"), "export const used = 2;\n").expect("change entry");
        commit_all(&root, "head");

        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock after epoch")
            .as_nanos();
        let temp = std::env::temp_dir();
        let dead = temp.join(format!("fallow-audit-base-{DEAD_PID}-{nanos}-0"));
        let dead_registered = temp.join(format!("fallow-audit-base-{DEAD_PID}-{nanos}-1"));
        let alive = temp.join(format!(
            "fallow-audit-base-{}-{nanos}-900000",
            std::process::id()
        ));
        for dir in [&dead, &alive] {
            std::fs::create_dir_all(dir).expect("create leftover");
            std::fs::write(dir.join("index.ts"), "").expect("write leftover file");
        }
        git(
            &root,
            &[
                "worktree",
                "add",
                "--detach",
                "--quiet",
                dead_registered.to_str().expect("UTF-8 path"),
                "HEAD",
            ],
        );
        assert_eq!(registered_worktree_count(&root), 2);
        let options = AuditOptions {
            analysis: AnalysisOptions {
                root: Some(root.clone()),
                ..AnalysisOptions::default()
            },
            base: Some("HEAD~1".to_owned()),
            ..AuditOptions::default()
        };

        let output = run_audit(&options).unwrap_or_else(|_| panic!("audit runs"));

        let alive_kept = alive.is_dir();
        let _ = std::fs::remove_dir_all(&alive);
        assert!(output.base_snapshot.is_some(), "the base pass must run");
        assert!(!dead.exists(), "the leftover of a dead owner is removed");
        assert!(!dead_registered.exists());
        assert_eq!(
            registered_worktree_count(&root),
            1,
            "the registration of a dead owner is removed"
        );
        assert!(alive_kept, "the snapshot of a live owner stays");
    }
}
