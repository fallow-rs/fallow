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

fn run_with_env(root: &Path, args: &[String], env: &[(&str, &str)]) -> Output {
    let mut command = Command::new(fallow_bin());
    command
        .current_dir(root)
        .env("NO_COLOR", "1")
        .env("RUST_LOG", "")
        .args(args);
    for (name, value) in env {
        command.env(name, value);
    }
    command.output().expect("run fallow with env")
}

fn run_with_type_aware_sidecar(root: &Path, args: &[String]) -> Output {
    let mut command = Command::new(fallow_bin());
    command
        .current_dir(root)
        .env("NO_COLOR", "1")
        .env("RUST_LOG", "")
        .args(args);
    common::configure_type_aware_sidecar(&mut command);
    command.output().expect("run type-aware fallow")
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

    let formats: &[&str] = if command.is_some() {
        &[
            "codeclimate",
            "sarif",
            "pr-comment-github",
            "pr-comment-gitlab",
            "review-github",
            "review-gitlab",
        ]
    } else {
        // Combined comments use their richer multi-gate presentation while
        // the saved generic renderer preserves the same typed findings.
        &["codeclimate", "sarif"]
    };
    for format in formats {
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
        &workspace_fixture("tests/fixtures/basic-project"),
        Some("check"),
    );
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
    assert_saved_report_parity_with_args(
        &workspace_fixture("tests/fixtures/basic-project"),
        Some("check"),
        &["--group-by", "directory"],
    );
}

#[test]
fn saved_owner_grouped_dead_code_matches_direct_rendering() {
    let project = tempfile::tempdir().expect("owner-grouped project");
    let root = project.path();
    std::fs::create_dir(root.join("src")).expect("create source directory");
    std::fs::write(
        root.join("package.json"),
        r#"{"name":"owner-grouped-parity","private":true,"main":"src/index.ts"}"#,
    )
    .expect("write manifest");
    std::fs::write(
        root.join("CODEOWNERS"),
        "src/a.ts @z-owner\nsrc/b.ts @a-owner\nsrc/c.ts @z-owner\n",
    )
    .expect("write owners");
    std::fs::write(root.join("src/index.ts"), "export const entry = true;\n")
        .expect("write entrypoint");
    for name in ["a", "b", "c"] {
        std::fs::write(
            root.join(format!("src/{name}.ts")),
            format!("export const {name} = true;\n"),
        )
        .expect("write unused source");
    }

    assert_saved_report_parity_with_args(root, Some("check"), &["--group-by", "owner"]);
}

