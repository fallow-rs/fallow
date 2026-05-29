//! Canonical process-spawning boundary for fallow's analysis crates.
//!
//! fallow's static analysis never executes code from the project under
//! analysis: it reads and parses source files, but it never runs an
//! `npm install` lifecycle script, a build step, or any other program the
//! analyzed repository could control. The single external program fallow's
//! analysis path invokes is `git` (for `--changed-since` diffing, churn
//! history, and repository-state queries), always fallow's own trusted `git`
//! on `PATH`, never a binary named by the analyzed project.
//!
//! This module is the ONLY sanctioned caller of [`std::process::Command::new`]
//! in `fallow-core`, `fallow-extract`, and `fallow-graph`. Those crates pin
//! `#![cfg_attr(not(test), deny(clippy::disallowed_methods))]` at their root and
//! `.clippy.toml` bans `std::process::Command::new`, so any new process spawn on
//! the analysis path fails the build and the author is pointed here. Routing
//! every `git` invocation through one wrapper keeps the "analysis spawns only
//! `git`" invariant machine-checkable rather than a prose promise in
//! `SECURITY.md`.

use std::process::Command;

/// Construct a `git` [`Command`] with the ambient git repository-state
/// environment stripped (see [`crate::git_env::clear_ambient_git_env`]).
///
/// This is the canonical, and only permitted, `Command::new` on fallow's
/// analysis path. Callers add `.args(...)`, `.current_dir(...)`, and so on.
#[expect(
    clippy::disallowed_methods,
    reason = "canonical git spawn wrapper: the sole Command::new permitted on the analysis path"
)]
pub fn git() -> Command {
    let mut command = Command::new("git");
    crate::git_env::clear_ambient_git_env(&mut command);
    command
}
