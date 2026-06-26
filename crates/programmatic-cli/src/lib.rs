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

use fallow_api::{ComplexityOptions, ProgrammaticError, ProgrammaticHealthRun};

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

#[cfg(test)]
mod tests {
    use fallow_api::{AnalysisOptions, ComplexityOptions, ProgrammaticHealthRunner};

    use super::*;

    #[test]
    fn cli_health_runner_returns_typed_programmatic_run() {
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
}
