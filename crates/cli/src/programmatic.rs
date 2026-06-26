//! CLI-backed health runner shim for the programmatic API.
//!
//! Dead-code and duplication programmatic execution live in `fallow-api`.
//! This module remains only to adapt the still-CLI-owned health implementation
//! into the typed `ProgrammaticHealthRunner` contract.

use std::path::Path;

use fallow_config::OutputFormat;
use fallow_engine::{ProgrammaticHealthNextStepFacts, ProgrammaticHealthRun};

use crate::health::HealthOptions;

#[cfg(test)]
use fallow_api::AnalysisOptions;
use fallow_api::{
    ComplexityOptions, ProgrammaticAnalysisContext, ProgrammaticError,
    derive_complexity_run_options, resolve_programmatic_analysis_context,
};

type ProgrammaticResult<T> = Result<T, ProgrammaticError>;

fn workspace_diagnostics_for_programmatic_output(
    root: &Path,
) -> Vec<fallow_output::WorkspaceDiagnosticOutput> {
    fallow_output::workspace_diagnostics_output(crate::runtime_support::workspace_diagnostics_for(
        root,
    ))
}

fn generic_analysis_error(command: &str) -> ProgrammaticError {
    let code = format!(
        "FALLOW_{}_FAILED",
        command.replace('-', "_").to_ascii_uppercase()
    );
    ProgrammaticError::new(format!("{command} failed"), 2)
        .with_code(code)
        .with_context(format!("fallow {command}"))
        .with_help(format!(
            "Re-run `fallow {command} --format json --quiet` in the target project for CLI diagnostics"
        ))
}

fn build_complexity_options<'a>(
    resolved: &'a ProgrammaticAnalysisContext,
    options: &'a ComplexityOptions,
) -> HealthOptions<'a> {
    let run = derive_complexity_run_options(options);

    HealthOptions {
        execution: fallow_engine::HealthExecutionOptions {
            root: resolved.root(),
            config_path: resolved.config_path(),
            output: OutputFormat::Human,
            no_cache: resolved.no_cache(),
            threads: resolved.threads(),
            quiet: true,
            thresholds: run.thresholds,
            top: run.top,
            sort: run.sort,
            production: resolved.production_override().unwrap_or(false),
            production_override: resolved.production_override(),
            changed_since: resolved.changed_since(),
            diff_index: resolved.diff_index(),
            use_shared_diff_index: false,
            workspace: resolved.workspace(),
            changed_workspaces: resolved.changed_workspaces(),
            baseline: None,
            save_baseline: None,
            complexity: run.sections.complexity,
            file_scores: run.sections.file_scores,
            coverage_gaps: run.sections.coverage_gaps,
            config_activates_coverage_gaps: !run.sections.any_section,
            hotspots: run.sections.hotspots,
            ownership: run.sections.ownership,
            ownership_emails: run.ownership_emails,
            targets: run.sections.targets,
            css: run.css,
            force_full: run.sections.force_full,
            score_only_output: run.sections.score_only_output,
            enforce_coverage_gap_gate: true,
            effort: run.effort,
            score: run.sections.score,
            gates: fallow_engine::HealthGateOptions::default(),
            since: run.since,
            min_commits: run.min_commits,
            explain: resolved.explain_enabled(),
            summary: false,
            save_snapshot: None,
            trend: false,
            coverage_inputs: run.coverage_inputs,
            performance: false,
            runtime_coverage: None,
            churn_file: None,
        },
        complexity_breakdown: false,
        group_by: None,
    }
}

struct CliProgrammaticHealthRunner;

