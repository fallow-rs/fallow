//! Runtime probes for programmatic `next_steps` output.
//!
//! Pure next-step builders live in `fallow-output`. This module owns the small
//! env/fs probes needed by the API and NAPI surfaces without depending on the
//! CLI suggestion renderer. Git-backed repo refs live in `fallow-engine`.

use std::path::Path;

use fallow_config::WorkspaceInfo;

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
    root.exists()
        && fallow_config::FallowConfig::find_config_path(root).is_none()
        && !fallow_engine::ci_env::is_ci()
}

/// Resolve a concrete `--changed-workspaces` ref for the `scope-workspaces`
/// next step, or `None` when no workspace or resolvable ref exists.
pub fn default_workspace_ref(root: &Path) -> Option<String> {
    fallow_engine::repo_refs::default_workspace_ref(root)
}

/// Resolve a concrete `--changed-workspaces` ref using already discovered
/// workspace metadata from an analysis session.
pub fn default_workspace_ref_for_workspaces(
    root: &Path,
    workspaces: &[WorkspaceInfo],
) -> Option<String> {
    fallow_engine::repo_refs::default_workspace_ref_for_workspaces(root, workspaces)
}
