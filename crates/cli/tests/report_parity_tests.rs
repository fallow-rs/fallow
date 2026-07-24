#![allow(
    clippy::expect_used,
    reason = "integration tests use expect to keep fixture setup concise"
)]

#[path = "common/mod.rs"]
mod common;

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use common::fallow_bin;

fn workspace_fixture(path: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(path)
        .canonicalize()
        .expect("fixture path")
}

fn run(root: &Path, args: &[String]) -> Output {
    Command::new(fallow_bin())
        .current_dir(root)
        .env("NO_COLOR", "1")
        .env("RUST_LOG", "")
        .args(args)
        .output()
        .expect("run fallow")
}

fn analysis_args(command: Option<&str>, root: &Path, format: &str, extra: &[&str]) -> Vec<String> {
    let mut args = Vec::new();
    if let Some(command) = command {
        args.push(command.to_string());
    }
    args.extend([
        "--root".to_string(),
        root.display().to_string(),
        "--quiet".to_string(),
        "--format".to_string(),
        format.to_string(),
    ]);
    args.extend(extra.iter().map(|value| (*value).to_string()));
    args
}

fn assert_saved_report_parity(root: &Path, command: Option<&str>) {
    assert_saved_report_parity_with_args(root, command, &[]);
}

fn assert_saved_report_parity_with_args(root: &Path, command: Option<&str>, extra: &[&str]) {
    let json = run(root, &analysis_args(command, root, "json", extra));
    assert!(
        matches!(json.status.code(), Some(0 | 1)),
        "analysis failed: {}",
        String::from_utf8_lossy(&json.stderr)
    );
    serde_json::from_slice::<serde_json::Value>(&json.stdout).expect("analysis JSON");

    let saved_dir = tempfile::tempdir().expect("saved report tempdir");
    let saved_path = saved_dir.path().join("results.json");
    std::fs::write(&saved_path, &json.stdout).expect("write saved report");

    for format in ["codeclimate", "sarif"] {
        let direct = run(root, &analysis_args(command, root, format, extra));
        assert!(
            matches!(direct.status.code(), Some(0 | 1)),
            "direct {format} failed: {}",
            String::from_utf8_lossy(&direct.stderr)
        );

        let saved = run(
            root,
            &[
                "report".to_string(),
                "--from".to_string(),
                saved_path.display().to_string(),
                "--root".to_string(),
                root.display().to_string(),
                "--quiet".to_string(),
                "--format".to_string(),
                format.to_string(),
            ],
        );
        assert!(
            saved.status.success(),
            "saved {format} failed: {}",
            String::from_utf8_lossy(&saved.stderr)
        );
        assert_eq!(
            saved.stdout, direct.stdout,
            "saved {format} must be byte-identical to direct rendering"
        );
    }
}

#[test]
fn saved_reports_preserve_native_health_duplication_and_combined_output() {
    assert_saved_report_parity(
        &workspace_fixture("tests/fixtures/complexity-project"),
        Some("health"),
    );
    assert_saved_report_parity(
        &workspace_fixture("tests/fixtures/duplicate-code"),
        Some("dupes"),
    );
    assert_saved_report_parity(
        &workspace_fixture("tests/fixtures/complexity-project"),
        None,
    );
    assert_saved_report_parity_with_args(
        &workspace_fixture("tests/fixtures/complexity-project"),
        Some("health"),
        &["--group-by", "directory"],
    );
    assert_saved_report_parity_with_args(
        &workspace_fixture("tests/fixtures/duplicate-code"),
        Some("dupes"),
        &["--group-by", "directory"],
    );
}

fn git(root: &Path, args: &[&str]) {
    let status = Command::new("git")
        .current_dir(root)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .env("GIT_AUTHOR_NAME", "Fallow Test")
        .env("GIT_AUTHOR_EMAIL", "fallow@example.invalid")
        .env("GIT_COMMITTER_NAME", "Fallow Test")
        .env("GIT_COMMITTER_EMAIL", "fallow@example.invalid")
        .args(args)
        .status()
        .expect("run git");
    assert!(status.success(), "git {args:?} failed");
}

#[test]
fn saved_audit_reports_preserve_all_native_sections() {
    let fixture = workspace_fixture("tests/fixtures/complexity-project");
    let project = tempfile::tempdir().expect("audit project");
    for entry in [
        "package.json",
        "src/index.ts",
        "src/simple.ts",
        "src/complex.ts",
    ] {
        let source = fixture.join(entry);
        let target = project.path().join(entry);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent).expect("create fixture directory");
        }
        std::fs::copy(source, target).expect("copy fixture file");
    }
    git(project.path(), &["init", "-q"]);
    git(project.path(), &["add", "."]);
    git(project.path(), &["commit", "-qm", "baseline"]);
    std::fs::copy(
        project.path().join("src/complex.ts"),
        project.path().join("src/complex-copy.ts"),
    )
    .expect("create changed duplicate");

    assert_saved_report_parity(project.path(), Some("audit"));
}

#[test]
fn saved_security_report_preserves_native_sarif_and_rejects_codeclimate() {
    let root = workspace_fixture("tests/fixtures/security-dangerous-html");
    let json = run(&root, &analysis_args(Some("security"), &root, "json", &[]));
    assert!(
        matches!(json.status.code(), Some(0 | 1)),
        "security JSON failed: {}",
        String::from_utf8_lossy(&json.stderr)
    );
    let saved_dir = tempfile::tempdir().expect("saved security tempdir");
    let saved_path = saved_dir.path().join("security.json");
    std::fs::write(&saved_path, &json.stdout).expect("write saved security report");

    let direct_sarif = run(&root, &analysis_args(Some("security"), &root, "sarif", &[]));
    let saved_sarif = run(
        &root,
        &[
            "report".to_string(),
            "--from".to_string(),
            saved_path.display().to_string(),
            "--root".to_string(),
            root.display().to_string(),
            "--quiet".to_string(),
            "--format".to_string(),
            "sarif".to_string(),
        ],
    );
    assert_eq!(saved_sarif.status.code(), direct_sarif.status.code());
    assert_eq!(saved_sarif.stdout, direct_sarif.stdout);

    let direct_codeclimate = run(
        &root,
        &analysis_args(Some("security"), &root, "codeclimate", &[]),
    );
    let saved_codeclimate = run(
        &root,
        &[
            "report".to_string(),
            "--from".to_string(),
            saved_path.display().to_string(),
            "--root".to_string(),
            root.display().to_string(),
            "--quiet".to_string(),
            "--format".to_string(),
            "codeclimate".to_string(),
        ],
    );
    assert_eq!(direct_codeclimate.status.code(), Some(2));
    assert_eq!(saved_codeclimate.status.code(), Some(2));
    assert_eq!(saved_codeclimate.stdout, direct_codeclimate.stdout);
}
