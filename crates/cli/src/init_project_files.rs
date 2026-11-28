//! Project files that `fallow init` and `hooks install --target git` write
//! next to the source tree, and the guard that keeps those writes inside the
//! project.

use std::path::Path;
use std::process::ExitCode;

use crate::exit_codes::exit_invalid_input;
use crate::setup_hooks::symlink_guard::{
    OUTSIDE_LINK, OUTSIDE_LINK_REMEDY, check_refusal_message, refusal_message,
};
use crate::state_file::read_optional_bytes;

/// `path` relative to `root` when it is below the root, else in full.
pub(super) fn display_hook_path(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .map_or_else(|_| path.display().to_string(), |p| p.display().to_string())
}

/// [`refuse_outside_link`] for a Git hook in the work tree, such as a
/// `core.hooksPath` of `.githooks`. A hook under `.git`, or in the common
/// Git directory of a linked worktree, skips this check: a clone cannot
/// commit files there, and users link `.git/hooks` on purpose.
pub(super) fn refuse_tracked_hook_link(root: &Path, path: &Path) -> Result<(), ExitCode> {
    if !path.starts_with(root) || path.starts_with(root.join(".git")) {
        return Ok(());
    }
    refuse_outside_link(root, path)
}

/// Print an error and fail when `path` goes through a symlink that resolves
/// outside `root`.
pub(super) fn refuse_outside_link(root: &Path, path: &Path) -> Result<(), ExitCode> {
    refuse_with(root, path, refusal_message)
}

/// [`refuse_outside_link`] for a path that the command only checks and does
/// not write.
pub(super) fn refuse_outside_link_to_check(root: &Path, path: &Path) -> Result<(), ExitCode> {
    refuse_with(root, path, check_refusal_message)
}

/// [`refuse_outside_link`] for a config name of `init`. The name that `init`
/// writes (`fallow.toml` with `use_toml`, else `.fallowrc.json`) gets the
/// change message, and each other name the check message.
pub(super) fn refuse_config_link(root: &Path, path: &Path, use_toml: bool) -> Result<(), ExitCode> {
    let written = if use_toml {
        "fallow.toml"
    } else {
        ".fallowrc.json"
    };
    if path.file_name().is_some_and(|name| name == written) {
        refuse_outside_link(root, path)
    } else {
        refuse_outside_link_to_check(root, path)
    }
}

fn refuse_with(
    root: &Path,
    path: &Path,
    message: fn(&str, &str) -> String,
) -> Result<(), ExitCode> {
    let Some(link) = fallow_types::path_guard::outside_link(root, path) else {
        return Ok(());
    };
    eprintln!(
        "Error: {}",
        message(
            &display_hook_path(root, path),
            &display_hook_path(root, &link)
        )
    );
    Err(exit_invalid_input())
}

/// Ensure `.fallow/` is listed in the project's `.gitignore`.
///
/// If `.gitignore` exists and already contains `.fallow` (with or without
/// trailing slash), this is a no-op. Otherwise the entry is appended (or
/// the file is created).
pub(super) fn ensure_gitignore(root: &Path) {
    let gitignore_path = root.join(".gitignore");
    if let Some(link) = fallow_types::path_guard::outside_link(root, &gitignore_path) {
        eprintln!(
            "Warning: Did not update .gitignore: {} {}; {}.",
            display_hook_path(root, &link),
            OUTSIDE_LINK,
            OUTSIDE_LINK_REMEDY
        );
        return;
    }
    let mut contents = match read_optional_bytes(&gitignore_path) {
        Ok(existing) => existing.unwrap_or_default(),
        Err(e) => {
            eprintln!("Warning: Did not update .gitignore: failed to read it: {e}");
            return;
        }
    };

    let already_ignored = contents.split(|byte| *byte == b'\n').any(|line| {
        let trimmed = line.trim_ascii();
        trimmed == b".fallow" || trimmed == b".fallow/"
    });

    if already_ignored {
        return;
    }

    let is_new = contents.is_empty();
    if !is_new && !contents.ends_with(b"\n") {
        contents.push(b'\n');
    }
    contents.extend_from_slice(b".fallow/\n");

    if let Err(e) = std::fs::write(&gitignore_path, contents) {
        eprintln!("Warning: Failed to update .gitignore: {e}");
        return;
    }

    if is_new {
        eprintln!("Created .gitignore with .fallow/ entry");
    } else {
        eprintln!("Added .fallow/ to .gitignore");
    }
}

#[cfg(test)]
#[path = "init_gitignore_tests.rs"]
mod gitignore_tests;

#[cfg(all(test, unix))]
#[path = "init_symlink_tests.rs"]
mod symlink_tests;
