//! Format detection and file selection of a `--coverage` input.

use std::path::{Path, PathBuf};

use super::coverage_v8;

/// A `--coverage` input after format detection.
pub(super) enum CoverageInput {
    Istanbul { file_path: PathBuf, json: String },
    V8 { dump_files: Vec<PathBuf> },
}

/// The files of a `--coverage` path, before any file is read.
enum SelectedCoverageFiles {
    /// One file: an Istanbul map or one V8 dump. The content decides.
    Single(PathBuf),
    /// The `NODE_V8_COVERAGE` dumps of a directory, in sorted order.
    V8Dumps(Vec<PathBuf>),
}

/// The size limit for one coverage file. V8 dumps of large test suites reach
/// hundreds of MiB, so this limit is above the default limit for project
/// files.
pub(super) const MAX_COVERAGE_FILE_BYTES: u64 = 1024 * 1024 * 1024;

/// True when `path` exists, is below `project_root`, and resolves outside it,
/// for example a committed link. A path outside the root that the user names
/// stays allowed.
pub(super) fn leaves_the_root(project_root: Option<&Path>, path: &Path) -> bool {
    project_root.is_some_and(|root| {
        path.symlink_metadata().is_ok()
            && crate::saved_file::lies_below(root, path)
            && !fallow_types::read_guard::is_inside_root(root, path)
    })
}

/// Select the files of a `--coverage` path. A directory gives its
/// `coverage-final.json` when that file exists, and its V8 dumps when it
/// does not. A file gives itself. A file below `project_root` that resolves
/// outside it is not selected.
fn select_coverage_files(
    resolved: &Path,
    project_root: Option<&Path>,
) -> Result<SelectedCoverageFiles, String> {
    if leaves_the_root(project_root, resolved) {
        return Err(format!(
            "refusing to read coverage input {}: it resolves to a location outside the project root",
            resolved.display()
        ));
    }
    if !resolved.is_dir() {
        return Ok(SelectedCoverageFiles::Single(resolved.to_path_buf()));
    }
    let candidate = resolved.join("coverage-final.json");
    if candidate.is_file() && !leaves_the_root(project_root, &candidate) {
        return Ok(SelectedCoverageFiles::Single(candidate));
    }
    let dump_files = coverage_v8::dump_files_in(resolved, project_root)?;
    if dump_files.is_empty() {
        return Err(format!(
            "no coverage-final.json or V8 coverage files found in {}",
            resolved.display()
        ));
    }
    Ok(SelectedCoverageFiles::V8Dumps(dump_files))
}

/// The files that the coverage reader reads for `resolved`, in a stable
/// order. `resolved` is an absolute path, or a path that the caller resolved
/// against the project root.
///
/// # Errors
///
/// Returns the reader error for a directory that holds no coverage file.
pub fn selected_coverage_files(resolved: &Path) -> Result<Vec<PathBuf>, String> {
    Ok(match select_coverage_files(resolved, None)? {
        SelectedCoverageFiles::Single(file) => vec![file],
        SelectedCoverageFiles::V8Dumps(files) => files,
    })
}

/// Detect the format of a `--coverage` path. A directory holds either an
/// Istanbul `coverage-final.json` (preferred) or `NODE_V8_COVERAGE` dumps; a
/// file is an Istanbul map or one V8 dump.
pub(super) fn read_coverage_input(
    resolved: &Path,
    project_root: Option<&Path>,
) -> Result<CoverageInput, String> {
    match select_coverage_files(resolved, project_root)? {
        SelectedCoverageFiles::Single(file) => read_coverage_file(file),
        SelectedCoverageFiles::V8Dumps(dump_files) => Ok(CoverageInput::V8 { dump_files }),
    }
}

fn read_coverage_file(file_path: PathBuf) -> Result<CoverageInput, String> {
    let json =
        fallow_types::read_guard::read_to_string_bounded(&file_path, MAX_COVERAGE_FILE_BYTES)
            .map_err(|e| format!("failed to read coverage file {}: {e}", file_path.display()))?;
    if coverage_v8::is_v8_dump(&json) {
        return Ok(CoverageInput::V8 {
            dump_files: vec![file_path],
        });
    }
    Ok(CoverageInput::Istanbul { file_path, json })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(dir: &Path, name: &str, content: &str) {
        std::fs::write(dir.join(name), content).expect("fixture file should be written");
    }

    #[test]
    fn a_directory_with_an_istanbul_map_selects_only_the_map() {
        let dir = tempfile::tempdir().expect("temp dir");
        write(dir.path(), "coverage-final.json", "{}");
        write(dir.path(), "coverage-1.json", r#"{"result":[]}"#);

        let files = selected_coverage_files(dir.path()).expect("selection");

        assert_eq!(files, vec![dir.path().join("coverage-final.json")]);
    }

    #[test]
    fn a_directory_without_a_map_selects_its_dumps_in_sorted_order() {
        let dir = tempfile::tempdir().expect("temp dir");
        write(dir.path(), "b.json", r#"{"result":[]}"#);
        write(dir.path(), "a.json", r#"{"result":[]}"#);
        write(dir.path(), "notes.txt", "not coverage");

        let files = selected_coverage_files(dir.path()).expect("selection");

        assert_eq!(
            files,
            vec![dir.path().join("a.json"), dir.path().join("b.json")]
        );
    }

    #[test]
    fn a_directory_without_coverage_files_is_an_error() {
        let dir = tempfile::tempdir().expect("temp dir");

        let error = selected_coverage_files(dir.path()).expect_err("empty directory");

        assert!(
            error.contains("no coverage-final.json or V8 coverage files found in"),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn a_file_selects_itself() {
        let dir = tempfile::tempdir().expect("temp dir");
        let file = dir.path().join("coverage.json");
        write(dir.path(), "coverage.json", "{}");
        let missing = dir.path().join("missing.json");

        assert_eq!(
            selected_coverage_files(&file).expect("selection"),
            vec![file]
        );
        assert_eq!(
            selected_coverage_files(&missing).expect("selection"),
            vec![missing]
        );
    }

    /// A `*.json` link in a coverage directory below the root that leaves
    /// the root is not selected, and a coverage path that is such a link is
    /// refused.
    #[test]
    #[cfg(unix)]
    fn a_dump_link_out_of_the_root_is_not_selected() {
        let dir = tempfile::tempdir().expect("temp dir");
        let root = dir.path().join("project");
        let coverage = root.join("coverage");
        let outside = dir.path().join("elsewhere");
        std::fs::create_dir_all(&coverage).expect("mkdir");
        std::fs::create_dir_all(&outside).expect("mkdir");
        write(&outside, "other.json", r#"{"result":[]}"#);
        write(&coverage, "own.json", r#"{"result":[]}"#);
        std::os::unix::fs::symlink(outside.join("other.json"), coverage.join("linked.json"))
            .expect("link");
        std::os::unix::fs::symlink(&outside, root.join("linked-dir")).expect("link");

        let Ok(SelectedCoverageFiles::V8Dumps(files)) =
            select_coverage_files(&coverage, Some(&root))
        else {
            panic!("expected V8 dumps");
        };
        assert_eq!(files, vec![coverage.join("own.json")]);
        assert!(select_coverage_files(&root.join("linked-dir"), Some(&root)).is_err());
        assert!(
            select_coverage_files(&outside, Some(&root)).is_ok(),
            "a user path outside the root stays allowed"
        );
    }
}
