//! Warn when a saved baseline lands inside the fallow cache directory.
//!
//! Fallow writes a `.gitignore` with `*` into its cache directory, so Git
//! ignores every file there. A baseline saved into that directory (for example
//! `--save-baseline .fallow/dead-code.json`) cannot be committed, and a CI run
//! on a fresh checkout does not find it.

use std::path::Path;

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
    let root_resolved = crate::write_guard::resolve(root);
    Some(format!(
        "Warning: the baseline {} is in the fallow cache directory {}. \
         Fallow tells Git to ignore all files in this directory, so you cannot commit the baseline. \
         Save the baseline to a path that you commit, for example {COMMITTED_BASELINE_DIR}/{name}.",
        shown(&absolute, &resolved, root, &root_resolved),
        shown(cache_dir, &cache, root, &root_resolved),
    ))
}

/// A path for the warning: the resolved path relative to the resolved root
/// when it is inside the project, else the given path with each `..`
/// applied. A resolved path outside the project can name the place that a
/// linked cache directory leads to, so the warning does not show it.
fn shown(given: &Path, resolved: &Path, root: &Path, root_resolved: &Path) -> String {
    if resolved.starts_with(root_resolved) {
        return display_from(resolved, root_resolved);
    }
    display_from(&fallow_types::path_guard::lexically_normalized(given), root)
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

/// `path` relative to `root` when it is inside it, else the full path, with
/// `/` as the separator on every platform. A resolved path on Windows has the
/// `\\?\` prefix, which the full path does not show.
fn display_from(path: &Path, root: &Path) -> String {
    let shown =
        fallow_types::path_util::display_relative(dunce::simplified(root), dunce::simplified(path));
    fallow_types::display_text::display_text(&shown).into_owned()
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

    /// The warning names the paths as given, not the place that a linked
    /// cache directory leads to.
    #[cfg(unix)]
    #[test]
    fn the_warning_does_not_print_where_a_linked_cache_dir_leads() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path().join("project");
        let elsewhere = dir.path().join("target-name-6631");
        std::fs::create_dir_all(&root).expect("root");
        std::fs::create_dir_all(&elsewhere).expect("elsewhere");
        let cache = root.join(".fallow");
        std::os::unix::fs::symlink(&elsewhere, &cache).expect("link");
        let warning = cache_dir_warning(&cache.join("dead-code.json"), &cache, &root)
            .expect("a baseline in the cache directory is flagged");
        assert!(!warning.contains("target-name-6631"), "{warning}");
        assert!(
            warning.contains("the baseline .fallow/dead-code.json is in"),
            "{warning}"
        );
    }

    #[test]
    fn displayed_paths_use_forward_slashes() {
        let root = Path::new("/project");
        let baseline = root.join(".fallow\\dead-code.json");
        assert_eq!(display_from(&baseline, root), ".fallow/dead-code.json");
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
