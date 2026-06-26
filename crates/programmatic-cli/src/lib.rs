//! Temporary programmatic runner bridge for CLI-backed health execution.
//!
//! `fallow-api` owns the public programmatic contracts and serialization.
//! Health execution still needs the CLI implementation until the health
//! pipeline finishes moving behind typed engine results, so this crate exposes
//! only the removable runner adapter.

#![cfg_attr(not(test), deny(clippy::disallowed_methods))]
#![cfg_attr(
    test,
    allow(
        clippy::expect_used,
        reason = "tests use expect to keep fixture setup concise"
    )
)]

use fallow_api::{
    ComplexityOptions, HealthProgrammaticOutput, ProgrammaticError, ProgrammaticHealthRun,
};

/// CLI-backed health runner used by embedders during the health migration.
pub struct CliHealthRunner;

impl fallow_api::ProgrammaticHealthRunner for CliHealthRunner {
    fn run_programmatic_health(
        &self,
        options: &ComplexityOptions,
    ) -> Result<ProgrammaticHealthRun, ProgrammaticError> {
        fallow_cli::programmatic::run_programmatic_health(options)
    }
}

/// Run health / complexity and return the typed programmatic output.
///
/// This is the temporary CLI-backed health entrypoint for embedders while the
/// health pipeline finishes moving behind the engine/API boundary.
///
/// # Errors
///
/// Returns a structured programmatic error for invalid options, failed health
/// analysis, or output assembly failures.
pub fn run_complexity(
    options: &ComplexityOptions,
) -> Result<HealthProgrammaticOutput, ProgrammaticError> {
    fallow_api::run_complexity_with_runner(options, &CliHealthRunner)
}

/// Alias for [`run_complexity`] with a product-oriented name.
///
/// # Errors
///
/// Returns the same structured errors as [`run_complexity`].
pub fn run_health(
    options: &ComplexityOptions,
) -> Result<HealthProgrammaticOutput, ProgrammaticError> {
    run_complexity(options)
}

/// Run health / complexity and return the stable JSON contract.
///
/// # Errors
///
/// Returns a structured programmatic error for invalid options, failed health
/// analysis, or JSON serialization failures.
pub fn compute_complexity(
    options: &ComplexityOptions,
) -> Result<serde_json::Value, ProgrammaticError> {
    run_complexity(options)?.into_json()
}

/// Alias for [`compute_complexity`] with a product-oriented name.
///
/// # Errors
///
/// Returns the same structured errors as [`compute_complexity`].
pub fn compute_health(options: &ComplexityOptions) -> Result<serde_json::Value, ProgrammaticError> {
    compute_complexity(options)
}

#[cfg(test)]
mod tests {
    use fallow_api::{AnalysisOptions, ComplexityOptions, ProgrammaticHealthRunner};

    use super::*;

    #[test]
    fn cli_health_runner_returns_typed_programmatic_run() {
        let project = tiny_project();
        let root = project.path();

        let run = CliHealthRunner
            .run_programmatic_health(&ComplexityOptions {
                analysis: AnalysisOptions {
                    root: Some(root.to_path_buf()),
                    ..AnalysisOptions::default()
                },
                ..ComplexityOptions::default()
            })
            .expect("health run");

        assert_eq!(run.analysis.config.root, root);
        assert!(
            !run.analysis.report.findings.is_empty()
                || run.analysis.report.summary.files_analyzed >= 1
        );
    }

    #[test]
    fn run_health_returns_typed_api_output() {
        let project = tiny_project();
        let output = run_health(&ComplexityOptions {
            analysis: AnalysisOptions {
                root: Some(project.path().to_path_buf()),
                ..AnalysisOptions::default()
            },
            score: true,
            ..ComplexityOptions::default()
        })
        .expect("health output");

        assert_eq!(output.root, project.path());
        assert!(output.report.health_score.is_some());
    }

    #[test]
    fn compute_health_returns_json_contract() {
        let project = tiny_project();
        let json = compute_health(&ComplexityOptions {
            analysis: AnalysisOptions {
                root: Some(project.path().to_path_buf()),
                ..AnalysisOptions::default()
            },
            score: true,
            ..ComplexityOptions::default()
        })
        .expect("health JSON");

        assert_eq!(json["kind"], "health");
        assert_eq!(json["schema_version"], 7);
        assert!(json["health_score"].is_object());
    }

    fn tiny_project() -> tempfile::TempDir {
        let project = tempfile::tempdir().expect("temp project");
        let root = project.path();
        std::fs::create_dir_all(root.join("src")).expect("src dir");
        std::fs::write(
            root.join("package.json"),
            r#"{"name":"programmatic-cli-health","main":"src/index.ts"}"#,
        )
        .expect("package.json");
        std::fs::write(
            root.join("src/index.ts"),
            "export const ok = 1;\nconsole.log(ok);\n",
        )
        .expect("source");
        project
    }
}
