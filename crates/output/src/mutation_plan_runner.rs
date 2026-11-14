//! The test runner verdict of a mutation plan.

use serde::Serialize;

/// The test runner of a plan. The plan reads the `package.json` of the
/// installed runner package and runs no code. A run that starts the runner
/// stops when `supported` is false: with `FALLOW_MUTATION_RUNNER_UNSUPPORTED`
/// for a version outside the range, and with the companion error
/// `runner-not-found` for a runner that is not installed. A run of a plan
/// with no test file or no mutant starts no runner.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct MutationPlanRunner {
    /// The runner: `vitest`, `node-test` or `bun-test`.
    pub name: String,
    /// The package or runtime that the supported range applies to.
    pub package: String,
    /// The installed version of the package. `null` when the package is not
    /// installed, or when `supported` is `null`.
    pub installed_version: Option<String>,
    /// The supported range, for example `>=4.1.0 <6.0.0`.
    pub supported_range: String,
    /// True when the installed version is in the range. False when it is
    /// outside the range or when the package is not installed. `null` for
    /// `node-test` and `bun-test`: their range applies to the runtime, and a
    /// run selects the runtime when it starts.
    pub supported: Option<bool>,
    /// The problem when `supported` is false, else `null`.
    pub message: Option<String>,
    /// The install step when `supported` is false, else `null`.
    pub help: Option<String>,
}
