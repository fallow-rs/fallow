//! Duplication runs on the files of its own production mode, as the CLI
//! `fallow dupes` does, also when the session holds the dead-code mode.
//!
//! The fixtures put the test file under `tests/`. Production mode leaves that
//! directory out, and the default duplication ignores do not, so only the
//! production mode decides whether duplication reads the file.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use fallow_config::EnvSnapshot;
use fallow_types::duplicates::CloneGroup;

use crate::project_config::config_for_project_with_env;
use crate::session::AnalysisSession;

/// A function of 40 lines, above the default token and line minimums.
fn clone_source() -> String {
    let mut source =
        String::from("export function repeated(input: number): number {\n  let total = input;\n");
    for step in 0..36 {
        writeln!(source, "  total = total * {} + {};", step + 2, step * 3 + 1)
            .expect("write to string");
    }
    source.push_str("  return total;\n}\n");
    source
}

fn write_project(root: &Path, production: &str) {
    std::fs::create_dir_all(root.join("src")).expect("src dir");
    std::fs::create_dir_all(root.join("tests")).expect("tests dir");
    std::fs::write(root.join("package.json"), "{}").expect("package.json");
    std::fs::write(
        root.join(".fallowrc.json"),
        format!(r#"{{"production":{production}}}"#),
    )
    .expect("config");
    std::fs::write(root.join("tests/a.ts"), clone_source()).expect("test file");
    std::fs::write(root.join("src/b.ts"), clone_source()).expect("source file");
}

fn clone_groups(session: &AnalysisSession) -> Vec<CloneGroup> {
    session
        .analyze_project_with(&session.config().duplicates, false)
        .expect("analysis runs")
        .duplication
        .clone_groups
}

fn instance_files(group: &CloneGroup, root: &Path) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = group
        .instances
        .iter()
        .map(|instance| {
            instance
                .file
                .strip_prefix(root)
                .unwrap_or(&instance.file)
                .to_path_buf()
        })
        .collect();
    files.sort();
    files.dedup();
    files
}

#[test]
fn duplication_uses_the_dupes_mode_when_it_is_off() {
    let dir = tempfile::tempdir().expect("temp dir");
    let root = dunce::canonicalize(dir.path()).expect("canonical root");
    write_project(&root, r#"{"deadCode":true,"dupes":false}"#);

    let session = AnalysisSession::load(&root, None).expect("session loads");
    let groups = clone_groups(&session);

    assert_eq!(groups.len(), 1, "the test file stays in duplication");
    assert_eq!(
        instance_files(&groups[0], &root),
        vec![PathBuf::from("src/b.ts"), PathBuf::from("tests/a.ts")]
    );
    assert!(
        session
            .files()
            .iter()
            .all(|file| !file.path.ends_with("tests/a.ts")),
        "dead code keeps its production file set"
    );
}

#[test]
fn duplication_skips_test_files_when_only_dupes_is_production() {
    let dir = tempfile::tempdir().expect("temp dir");
    let root = dunce::canonicalize(dir.path()).expect("canonical root");
    write_project(&root, r#"{"deadCode":false,"dupes":true}"#);

    let session = AnalysisSession::load(&root, None).expect("session loads");

    assert!(
        clone_groups(&session).is_empty(),
        "production duplication leaves the test file out"
    );
    assert!(
        session
            .files()
            .iter()
            .any(|file| file.path.ends_with("tests/a.ts")),
        "dead code keeps the test file"
    );
}

#[test]
fn duplication_scope_honors_max_file_size_env() {
    let dir = tempfile::tempdir().expect("temp dir");
    let root = dunce::canonicalize(dir.path()).expect("canonical root");
    write_project(&root, r#"{"deadCode":true,"dupes":false}"#);
    let mut big = clone_source();
    let padding = format!("// {}\n", "x".repeat(100));
    while big.len() < 3 * 1024 * 1024 / 2 {
        big.push_str(&padding);
    }
    std::fs::write(root.join("tests/big.ts"), big).expect("big test file");

    let env = EnvSnapshot::from_pairs(&[("FALLOW_MAX_FILE_SIZE", "1")], None);
    let session = AnalysisSession::from_config(
        config_for_project_with_env(&root, None, &env).expect("config loads"),
    );
    let groups = clone_groups(&session);

    assert_eq!(groups.len(), 1, "the small test file stays in duplication");
    assert_eq!(
        instance_files(&groups[0], &root),
        vec![PathBuf::from("src/b.ts"), PathBuf::from("tests/a.ts")],
        "FALLOW_MAX_FILE_SIZE keeps the large file out of duplication"
    );
}

#[test]
fn duplication_walks_again_after_a_discovery_refresh() {
    let dir = tempfile::tempdir().expect("temp dir");
    let root = dunce::canonicalize(dir.path()).expect("canonical root");
    write_project(&root, r#"{"deadCode":true,"dupes":false}"#);
    std::fs::remove_file(root.join("tests/a.ts")).expect("remove test file");

    let mut session = AnalysisSession::load(&root, None).expect("session loads");
    assert!(clone_groups(&session).is_empty(), "one copy is no clone");

    std::fs::write(root.join("tests/a.ts"), clone_source()).expect("test file");
    session.refresh_discovery();
    let groups = clone_groups(&session);

    assert_eq!(groups.len(), 1, "the refresh finds the new test file");
    assert_eq!(
        instance_files(&groups[0], &root),
        vec![PathBuf::from("src/b.ts"), PathBuf::from("tests/a.ts")]
    );
}