impl fallow_api::ProgrammaticHealthRunner for CliProgrammaticHealthRunner {
    fn run_programmatic_health(
        &self,
        options: &ComplexityOptions,
    ) -> ProgrammaticResult<ProgrammaticHealthRun> {
        let resolved = resolve_programmatic_analysis_context(&options.analysis)?;
        resolved.install(|| {
            let health_options = build_complexity_options(&resolved, options);
            let result = crate::health::execute_health(&health_options)
                .map_err(|_| generic_analysis_error("health"))?;
            let root = &result.config.root;
            let workspace_diagnostics = workspace_diagnostics_for_programmatic_output(root);
            let next_step_facts = ProgrammaticHealthNextStepFacts {
                suggestions_enabled: crate::report::suggestions::suggestions_enabled(),
                offer_setup: crate::report::suggestions::setup_pointer_applicable(root),
                impact_digest: crate::report::suggestions::due_impact_digest(root)
                    .map(crate::report::suggestions::impact_counts),
                audit_changed: crate::report::suggestions::audit_changed_applicable(root),
            };
            Ok(ProgrammaticHealthRun {
                analysis: result.without_group_resolver(),
                workspace_diagnostics,
                next_step_facts,
                telemetry_analysis_run_id: crate::output_runtime::telemetry_analysis_run_id(),
            })
        })
    }
}

/// Run health / complexity and return the typed API health payload.
///
/// This is a narrow compatibility shim for the temporary
/// `fallow-programmatic-cli` adapter. Public embedders should use
/// `fallow-api` contracts instead of depending on CLI runner types.
pub fn run_programmatic_health(
    options: &ComplexityOptions,
) -> ProgrammaticResult<ProgrammaticHealthRun> {
    fallow_api::ProgrammaticHealthRunner::run_programmatic_health(
        &CliProgrammaticHealthRunner,
        options,
    )
}

/// Run the health / complexity analysis and return the CLI JSON contract as a value.
#[cfg(test)]
fn compute_complexity(options: &ComplexityOptions) -> ProgrammaticResult<serde_json::Value> {
    fallow_api::run_complexity_with_runner(options, &CliProgrammaticHealthRunner)?.into_json()
}

