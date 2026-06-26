//! Temporary programmatic runner bridge for CLI-backed health execution.
//!
//! `fallow-api` owns the public programmatic contracts and serialization.
//! Health execution still needs the CLI implementation until the health
//! pipeline finishes moving behind typed engine results, so this crate exposes
//! only the removable runner adapter.

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
