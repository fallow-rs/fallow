//! Temporary programmatic bridge for CLI-backed runtime paths.
//!
//! `fallow-api` owns the public programmatic contracts. Most NAPI calls now use
//! the API runtime directly. Health still needs the CLI implementation until
//! the health execution pipeline finishes moving behind typed engine results, so
//! this crate keeps that dependency explicit and removable.

#![cfg_attr(not(test), deny(clippy::disallowed_methods))]

use fallow_api::{ComplexityOptions, ProgrammaticError, ProgrammaticHealthRun};

/// CLI-backed health runner used by embedders during the health migration.
pub struct CliHealthRunner;

impl fallow_api::ProgrammaticHealthRunner for CliHealthRunner {
    fn run_programmatic_health(
        &self,
        options: &ComplexityOptions,
    ) -> Result<ProgrammaticHealthRun, ProgrammaticError> {
        fallow_cli::programmatic::CliProgrammaticHealthRunner.run_programmatic_health(options)
    }
}

/// Run complexity analysis through the temporary CLI-backed health bridge.
///
/// # Errors
///
/// Returns structured programmatic errors from option validation, analysis, or
/// JSON serialization.
pub fn compute_complexity(
    options: &ComplexityOptions,
) -> Result<serde_json::Value, ProgrammaticError> {
    fallow_api::compute_complexity_with_runner(options, &CliHealthRunner)
}

/// Run health analysis through the temporary CLI-backed health bridge.
///
/// # Errors
///
/// Returns structured programmatic errors from option validation, analysis, or
/// JSON serialization.
pub fn compute_health(options: &ComplexityOptions) -> Result<serde_json::Value, ProgrammaticError> {
    fallow_api::compute_health_with_runner(options, &CliHealthRunner)
}