#[test]
fn saved_gitlab_surfaces_match_direct_type_aware_findings() {
    let root = workspace_fixture("tests/fixtures/type-aware-unused-export-refinement");
    let extra = ["--type-aware", "--unused-exports", "--unused-types"];
    let json =
        run_with_type_aware_sidecar(&root, &analysis_args(Some("check"), &root, "json", &extra));
    assert!(matches!(json.status.code(), Some(0 | 1)));
    let saved_dir = tempfile::tempdir().expect("saved type-aware directory");
    let saved_path = saved_dir.path().join("results.json");
    std::fs::write(&saved_path, &json.stdout).expect("write type-aware results");

    for format in ["pr-comment-gitlab", "review-gitlab"] {
        let direct = run_with_type_aware_sidecar(
            &root,
            &analysis_args(Some("check"), &root, format, &extra),
        );
        assert!(matches!(direct.status.code(), Some(0 | 1)), "{format}");
        let saved = run(
            &root,
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
        assert!(saved.status.success(), "{format}");
        assert_eq!(saved.stdout, direct.stdout, "{format}");
        let rendered = String::from_utf8_lossy(&saved.stdout);
        assert!(!rendered.contains("PublicApi"), "{format}: {rendered}");
        assert!(rendered.contains("actuallyUnused"), "{format}: {rendered}");
    }
}

#[test]
fn saved_dead_code_comment_preserves_direct_decision_sidecar() {
    let root = workspace_fixture("tests/fixtures/basic-project");
    let json = run(&root, &analysis_args(Some("check"), &root, "json", &[]));
    assert!(matches!(json.status.code(), Some(0 | 1)));
    let saved_dir = tempfile::tempdir().expect("saved sidecar directory");
    let saved_path = saved_dir.path().join("results.json");
    std::fs::write(&saved_path, &json.stdout).expect("write saved results");
    let direct_decision = saved_dir.path().join("direct-decision.json");
    let saved_decision = saved_dir.path().join("saved-decision.json");

    let direct = run_with_env(
        &root,
        &analysis_args(Some("check"), &root, "pr-comment-gitlab", &[]),
        &[(
            "FALLOW_PR_DECISION_FILE",
            direct_decision.to_str().expect("utf8"),
        )],
    );
    assert!(matches!(direct.status.code(), Some(0 | 1)));
    let rendered = run_with_env(
        &root,
        &[
            "report".to_owned(),
            "--from".to_owned(),
            saved_path.display().to_string(),
            "--root".to_owned(),
            root.display().to_string(),
            "--quiet".to_owned(),
            "--format".to_owned(),
            "pr-comment-gitlab".to_owned(),
        ],
        &[(
            "FALLOW_PR_DECISION_FILE",
            saved_decision.to_str().expect("utf8"),
        )],
    );
    assert!(rendered.status.success());

    let direct_sidecar: serde_json::Value =
        serde_json::from_slice(&std::fs::read(direct_decision).expect("read direct decision"))
            .expect("parse direct decision");
    let saved_sidecar: serde_json::Value =
        serde_json::from_slice(&std::fs::read(saved_decision).expect("read saved decision"))
            .expect("parse saved decision");
    assert_eq!(saved_sidecar, direct_sidecar);
    assert_eq!(saved_sidecar["gates"][0]["id"], "dead-code");
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
fn saved_audit_verdict_survives_review_rendering() {
    let root = workspace_fixture("tests/fixtures/basic-project");
    let saved_dir = tempfile::tempdir().expect("saved audit verdict directory");
    for (verdict, summary) in [
        ("pass", "Quality gate passed"),
        ("warn", "Review needed"),
        ("fail", "Quality gate failed"),
    ] {
        let path = saved_dir.path().join(format!("audit-{verdict}.json"));
        std::fs::write(
            &path,
            serde_json::to_vec(&serde_json::json!({
                "kind": "audit",
                "schema_version": fallow_output::AUDIT_SCHEMA_VERSION,
                "version": env!("CARGO_PKG_VERSION"),
                "command": "audit",
                "verdict": verdict,
                "changed_files_count": 0,
                "base_ref": "main",
                "elapsed_ms": 0,
                "summary": {
                    "dead_code_issues": 0,
                    "dead_code_has_errors": false,
                    "complexity_findings": 0,
                    "max_cyclomatic": null,
                    "duplication_clone_groups": 0
                },
                "attribution": {
                    "gate": "new-only",
                    "dead_code_introduced": 0,
                    "dead_code_inherited": 0,
                    "complexity_introduced": 0,
                    "complexity_inherited": 0,
                    "duplication_introduced": 0,
                    "duplication_inherited": 0,
                    "styling_introduced": 0,
                    "styling_inherited": 0,
                    "duplication_demoted": 0
                }
            }))
            .expect("serialize audit envelope"),
        )
        .expect("write audit envelope");
        let rendered = run(
            &root,
            &[
                "report".to_string(),
                "--from".to_string(),
                path.display().to_string(),
                "--root".to_string(),
                root.display().to_string(),
                "--quiet".to_string(),
                "--format".to_string(),
                "review-gitlab".to_string(),
            ],
        );
        assert!(rendered.status.success(), "{verdict}");
        let envelope: serde_json::Value =
            serde_json::from_slice(&rendered.stdout).expect("review envelope");
        assert!(
            envelope["body"]
                .as_str()
                .is_some_and(|body| body.contains(summary)),
            "{verdict}: {envelope}"
        );
    }
}

#[test]
fn truncated_current_audit_envelope_fails_closed_for_every_ci_surface() {
    let root = workspace_fixture("tests/fixtures/basic-project");
    let saved_dir = tempfile::tempdir().expect("truncated audit directory");
    let path = saved_dir.path().join("truncated-audit.json");
    std::fs::write(
        &path,
        serde_json::to_vec(&serde_json::json!({
            "kind": "audit",
            "schema_version": fallow_output::AUDIT_SCHEMA_VERSION,
            "verdict": "pass"
        }))
        .expect("serialize truncated audit envelope"),
    )
    .expect("write truncated audit envelope");

    for format in [
        "codeclimate",
        "pr-comment-github",
        "pr-comment-gitlab",
        "review-github",
        "review-gitlab",
    ] {
        let rendered = run(
            &root,
            &[
                "report".to_string(),
                "--from".to_string(),
                path.display().to_string(),
                "--root".to_string(),
                root.display().to_string(),
                "--quiet".to_string(),
                "--format".to_string(),
                format.to_string(),
            ],
        );
        assert_eq!(rendered.status.code(), Some(2), "{format}");
        assert!(
            String::from_utf8_lossy(&rendered.stderr).contains("missing required field `version`"),
            "{format}: {}",
            String::from_utf8_lossy(&rendered.stderr)
        );
    }
}

#[test]
fn saved_future_schema_fails_with_precise_exit_two_error() {
    let root = workspace_fixture("tests/fixtures/basic-project");
    let saved_dir = tempfile::tempdir().expect("future schema directory");
    let path = saved_dir.path().join("future.json");
    std::fs::write(
        &path,
        r#"{"kind":"dead-code","schema_version":999,"unused_files":[]}"#,
    )
    .expect("write future envelope");
    let rendered = run(
        &root,
        &[
            "report".to_string(),
            "--from".to_string(),
            path.display().to_string(),
            "--root".to_string(),
            root.display().to_string(),
            "--quiet".to_string(),
            "--format".to_string(),
            "review-gitlab".to_string(),
        ],
    );
    assert_eq!(rendered.status.code(), Some(2));
    assert!(
        String::from_utf8_lossy(&rendered.stderr)
            .contains("unsupported saved dead-code schema version 999"),
        "{}",
        String::from_utf8_lossy(&rendered.stderr)
    );
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

    for format in [
        "pr-comment-github",
        "pr-comment-gitlab",
        "review-github",
        "review-gitlab",
    ] {
        let saved_ci = run(
            &root,
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
        assert_eq!(saved_ci.status.code(), Some(2), "{format}");
        assert!(
            String::from_utf8_lossy(&saved_ci.stderr).contains("do not support"),
            "{format}: {}",
            String::from_utf8_lossy(&saved_ci.stderr)
        );
    }
}
