//! Runtime probes for programmatic `next_steps` output.
//!
//! Pure next-step builders live in `fallow-output`. This module owns the small
//! env/fs/git probes needed by the API and NAPI surfaces without depending on
//! the CLI suggestion renderer.

use std::path::Path;

/// `FALLOW_SUGGESTIONS=off` (or `0`/`false`/`no`/`disabled`) disables the
/// `next_steps[]` array. Mirrors `report::suggestions::suggestions_enabled`.
pub fn suggestions_enabled() -> bool {
    match std::env::var("FALLOW_SUGGESTIONS").ok().as_deref() {
        Some(raw) => !matches!(
            raw.trim().to_ascii_lowercase().as_str(),
            "off" | "0" | "false" | "no" | "disabled"
        ),
        None => true,
    }
}

/// First-contact `setup` next-step gate: no fallow config up to the repo root
/// and not running in CI. The CLI additionally consults the impact store for a
/// declined-onboarding flag; that store is CLI-owned, so the API surface omits
/// it. Embedders can suppress all suggestions with `FALLOW_SUGGESTIONS`.
pub fn setup_pointer_applicable(root: &Path) -> bool {
    root.exists() && fallow_config::FallowConfig::find_config_path(root).is_none() && !is_ci()
}

/// Resolve a concrete `--changed-workspaces` ref for the `scope-workspaces`
/// next step, or `None` when no workspace or resolvable ref exists.
pub fn default_workspace_ref(root: &Path) -> Option<String> {
    if fallow_config::discover_workspaces(root).is_empty() {
        return None;
    }
    if let Some(reference) = run_git(
        root,
        &[
            "symbolic-ref",
            "--quiet",
            "--short",
            "refs/remotes/origin/HEAD",
        ],
    ) {
        let reference = reference.trim();
        if !reference.is_empty() {
            return Some(reference.to_string());
        }
    }
    ["origin/main", "origin/master"]
        .into_iter()
        .find(|candidate| git_ref_exists(root, candidate))
        .map(str::to_string)
}

fn is_ci() -> bool {
    std::env::var_os("CI").is_some()
        || std::env::var_os("GITHUB_ACTIONS").is_some()
        || std::env::var_os("GITLAB_CI").is_some()
}

fn git_ref_exists(root: &Path, reference: &str) -> bool {
    std::process::Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["rev-parse", "--verify", "--quiet", reference])
        .output()
        .is_ok_and(|output| output.status.success())
}

fn run_git(root: &Path, args: &[&str]) -> Option<String> {
    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8(output.stdout).ok()
}
