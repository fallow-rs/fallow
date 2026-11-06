//! Warn when a saved baseline lands inside the fallow cache directory.
//!
//! Fallow writes a `.gitignore` with `*` into its cache directory, so Git
//! ignores every file there. A baseline saved into that directory (for example
//! `--save-baseline .fallow/dead-code.json`) cannot be committed, and a CI run
//! on a fresh checkout does not find it.

use std::path::{Path, PathBuf};

/// The directory that the warning recommends for committed baselines.
const COMMITTED_BASELINE_DIR: &str = "fallow-baselines";

/// The warning for a baseline at `baseline` when it is inside `cache_dir`, or
/// `None` when it is not. A relative `baseline` is relative to the working
/// directory, like the write itself. Symlinks are resolved on both sides.
#[must_use]
fn cache_dir_warning(baseline: &Path, cache_dir: &Path, root: &Path) -> Option<String> {
    let absolute =
        std::env::current_dir().map_or_else(|_| baseline.to_path_buf(), |cwd| cwd.join(baseline));
    let resolved = crate::write_guard::resolve(&absolute);
    let cache = crate::write_guard::resolve(cache_dir);
    if !resolved.starts_with(&cache) {
        return None;
    }
    let name = baseline
        .file_name()
        .map_or_else(|| "baseline.json".into(), |name| name.to_string_lossy());
    let root = crate::write_guard::resolve(root);
    Some(format!(
        "Warning: the baseline {} is in the fallow cache directory {}. \
         Fallow tells Git to ignore all files in this directory, so you cannot commit the baseline. \
         Save the baseline to a path that you commit, for example {COMMITTED_BASELINE_DIR}/{name}.",
        display_from(&resolved, &root),
        display_from(&cache, &root),
    ))
}

/// Print a warning to stderr when `baseline` is inside `cache_dir`. A relative
/// `baseline` is relative to the working directory, like the write itself.
#[expect(
    clippy::print_stderr,
    reason = "the warning is for the person who runs the save, next to the save note"
)]
pub fn warn_if_in_cache_dir(baseline: &Path, cache_dir: &Path, root: &Path) {
    if let Some(warning) = cache_dir_warning(baseline, cache_dir, root) {
        eprintln!("{warning}");
    }
}

/// `path` relative to `root` when it is inside it, else the full path.
fn display_from(path: &Path, root: &Path) -> String {
    path.strip_prefix(root)
        .map_or_else(|_| path.to_path_buf(), PathBuf::from)
        .display()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn baseline_in_the_cache_dir_gets_a_warning() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        let cache = root.join(".fallow");
        let warning = cache_dir_warning(&cache.join("dead-code.json"), &cache, root)
            .expect("a baseline in the cache directory is flagged");
        assert!(
            warning.contains("the baseline .fallow/dead-code.json is in"),
            "{warning}"
        );
        assert!(warning.contains("cache directory .fallow."), "{warning}");
        assert!(
            warning.contains("fallow-baselines/dead-code.json"),
            "{warning}"
        );
    }

    #[test]
    fn baseline_in_a_nested_missing_cache_subdir_gets_a_warning() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        let cache = root.join(".fallow");
        let baseline = cache.join("baselines").join("dupes.json");
        assert!(cache_dir_warning(&baseline, &cache, root).is_some());
    }

    #[test]
    fn baseline_outside_the_cache_dir_gets_no_warning() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        let cache = root.join(".fallow");
        for path in [
            "fallow-baselines/dead-code.json",
            ".fallow-baseline.json",
            "dead-code.json",
        ] {
            assert_eq!(
                cache_dir_warning(&root.join(path), &cache, root),
                None,
                "{path}"
            );
        }
    }

    #[test]
    fn a_dot_dot_path_out_of_the_cache_dir_gets_no_warning() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        let cache = root.join(".fallow");
        let baseline = cache
            .join("..")
            .join("fallow-baselines")
            .join("health.json");
        assert_eq!(cache_dir_warning(&baseline, &cache, root), None);
    }

    #[test]
    fn a_custom_cache_dir_is_honoured() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        let cache = root.join(".cache").join("fallow");
        assert!(cache_dir_warning(&cache.join("x.json"), &cache, root).is_some());
        assert_eq!(
            cache_dir_warning(&root.join(".fallow/x.json"), &cache, root),
            None
        );
    }
}
