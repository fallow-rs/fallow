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

/// Select the files of a `--coverage` path. A directory gives its
/// `coverage-final.json` when that file exists, and its V8 dumps when it
/// does not. A file gives itself.
fn select_coverage_files(resolved: &Path) -> Result<SelectedCoverageFiles, String> {
    if !resolved.is_dir() {
        return Ok(SelectedCoverageFiles::Single(resolved.to_path_buf()));
    }
    let candidate = resolved.join("coverage-final.json");
    if candidate.is_file() {
        return Ok(SelectedCoverageFiles::Single(candidate));
    }
    let dump_files = coverage_v8::dump_files_in(resolved)?;
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
    Ok(match select_coverage_files(resolved)? {
        SelectedCoverageFiles::Single(file) => vec![file],
        SelectedCoverageFiles::V8Dumps(files) => files,
    })
}

/// Detect the format of a `--coverage` path. A directory holds either an
/// Istanbul `coverage-final.json` (preferred) or `NODE_V8_COVERAGE` dumps; a
/// file is an Istanbul map or one V8 dump.
pub(super) fn read_coverage_input(resolved: &Path) -> Result<CoverageInput, String> {
    match select_coverage_files(resolved)? {
        SelectedCoverageFiles::Single(file) => read_coverage_file(file),
        SelectedCoverageFiles::V8Dumps(dump_files) => Ok(CoverageInput::V8 { dump_files }),
    }
}

fn read_coverage_file(file_path: PathBuf) -> Result<CoverageInput, String> {
    let json = std::fs::read_to_string(&file_path)
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
}
