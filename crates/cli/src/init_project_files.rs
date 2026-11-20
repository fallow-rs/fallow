//! Project files that `fallow init` and `hooks install --target git` write
//! next to the source tree, and the guard that keeps those writes inside the
//! project.

use std::path::Path;
use std::process::ExitCode;

use crate::exit_codes::exit_invalid_input;
use crate::setup_hooks::symlink_guard::{
    OUTSIDE_LINK, OUTSIDE_LINK_REMEDY, refusal_message, symlinked_component,
};
use crate::state_file::read_optional_bytes;

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
