//! The remote default branch fallback of `fallow hooks install`.

use std::path::Path;

use crate::validate;

/// The remote default branch name, such as `main`, for the hook fallback.
///
/// The name comes from repository metadata that a remote controls, and the
/// hook writes it into a shell script. A name that fails the ref validator is
/// dropped, so a crafted branch name cannot inject shell code into the hook.
pub(super) fn detect_default_branch(root: &Path) -> Option<String> {
    let branch = fallow_engine::repo_refs::detect_remote_default_ref(root)
        .and_then(|remote_ref| remote_ref.strip_prefix("origin/").map(str::to_owned))?;
    if let Err(e) = validate::validate_git_ref(&branch) {
        eprintln!(
            "Warning: ignoring the remote default branch name for the hook fallback: {e}. \
             Pass --branch to set the fallback explicitly."
        );
        return None;
    }
    Some(branch)
}

#[cfg(test)]
#[path = "init_hooks_tests.rs"]
mod tests;