/// Alias for `compute_complexity` with a more product-oriented name.
#[cfg(test)]
fn compute_health(options: &ComplexityOptions) -> ProgrammaticResult<serde_json::Value> {
    compute_complexity(options)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn analysis_resolve_uses_per_call_thread_pool() {
        let dir = tempfile::tempdir().expect("temp dir");
        let root = dir.path();

        let one_options = AnalysisOptions {
            root: Some(root.to_path_buf()),
            threads: Some(1),
            ..AnalysisOptions::default()
        };
        let one = resolve_programmatic_analysis_context(&one_options)
            .expect("one-thread options should resolve");
        let two_options = AnalysisOptions {
            root: Some(root.to_path_buf()),
            threads: Some(2),
            ..AnalysisOptions::default()
        };
        let two = resolve_programmatic_analysis_context(&two_options)
            .expect("two-thread options should resolve");

        assert_eq!(one.install(rayon::current_num_threads), 1);
        assert_eq!(two.install(rayon::current_num_threads), 2);
    }

    #[test]
    fn explicit_diff_file_rejects_stdin_sentinel() {
        let dir = tempfile::tempdir().expect("temp dir");
        let options = AnalysisOptions {
            root: Some(dir.path().to_path_buf()),
            diff_file: Some(PathBuf::from("-")),
            ..AnalysisOptions::default()
        };
        let Err(error) = resolve_programmatic_analysis_context(&options) else {
            panic!("stdin sentinel is not part of the programmatic API");
        };

        assert_eq!(error.code.as_deref(), Some("FALLOW_INVALID_DIFF_FILE"));
        assert_eq!(error.context.as_deref(), Some("analysis.diffFile"));
    }

    /// Minimal valid project used by the end-to-end programmatic entry points.
    fn tiny_project() -> tempfile::TempDir {
        let dir = tempfile::tempdir().expect("temp dir");
        let root = dir.path();
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(
            root.join("package.json"),
            r#"{"name":"prog-e2e","main":"src/index.ts"}"#,
        )
        .unwrap();
        std::fs::write(
            root.join("src/index.ts"),
            "export const ok = 1;\nconsole.log(ok);\n",
        )
        .unwrap();
        dir
    }

    fn analysis_at(root: &Path) -> AnalysisOptions {
        AnalysisOptions {
            root: Some(root.to_path_buf()),
            ..AnalysisOptions::default()
        }
    }

    #[test]
    fn resolve_rejects_zero_threads() {
        let options = AnalysisOptions {
            threads: Some(0),
            ..AnalysisOptions::default()
        };
        let err = resolve_programmatic_analysis_context(&options)
            .err()
            .expect("zero threads must be rejected");
        assert_eq!(err.exit_code, 2);
        assert_eq!(err.code.as_deref(), Some("FALLOW_INVALID_THREADS"));
        assert_eq!(err.context.as_deref(), Some("analysis.threads"));
    }

    #[test]
    fn resolve_rejects_mutually_exclusive_workspace_flags() {
        let options = AnalysisOptions {
            workspace: Some(vec!["packages/*".to_owned()]),
            changed_workspaces: Some("HEAD~1".to_owned()),
            ..AnalysisOptions::default()
        };
        let err = resolve_programmatic_analysis_context(&options)
            .err()
            .expect("workspace + changed_workspaces must be rejected");
        assert_eq!(err.code.as_deref(), Some("FALLOW_MUTUALLY_EXCLUSIVE_SCOPE"));
        assert_eq!(err.context.as_deref(), Some("analysis.workspace"));
    }

    #[test]
    fn resolve_rejects_nonexistent_root() {
        let options = AnalysisOptions {
            root: Some(PathBuf::from("/definitely/not/a/real/path/xyzzy")),
            ..AnalysisOptions::default()
        };
        let err = resolve_programmatic_analysis_context(&options)
            .err()
            .expect("nonexistent root must be rejected");
        assert_eq!(err.code.as_deref(), Some("FALLOW_INVALID_ROOT"));
        assert_eq!(err.context.as_deref(), Some("analysis.root"));
    }

    #[test]
    fn resolve_rejects_root_that_is_a_file() {
        let dir = tempfile::tempdir().expect("temp dir");
        let file = dir.path().join("not-a-dir.txt");
        std::fs::write(&file, "x").unwrap();
        let options = AnalysisOptions {
            root: Some(file),
            ..AnalysisOptions::default()
        };
        let err = resolve_programmatic_analysis_context(&options)
            .err()
            .expect("a file root must be rejected");
        assert_eq!(err.code.as_deref(), Some("FALLOW_INVALID_ROOT"));
    }

    #[test]
    fn resolve_rejects_nonexistent_config_path() {
        let dir = tempfile::tempdir().expect("temp dir");
        let options = AnalysisOptions {
            root: Some(dir.path().to_path_buf()),
            config_path: Some(dir.path().join("missing.fallowrc.json")),
            ..AnalysisOptions::default()
        };
        let err = resolve_programmatic_analysis_context(&options)
            .err()
            .expect("nonexistent config must be rejected");
        assert_eq!(err.code.as_deref(), Some("FALLOW_INVALID_CONFIG_PATH"));
        assert_eq!(err.context.as_deref(), Some("analysis.configPath"));
    }

    #[test]
    fn resolve_rejects_missing_diff_file() {
        let dir = tempfile::tempdir().expect("temp dir");
        let options = AnalysisOptions {
            root: Some(dir.path().to_path_buf()),
            diff_file: Some(PathBuf::from("nope.diff")),
            ..AnalysisOptions::default()
        };
        let err = resolve_programmatic_analysis_context(&options)
            .err()
            .expect("missing diff file must be rejected");
        assert_eq!(err.code.as_deref(), Some("FALLOW_INVALID_DIFF_FILE"));
        assert_eq!(err.context.as_deref(), Some("analysis.diffFile"));
    }

    #[test]
    fn resolve_rejects_diff_path_that_is_a_directory() {
        let dir = tempfile::tempdir().expect("temp dir");
        std::fs::create_dir_all(dir.path().join("a-dir")).unwrap();
        let options = AnalysisOptions {
            root: Some(dir.path().to_path_buf()),
            diff_file: Some(PathBuf::from("a-dir")),
            ..AnalysisOptions::default()
        };
        let err = resolve_programmatic_analysis_context(&options)
            .err()
            .expect("a directory diff path must be rejected");
        assert_eq!(err.code.as_deref(), Some("FALLOW_INVALID_DIFF_FILE"));
    }

    #[test]
    fn compute_health_returns_health_envelope() {
        let project = tiny_project();
        let options = ComplexityOptions {
            analysis: analysis_at(project.path()),
            ..ComplexityOptions::default()
        };
        // compute_health is a thin alias for compute_complexity.
        let json = compute_health(&options).expect("health analysis should succeed");
        assert_eq!(json["kind"], "health");
        // HealthOutput.report is `#[serde(flatten)]`, so its fields are top-level.
        assert!(json["summary"].is_object());
        assert!(json["findings"].is_array());
    }

    #[test]
    fn compute_health_css_option_returns_css_analytics() {
        let project = tempfile::tempdir().expect("temp dir");
        let root = project.path();
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(
            root.join("package.json"),
            r#"{"name":"prog-css","main":"src/index.ts","dependencies":{"tailwindcss":"4.0.0"}}"#,
        )
        .unwrap();
        std::fs::write(
            root.join("src/index.ts"),
            "import './style.css';\nexport const ok = true;\n",
        )
        .unwrap();
        std::fs::write(
            root.join("src/style.css"),
            r"
@theme {
  --color-brand: #0055cc;
}

.used { color: var(--color-brand); }
",
        )
        .unwrap();

        let json = compute_health(&ComplexityOptions {
            analysis: analysis_at(root),
            css: true,
            ..ComplexityOptions::default()
        })
        .expect("CSS health analysis should succeed");

        assert_eq!(json["kind"], "health");
        assert!(json["css_analytics"].is_object());
    }

    #[test]
    fn compute_complexity_rejects_missing_coverage_path() {
        let project = tiny_project();
        let err = compute_complexity(&ComplexityOptions {
            analysis: analysis_at(project.path()),
            coverage: Some(project.path().join("missing-coverage.json")),
            ..ComplexityOptions::default()
        })
        .expect_err("a missing coverage path must be rejected");
        assert_eq!(err.code.as_deref(), Some("FALLOW_INVALID_COVERAGE_PATH"));
        assert_eq!(err.context.as_deref(), Some("health.coverage"));
    }

    #[test]
    fn compute_complexity_rejects_relative_coverage_root() {
        let project = tiny_project();
        let err = compute_complexity(&ComplexityOptions {
            analysis: analysis_at(project.path()),
            coverage_root: Some(PathBuf::from("relative/prefix")),
            ..ComplexityOptions::default()
        })
        .expect_err("a relative coverage_root must be rejected");
        assert_eq!(err.code.as_deref(), Some("FALLOW_INVALID_COVERAGE_ROOT"));
        assert_eq!(err.context.as_deref(), Some("health.coverage_root"));
    }

    #[test]
    fn programmatic_error_builders_compose_and_display() {
        let err = ProgrammaticError::new("boom", 7)
            .with_code("FALLOW_X")
            .with_help("try again")
            .with_context("ctx.path");
        assert_eq!(err.message, "boom");
        assert_eq!(err.exit_code, 7);
        assert_eq!(err.code.as_deref(), Some("FALLOW_X"));
        assert_eq!(err.help.as_deref(), Some("try again"));
        assert_eq!(err.context.as_deref(), Some("ctx.path"));
        // Display surfaces only the message.
        assert_eq!(format!("{err}"), "boom");
    }

    #[test]
    fn generic_analysis_error_uppercases_command_into_code() {
        let err = generic_analysis_error("dead-code");
        assert_eq!(err.code.as_deref(), Some("FALLOW_DEAD_CODE_FAILED"));
        assert_eq!(err.exit_code, 2);
        assert_eq!(err.context.as_deref(), Some("fallow dead-code"));
        assert!(err.help.is_some(), "diagnostics hint should be attached");
    }
}
