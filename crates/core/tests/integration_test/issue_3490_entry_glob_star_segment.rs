//! A single `*` in `entry` and `dynamicallyLoaded` globs stays within one path
//! segment. Only `**` crosses directories, as in gitignore and picomatch.

use super::common::create_config;
use std::path::Path;

const TOP_INDEX: &str = "src/plugins/alpha/index.js";
const NESTED_INDEX: &str = "src/plugins/alpha/components/button/index.js";

fn project() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("project");
    let root = dir.path();
    let files = [
        (
            "package.json",
            r#"{"name":"repro-star","private":true,"type":"module","main":"src/main.js"}"#,
        ),
        ("src/main.js", "export const ready = true;\n"),
        (TOP_INDEX, "export default 'alpha';\n"),
        (NESTED_INDEX, "export default 'button';\n"),
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
fn dynamically_loaded_star_does_not_cross_directories() {
    let dir = project();
    let unused = unused_files(
        dir.path(),
        vec![],
        vec!["src/plugins/*/index.js".to_string()],
    );
    assert_eq!(unused, vec![NESTED_INDEX.to_string()]);
}

#[test]
fn entry_star_does_not_cross_directories() {
    let dir = project();
    let unused = unused_files(
        dir.path(),
        vec!["src/plugins/*/index.js".to_string()],
        vec![],
    );
    assert_eq!(unused, vec![NESTED_INDEX.to_string()]);
}

#[test]
fn double_star_still_crosses_directories() {
    let dir = project();
    let unused = unused_files(
        dir.path(),
        vec!["src/plugins/**/index.js".to_string()],
        vec!["src/plugins/**/index.js".to_string()],
    );
    assert!(unused.is_empty(), "unexpected unused files: {unused:?}");
}
