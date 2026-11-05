//! Project files that `fallow init` and `hooks install --target git` write
//! next to the source tree, and the guard that keeps those writes inside the
//! project.

use std::path::Path;
use std::process::ExitCode;

use crate::exit_codes::exit_invalid_input;
use crate::setup_hooks::symlink_guard::{
    OUTSIDE_LINK, OUTSIDE_LINK_REMEDY, refusal_message, symlinked_component,
};

/// Print an error and fail when `path` goes through a symlink that resolves
/// outside `root`. The `.git/hooks` target skips this check: a clone cannot
/// commit files under `.git`, and users link `.git/hooks` on purpose.
pub(super) fn refuse_outside_link(root: &Path, path: &Path) -> Result<(), ExitCode> {
    let Some(link) = symlinked_component(root, path) else {
        return Ok(());
    };
    eprintln!(
        "Error: {}",
        refusal_message(
            &super::display_hook_path(root, path),
            &super::display_hook_path(root, &link)
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
    if let Some(link) = symlinked_component(root, &gitignore_path) {
        eprintln!(
            "Warning: Did not update .gitignore: {} {}; {}.",
            super::display_hook_path(root, &link),
            OUTSIDE_LINK,
            OUTSIDE_LINK_REMEDY
        );
        return;
    }
    let existing = std::fs::read_to_string(&gitignore_path).unwrap_or_default();

    let already_ignored = existing.lines().any(|line| {
        let trimmed = line.trim();
        trimmed == ".fallow" || trimmed == ".fallow/"
    });

    if already_ignored {
        return;
    }

    let is_new = existing.is_empty();
    let entry = if is_new || existing.ends_with('\n') {
        ".fallow/\n"
    } else {
        "\n.fallow/\n"
    };

    let mut contents = existing;
    contents.push_str(entry);

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

#[cfg(all(test, unix))]
#[path = "init_symlink_tests.rs"]
mod symlink_tests;
