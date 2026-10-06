//! Map a package manifest entry in a build output directory to its source file.
//!
//! Entry point discovery (`fallow-core`) and the public API entry set
//! (`fallow-engine`) share these helpers, so both apply the same rule.

use std::path::{Component, Path, PathBuf};

use super::types::{MISSING_ONLY_OUTPUT_DIRS, OUTPUT_DIRS};

/// Map an output directory entry to its same-stem source file.
///
/// Given `base=/project/packages/ui` and `entry=./dist/utils.js`, this tries
/// `/project/packages/ui/src/utils.<ext>` for each extension in
/// `source_extensions`. A path prefix between the package root and the output
/// directory stays: `./modules/dist/utils.js` maps to `modules/src/utils.ts`.
///
/// `OUTPUT_DIRS` decide first. A `lib/` entry (`MISSING_ONLY_OUTPUT_DIRS`)
/// maps only when the entry target is absent. The target is present when the
/// filesystem probe finds a file and `is_discovered` accepts that file. Thus a
/// hand-written `lib/` file in the analyzed file set stays the entry, and a
/// `lib/` build output that `.gitignore` or `ignorePatterns` excludes maps to
/// `src/`.
///
/// The probe and `is_discovered` run only for a `lib/` entry that has a
/// same-stem source file. Other entries never call `is_discovered`, so a
/// caller can build an expensive lookup on first use.
pub fn output_entry_to_source_path(
    base: &Path,
    entry: &str,
    source_extensions: &[&str],
    is_discovered: impl Fn(&Path) -> bool,
) -> Option<PathBuf> {
    output_dir_to_source_path(base, entry, OUTPUT_DIRS, source_extensions).or_else(|| {
        let resolved = base.join(entry);
        if is_bare_missing_only_dir(&resolved) {
            return None;
        }
        let source =
            output_dir_to_source_path(base, entry, MISSING_ONLY_OUTPUT_DIRS, source_extensions)?;
        let target_present = probed_entry_target(&resolved, source_extensions)
            .is_some_and(|target| is_discovered(&target));
        (!target_present).then_some(source)
    })
}

/// Return the first `index.<ext>` file in the directory `resolved`.
pub fn directory_index_entry(resolved: &Path, source_extensions: &[&str]) -> Option<PathBuf> {
    source_extensions
        .iter()
        .map(|ext| resolved.join(format!("index.{ext}")))
        .find(|candidate| candidate.is_file())
}

/// Map the last `dirs` component of `entry` to `src/` with the same stem.
fn output_dir_to_source_path(
    base: &Path,
    entry: &str,
    dirs: &[&str],
    source_extensions: &[&str],
) -> Option<PathBuf> {
    let components: Vec<_> = Path::new(entry).components().collect();

    let output_pos = components.iter().rposition(|component| {
        if let Component::Normal(name) = component
            && let Some(name) = name.to_str()
        {
            return dirs.contains(&name);
        }
        false
    })?;

    let prefix: PathBuf = components[..output_pos]
        .iter()
        .filter(|component| !matches!(component, Component::CurDir))
        .collect();
    let suffix: PathBuf = components[output_pos + 1..].iter().collect();

    source_extensions
        .iter()
        .map(|ext| {
            base.join(&prefix)
                .join("src")
                .join(suffix.with_extension(ext))
        })
        .find(|candidate| candidate.exists())
}

/// Return `true` when `path` ends in a missing-only output directory, such as
/// `./lib`. Such an entry has no file to map to a same-stem source file.
fn is_bare_missing_only_dir(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| MISSING_ONLY_OUTPUT_DIRS.contains(&name))
}

/// Return the file that the filesystem probe finds for `resolved`: the exact
/// file, then an extension variant, then a directory index.
fn probed_entry_target(resolved: &Path, source_extensions: &[&str]) -> Option<PathBuf> {
    if resolved.is_file() {
        return Some(resolved.to_path_buf());
    }
    source_extensions
        .iter()
        .map(|ext| resolved.with_extension(ext))
        .find(|candidate| candidate.is_file())
        .or_else(|| directory_index_entry(resolved, source_extensions))
}

