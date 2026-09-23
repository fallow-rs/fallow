//! Continuous integration detection shared by the API runtime and the CLI.
//!
//! Every surface that changes behavior in CI (next-step hints, telemetry, the
//! update check, the cache notice and the Impact record gate) reads this one
//! predicate, so the surfaces cannot disagree about what counts as CI.

/// Environment variables whose presence marks a CI run.
const CI_ENV_VARS: [&str; 3] = ["CI", "GITHUB_ACTIONS", "GITLAB_CI"];

/// Returns `true` when the process runs in CI.
///
/// The check is presence-only: `CI`, `GITHUB_ACTIONS` or `GITLAB_CI` set to any
/// value (also an empty value) marks a CI run.
#[must_use]
pub fn is_ci() -> bool {
    CI_ENV_VARS
        .iter()
        .any(|name| std::env::var_os(name).is_some())
}
