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
const MAX_COVERAGE_FILE_BYTES: u64 = 1024 * 1024 * 1024;

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
fn is_outside_link(root: &Path, path: &Path) -> bool {
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

/// The files that replace a V8 dump for the companion: the remapped
/// Istanbul file and the residual V8 file, each when it has content.
pub(super) type PreparedDump = (Option<PathBuf>, Option<PathBuf>);

/// The scripts of a V8 dump that the companion may read, and the count of
/// the other scripts. A script with a local path (a `file://` URL or an
/// absolute path) must resolve inside `root`. A missing file must lie below
/// `root` by its path. A script with another URL, such as `node:fs`, stays.
pub(super) fn scripts_inside_root(
    root: &Path,
    scripts: Vec<ScriptCoverage>,
) -> (Vec<ScriptCoverage>, usize) {
    let (kept, dropped): (Vec<ScriptCoverage>, Vec<ScriptCoverage>) =
        scripts.into_iter().partition(|script| {
            file_url_to_path(&script.url).is_none_or(|path| script_inside_root(root, &path))
        });
    if !dropped.is_empty() {
        tracing::debug!(
            dropped = dropped.len(),
            "dropped V8 coverage scripts outside the project root"
        );
    }
    (kept, dropped.len())
}

fn script_inside_root(root: &Path, path: &Path) -> bool {
    if path.symlink_metadata().is_ok() {
        return is_inside_root(root, path);
    }
    let parent_free = path
        .components()
        .all(|component| component != std::path::Component::ParentDir);
    parent_free && lies_below(root, path)
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

    /// The companion gets only the scripts of a V8 dump inside the root.
    #[test]
    #[cfg_attr(miri, ignore = "tempdir is blocked by Miri isolation")]
    fn v8_scripts_outside_the_root_do_not_reach_the_companion() {
        let dir = tempfile::tempdir().expect("temp dir");
        let base = dunce::canonicalize(dir.path()).expect("canonical");
        let root = base.join("project");
        std::fs::create_dir_all(root.join("dist")).expect("mkdir");
        std::fs::write(root.join("dist/app.js"), "").expect("write");
        std::fs::write(base.join("other.js"), "").expect("write");
        let url = |path: &Path| {
            url::Url::from_file_path(path)
                .expect("file url")
                .to_string()
        };
        let script = |url: String| {
            serde_json::json!({
                "scriptId": "1",
                "url": url,
                "functions": [{
                    "functionName": "f",
                    "ranges": [{"startOffset": 0, "endOffset": 1, "count": 1}],
                    "isBlockCoverage": false
                }]
            })
        };
        let dump = root.join("coverage.json");
        let json = serde_json::json!({
            "result": [script(url(&root.join("dist/app.js"))), script(url(&base.join("other.js")))]
        });
        std::fs::write(&dump, json.to_string()).expect("write");

        let prepared =
            super::super::coverage::prepare_coverage_sources(&root, &dump).expect("prepare");
        let mut urls = Vec::new();
        for source in &prepared.sources {
            if let CoverageSource::V8 { path } = source {
                assert_ne!(Path::new(path), dump, "the raw dump is not handed over");
                let text = std::fs::read_to_string(path).expect("read");
                let value: serde_json::Value = serde_json::from_str(&text).expect("parse");
                for entry in value["result"].as_array().expect("result") {
                    urls.push(entry["url"].as_str().expect("url").to_owned());
                }
            }
        }
        assert_eq!(urls, vec![url(&root.join("dist/app.js"))]);
    }

    #[test]
    fn script_filter_keeps_missing_files_below_the_root_and_other_urls() {
        let root = Path::new("/project");
        let script = |url: &str| ScriptCoverage {
            script_id: "1".to_owned(),
            url: url.to_owned(),
            functions: Vec::new(),
        };
        let (kept, dropped) = scripts_inside_root(
            root,
            vec![
                script("file:///project/dist/missing.js"),
                script("node:fs"),
                script("file:///project/../elsewhere/a.js"),
                script("/elsewhere/b.js"),
                script("/project/dist/../c.js"),
            ],
        );
        let urls: Vec<&str> = kept.iter().map(|script| script.url.as_str()).collect();
        assert_eq!(urls, ["file:///project/dist/missing.js", "node:fs"]);
        assert_eq!(dropped, 3);
        assert_eq!(
            file_url_to_path("/project/dist/../c.js"),
            Some(PathBuf::from("/project/dist/../c.js")),
            "the parser keeps the raw `..` segments of a plain path"
        );
    }

    #[test]
    fn parse_errors_name_the_position_only() {
        let err = serde_json::from_str::<u32>(r#""hidden-value""#).unwrap_err();
        let message = parse_error("v8", Path::new("a.json"), &err);
        assert!(!message.contains("hidden-value"), "{message}");
    }
}
