//! Ignore-file settings that source discovery uses.

use std::path::Path;

use ignore::WalkBuilder;

/// Make a directory walker with the ignore-file settings of source discovery.
///
/// The walker reads `.gitignore` files at each level, the global gitignore,
/// `.git/info/exclude` and `.ignore` files. Gitignore rules apply only inside a
/// git repository. Hidden entries are not skipped here: each caller filters
/// them, because source discovery admits some hidden directories.
///
/// Source discovery and each check that must agree with it start from this
/// builder, so the two cannot apply different ignore rules.
#[must_use]
pub fn source_walk_builder(root: &Path) -> WalkBuilder {
    let mut builder = WalkBuilder::new(root);
    builder
        .hidden(false)
        .git_ignore(true)
        .git_global(true)
        .git_exclude(true);
    builder
}
