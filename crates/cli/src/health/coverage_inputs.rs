//! Read the coverage files of `--runtime-coverage` and of coverage
//! auto-detection.
//!
//! A path below the project root must resolve inside the root, so a link that
//! the project commits cannot point the read at a file or a directory outside
//! it. A path outside the root that the user names stays allowed. Each file
//! must be a regular file of at most [`MAX_COVERAGE_FILE_BYTES`] bytes.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use fallow_cov_protocol::CoverageSource;
use fallow_engine::health::coverage_v8::source_map_cache::file_url_to_path;
use fallow_engine::saved_file::{json_error_position, lies_below};
use fallow_types::read_guard::{is_inside_root, read_to_string_bounded, read_to_string_inside};
use fallow_v8_coverage::ScriptCoverage;

/// The size limit for one coverage file. V8 dumps of large test suites reach
/// hundreds of MiB, so this limit is above the default limit for project
/// files.
pub(super) const MAX_COVERAGE_FILE_BYTES: u64 = 1024 * 1024 * 1024;

/// Fail when `path` is below `root` and resolves outside it.
pub(super) fn check_input(root: &Path, path: &Path) -> Result<(), String> {
    if is_outside_link(root, path) {
        return Err(format!(
            "refusing to read coverage input {}: it resolves to a location outside the project root",
            path.display()
        ));
    }
    Ok(())
}

/// True when `path` exists, is below `root`, and resolves outside it. A
/// relative `path` is taken relative to the working directory, as the read
/// takes it. When the working directory cannot be read, the path counts as
/// outside.
pub(super) fn is_outside_link(root: &Path, path: &Path) -> bool {
    let Ok(path) = std::path::absolute(path) else {
        return true;
    };
    path.symlink_metadata().is_ok() && lies_below(root, &path) && !is_inside_root(root, &path)
}

/// The `.json` regular files of the coverage directory `dir`, sorted, and
/// whether an entry was skipped because it is below `root` and resolves
/// outside it.
pub(super) fn coverage_json_files(root: &Path, dir: &Path) -> Result<(Vec<PathBuf>, bool), String> {
    let entries = std::fs::read_dir(dir)
        .map_err(|err| format!("failed to read coverage directory {}: {err}", dir.display()))?;
    let (refused, kept): (Vec<PathBuf>, Vec<PathBuf>) = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .partition(|entry| is_outside_link(root, entry));
    let mut files: Vec<PathBuf> = kept
        .into_iter()
        .filter(|entry| entry.extension() == Some(OsStr::new("json")) && entry.is_file())
        .collect();
    files.sort();
    Ok((files, !refused.is_empty()))
}

/// The sources of a coverage directory without `.json` files. The companion
/// reads such a directory itself, so a directory with a refused entry gives
/// no source.
pub(super) fn dir_fallback(dir: &Path, refused: bool) -> Vec<CoverageSource> {
    if refused {
        return Vec::new();
    }
    vec![CoverageSource::V8Dir {
        path: dir.to_string_lossy().into_owned(),
    }]
}

/// The text of the generated script of a V8 entry, when its `file://` URL
/// names a regular file inside `root`.
pub(super) fn generated_source(root: &Path, script: &ScriptCoverage) -> Option<String> {
    read_script_source(root, &file_url_to_path(&script.url)?)
}

/// The text of the script file `path` of a V8 dump, when it is a regular
/// file inside `root`.
pub(super) fn read_script_source(root: &Path, path: &Path) -> Option<String> {
    read_to_string_inside(root, path, MAX_COVERAGE_FILE_BYTES).ok()
}

/// Read one coverage file that [`check_input`] or [`coverage_json_files`]
/// accepted.
pub(super) fn read_coverage_file(path: &Path) -> Result<String, String> {
    read_to_string_bounded(path, MAX_COVERAGE_FILE_BYTES)
        .map_err(|err| format!("failed to read coverage file {}: {err}", path.display()))
}

/// The parse error of a coverage file, with the position only.
pub(super) fn parse_error(kind: &str, path: &Path, err: &serde_json::Error) -> String {
    format!(
        "failed to parse {kind} coverage file {}: {}",
        path.display(),
        json_error_position(err)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(unix)]
    fn refuses_coverage_links_below_the_root_that_leave_it() {
        let dir = tempfile::tempdir().expect("temp dir");
        let root = dir.path().join("project");
        let outside = dir.path().join("elsewhere");
        std::fs::create_dir_all(root.join("own")).expect("mkdir");
        std::fs::create_dir_all(&outside).expect("mkdir");
        std::fs::write(outside.join("a.json"), r#"{"result":[]}"#).expect("write");
        std::os::unix::fs::symlink(&outside, root.join("coverage")).expect("link");
        std::os::unix::fs::symlink(outside.join("a.json"), root.join("own/linked.json"))
            .expect("link");
        std::fs::write(root.join("own/b.json"), r#"{"result":[]}"#).expect("write");

        assert!(check_input(&root, &root.join("coverage")).is_err());
        assert!(
            check_input(&root, &outside).is_ok(),
            "a user path outside stays allowed"
        );

        // The same link, named by a path relative to the working directory.
        let cwd = std::env::current_dir().expect("cwd");
        let mut relative = PathBuf::new();
        for _ in cwd.components().skip(1) {
            relative.push("..");
        }
        let link = root.join("coverage");
        relative.push(link.strip_prefix("/").expect("absolute link"));
        assert!(
            check_input(&root, &relative).is_err(),
            "{}",
            relative.display()
        );
        assert_eq!(
            coverage_json_files(&root, &root.join("own")).expect("list"),
            (vec![root.join("own/b.json")], true)
        );
    }

    /// A coverage directory whose only entries link out of the project gives
    /// no source, so the companion never reads that directory.
    #[test]
    #[cfg(unix)]
    fn a_directory_with_only_refused_entries_gives_no_source() {
        let dir = tempfile::tempdir().expect("temp dir");
        let root = dir.path().join("project");
        let outside = dir.path().join("elsewhere");
        std::fs::create_dir_all(root.join("coverage")).expect("mkdir");
        std::fs::create_dir_all(root.join("plain")).expect("mkdir");
        std::fs::create_dir_all(&outside).expect("mkdir");
        std::fs::write(outside.join("dump.json"), r#"{"result":[]}"#).expect("write");
        std::os::unix::fs::symlink(outside.join("dump.json"), root.join("coverage/dump.json"))
            .expect("link");

        let (files, refused) = coverage_json_files(&root, &root.join("coverage")).expect("list");
        assert!(files.is_empty());
        assert!(dir_fallback(&root.join("coverage"), refused).is_empty());
        let (_, refused) = coverage_json_files(&root, &root.join("plain")).expect("list");
        assert_eq!(dir_fallback(&root.join("plain"), refused).len(), 1);
    }

    #[test]
    fn parse_errors_name_the_position_only() {
        let err = serde_json::from_str::<u32>(r#""hidden-value""#).unwrap_err();
        let message = parse_error("v8", Path::new("a.json"), &err);
        assert!(!message.contains("hidden-value"), "{message}");
    }
}
