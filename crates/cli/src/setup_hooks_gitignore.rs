//! Add an entry to the `.gitignore` of the project, for
//! `fallow hooks install --target agent --gitignore-claude`.

use std::path::Path;

use super::read_optional_text;

/// Append `entry` to `.gitignore` under `root`. Returns `false` when the file
/// already lists the entry and nothing changed.
pub fn ensure_gitignore_entry(root: &Path, entry: &str) -> std::io::Result<bool> {
    let gitignore_path = root.join(".gitignore");
    let existing = read_optional_text(&gitignore_path)?.unwrap_or_default();
    if gitignore_lists(&existing, entry) {
        return Ok(false);
    }

    let is_new = existing.is_empty();
    let mut contents = existing;
    if !is_new && !contents.ends_with('\n') {
        contents.push('\n');
    }
    contents.push_str(entry);
    if !entry.ends_with('\n') {
        contents.push('\n');
    }
    std::fs::write(&gitignore_path, contents)?;
    Ok(true)
}

/// Whether [`ensure_gitignore_entry`] would add `entry`. Writes nothing.
pub(super) fn gitignore_needs_entry(root: &Path, entry: &str) -> std::io::Result<bool> {
    let existing = read_optional_text(&root.join(".gitignore"))?.unwrap_or_default();
    Ok(!gitignore_lists(&existing, entry))
}

fn gitignore_lists(contents: &str, entry: &str) -> bool {
    let target = entry.trim_end_matches('/');
    contents.lines().any(|line| {
        let trimmed = line.trim();
        trimmed == target || trimmed == entry
    })
}
