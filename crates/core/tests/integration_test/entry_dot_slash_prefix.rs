//! A leading `./` on an `entry` or `dynamicallyLoaded` glob is not part of
//! the match. `./src/extra.js` and `src/extra.js` select the same file.

use super::common::create_config;
use std::path::Path;

const EXTRA: &str = "src/extra.js";

fn project() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("project");
    let root = dir.path();
    let files = [
        (
            "package.json",
            r#"{"name":"repro-dot-slash","private":true,"type":"module","main":"src/main.js"}"#,
        ),
        ("src/main.js", "export const ready = true;\n"),
        (EXTRA, "export default 'extra';\n"),
    ];
    for (path, source) in files {
        let path = root.join(path);
        std::fs::create_dir_all(path.parent().expect("parent")).expect("directory");
        std::fs::write(path, source).expect("fixture file");
    }
    dir
}

fn unused_files(root: &Path, entry: Vec<String>, dynamically_loaded: Vec<String>) -> Vec<String> {
    let mut config = create_config(root.to_path_buf());
    config.entry_patterns = entry;
    config.dynamically_loaded = dynamically_loaded;
    let mut files: Vec<String> = fallow_core::analyze(&config)
        .expect("analysis")
        .unused_files
        .iter()
        .map(|file| {
            file.file
                .path
                .strip_prefix(root)
                .expect("project-relative finding")
                .to_string_lossy()
                .replace('\\', "/")
        })
        .collect();
    files.sort();
    files
}

#[test]
fn entry_with_dot_slash_prefix_matches_the_file() {
    let dir = project();
    let unused = unused_files(dir.path(), vec![format!("./{EXTRA}")], vec![]);
    assert!(unused.is_empty(), "unexpected unused files: {unused:?}");
}

#[test]
fn dynamically_loaded_with_dot_slash_prefix_matches_the_file() {
    let dir = project();
    let unused = unused_files(dir.path(), vec![], vec![format!("./{EXTRA}")]);
    assert!(unused.is_empty(), "unexpected unused files: {unused:?}");
}

#[test]
fn entry_without_the_file_reports_it() {
    let dir = project();
    let unused = unused_files(dir.path(), vec![], vec![]);
    assert_eq!(unused, vec![EXTRA.to_string()]);
}