#[cfg(test)]
mod tests {
    use super::*;

    const EXTS: &[&str] = &["ts", "tsx", "js"];

    fn write(path: &Path) {
        std::fs::create_dir_all(path.parent().expect("parent directory")).expect("directory");
        std::fs::write(path, "export {};\n").expect("file");
    }

    #[cfg_attr(miri, ignore)]
    #[test]
    fn missing_lib_entry_maps_to_source() {
        let dir = tempfile::tempdir().expect("temporary directory");
        write(&dir.path().join("src/index.ts"));

        assert_eq!(
            output_entry_to_source_path(dir.path(), "./lib/index.mjs", EXTS, |_| true),
            Some(dir.path().join("src/index.ts"))
        );
    }

    #[cfg_attr(miri, ignore)]
    #[test]
    fn discovered_lib_entry_stays_the_entry() {
        let dir = tempfile::tempdir().expect("temporary directory");
        write(&dir.path().join("src/index.ts"));
        write(&dir.path().join("lib/index.js"));

        assert_eq!(
            output_entry_to_source_path(dir.path(), "./lib/index.js", EXTS, |_| true),
            None
        );
        assert_eq!(
            output_entry_to_source_path(dir.path(), "./lib/index", EXTS, |_| true),
            None
        );
    }

    #[cfg_attr(miri, ignore)]
    #[test]
    fn undiscovered_lib_entry_on_disk_maps_to_source() {
        let dir = tempfile::tempdir().expect("temporary directory");
        write(&dir.path().join("src/index.ts"));
        write(&dir.path().join("lib/index.js"));

        assert_eq!(
            output_entry_to_source_path(dir.path(), "./lib/index.js", EXTS, |_| false),
            Some(dir.path().join("src/index.ts"))
        );
    }

    #[cfg_attr(miri, ignore)]
    #[test]
    fn discovered_check_runs_only_for_lib_entry_with_source() {
        let dir = tempfile::tempdir().expect("temporary directory");
        write(&dir.path().join("index.js"));
        write(&dir.path().join("lib/other.js"));
        write(&dir.path().join("lib/index.js"));
        write(&dir.path().join("src/index.ts"));
        let calls = std::cell::Cell::new(0);
        let count = |_: &Path| {
            calls.set(calls.get() + 1);
            true
        };

        assert_eq!(
            output_entry_to_source_path(dir.path(), "./index.js", EXTS, count),
            None
        );
        assert_eq!(
            output_entry_to_source_path(dir.path(), "./lib/other.js", EXTS, count),
            None
        );
        assert_eq!(calls.get(), 0, "no lib/ entry with a source file, no check");

        assert_eq!(
            output_entry_to_source_path(dir.path(), "./lib/index.js", EXTS, count),
            None
        );
        assert_eq!(calls.get(), 1);
    }

    #[cfg_attr(miri, ignore)]
    #[test]
    fn bare_lib_entry_does_not_map() {
        let dir = tempfile::tempdir().expect("temporary directory");
        write(&dir.path().join("src/index.ts"));

        assert_eq!(
            output_entry_to_source_path(dir.path(), "./lib", EXTS, |_| false),
            None
        );
    }

    #[cfg_attr(miri, ignore)]
    #[test]
    fn output_dirs_map_without_the_discovered_check() {
        let dir = tempfile::tempdir().expect("temporary directory");
        write(&dir.path().join("src/utils.ts"));
        write(&dir.path().join("dist/utils.js"));

        assert_eq!(
            output_entry_to_source_path(dir.path(), "./dist/utils.js", EXTS, |_| true),
            Some(dir.path().join("src/utils.ts"))
        );
    }
}
