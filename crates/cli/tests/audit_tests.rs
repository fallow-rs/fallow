#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests and benches use unwrap and expect to keep fixture setup concise"
)]

#[path = "common/mod.rs"]
mod common;

use common::{fallow_bin, parse_json, run_fallow_raw};
use std::fs;
use std::path::Path;
use std::process::Command;
use tempfile::TempDir;

fn git(dir: &std::path::Path, args: &[&str]) {
    let output = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .env("GIT_AUTHOR_NAME", "test")
        .env("GIT_AUTHOR_EMAIL", "test@test.com")
        .env("GIT_COMMITTER_NAME", "test")
        .env("GIT_COMMITTER_EMAIL", "test@test.com")
        .output()
        .expect("git command failed");
    assert!(
        output.status.success(),
        "git {:?} failed\nstdout: {}\nstderr: {}",
        args,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn commit_all(dir: &std::path::Path, message: &str) {
    git(dir, &["add", "."]);
    git(
        dir,
        &["-c", "commit.gpgsign=false", "commit", "-m", message],
    );
}

/// Create a temp git repo with a commit, suitable for audit testing.
/// Returns the `TempDir` guard so the directory lives as long as the caller holds it.
fn create_audit_fixture(_suffix: &str) -> TempDir {
    let tmp = TempDir::new().expect("failed to create temp dir");
    let dir = tmp.path();
    fs::create_dir_all(dir.join("src")).unwrap();

    fs::write(
        dir.join("package.json"),
        r#"{"name": "audit-test", "main": "src/index.ts", "dependencies": {"unused-pkg": "1.0.0"}}"#,
    )
    .unwrap();

    fs::write(
        dir.join("src/index.ts"),
        "import { used } from './utils';\nused();\n",
    )
    .unwrap();
    fs::write(
        dir.join("src/utils.ts"),
        "export const used = () => 42;\nexport const unused = () => 0;\n",
    )
    .unwrap();
    fs::write(
        dir.join("src/orphan.ts"),
        "export const orphaned = 'nobody';\n",
    )
    .unwrap();

    let git = |args: &[&str]| {
        Command::new("git")
            .args(args)
            .current_dir(dir)
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            .env("GIT_AUTHOR_NAME", "test")
            .env("GIT_AUTHOR_EMAIL", "test@test.com")
            .env("GIT_COMMITTER_NAME", "test")
            .env("GIT_COMMITTER_EMAIL", "test@test.com")
            .output()
            .expect("git command failed")
    };

    git(&["init", "-b", "main"]);
    git(&["add", "."]);
    git(&["-c", "commit.gpgsign=false", "commit", "-m", "initial"]);

    tmp
}

fn write_branchy_change(dir: &std::path::Path) {
    fs::write(
        dir.join("src/index.ts"),
        "import { used } from './utils';\n\
         used();\n\
         function branchy(n: number): number {\n\
           if (n < 0) return -1;\n\
           if (n === 0) return 0;\n\
           if (n < 10) return 1;\n\
           if (n < 100) return 2;\n\
           if (n < 1000) return 3;\n\
           if (n < 10000) return 4;\n\
           return 5;\n\
         }\n\
         branchy(used());\n",
    )
    .unwrap();
    commit_all(dir, "add branchy");
}

fn write_branchy_istanbul_coverage(coverage_path: &std::path::Path, coverage_source_path: &str) {
    fs::create_dir_all(coverage_path.parent().unwrap()).unwrap();
    let mut coverage = serde_json::Map::new();
    coverage.insert(
        coverage_source_path.to_string(),
        serde_json::json!({
            "path": coverage_source_path,
            "statementMap": {},
            "fnMap": {
                "0": {
                    "name": "branchy",
                    "line": 3,
                    "decl": {
                        "start": { "line": 3, "column": 9 },
                        "end": { "line": 3, "column": 16 }
                    },
                    "loc": {
                        "start": { "line": 3, "column": 35 },
                        "end": { "line": 11, "column": 10 }
                    }
                }
            },
            "branchMap": {},
            "s": {},
            "f": { "0": 1 },
            "b": {}
        }),
    );
    fs::write(coverage_path, serde_json::to_string(&coverage).unwrap()).unwrap();
}

fn run_fallow_raw_with_env(
    args: &[&str],
    env: &[(&str, &std::path::Path)],
) -> common::CommandOutput {
    let mut cmd = Command::new(fallow_bin());
    cmd.env("RUST_LOG", "").env("NO_COLOR", "1");
    for (key, value) in env {
        cmd.env(key, value);
    }
    for arg in args {
        cmd.arg(arg);
    }
    let output = cmd.output().expect("failed to run fallow binary");
    common::CommandOutput {
        stdout: String::from_utf8_lossy(&output.stdout).to_string(),
        stderr: String::from_utf8_lossy(&output.stderr).to_string(),
        code: output.status.code().unwrap_or(-1),
    }
}

#[test]
fn audit_json_has_verdict_and_schema() {
    let dir = create_audit_fixture("verdict");
    let output = run_fallow_raw(&[
        "audit",
        "--root",
        dir.path().to_str().unwrap(),
        "--base",
        "HEAD",
        "--format",
        "json",
        "--quiet",
    ]);

    assert_eq!(
        output.code, 0,
        "audit with no changes should exit 0. stderr: {}",
        output.stderr
    );

    let json = parse_json(&output);
    assert_eq!(
        json["verdict"].as_str(),
        Some("pass"),
        "no changes should give pass verdict"
    );
    assert_eq!(
        json["command"].as_str(),
        Some("audit"),
        "command should be 'audit'"
    );
    assert!(
        json.get("schema_version").is_some(),
        "audit JSON should have schema_version"
    );
}

#[test]
fn audit_pass_verdict_when_no_changes() {
    let dir = create_audit_fixture("nochanges");
    let output = run_fallow_raw(&[
        "audit",
        "--root",
        dir.path().to_str().unwrap(),
        "--base",
        "HEAD",
        "--format",
        "json",
        "--quiet",
    ]);

    assert_eq!(output.code, 0, "no changes should give exit 0");

    let json = parse_json(&output);
    assert_eq!(
        json["verdict"].as_str(),
        Some("pass"),
        "no changes should give pass verdict"
    );
    assert_eq!(
        json["changed_files_count"].as_u64(),
        Some(0),
        "should report 0 changed files"
    );
}

/// Audit's HEAD analyses and base-snapshot computation run concurrently via
/// `rayon::join`; inside the base snapshot, check and dupes also run
/// concurrently. Verify nondeterministic scheduling does not leak into the
/// rendered JSON: repeated runs against the same fixture must produce
/// byte-identical output once wall-clock fields are stripped.
#[test]
fn audit_parallel_output_is_deterministic() {
    let dir = create_audit_fixture("determinism");

    fs::write(
        dir.path().join("src/new.ts"),
        "export const dupA = (x: number) => x + 1;\nexport const dupB = (x: number) => x + 1;\n",
    )
    .unwrap();
    Command::new("git")
        .args(["add", "."])
        .current_dir(dir.path())
        .output()
        .unwrap();
    Command::new("git")
        .args(["-c", "commit.gpgsign=false", "commit", "-m", "add new file"])
        .current_dir(dir.path())
        .env("GIT_AUTHOR_NAME", "test")
        .env("GIT_AUTHOR_EMAIL", "test@test.com")
        .env("GIT_COMMITTER_NAME", "test")
        .env("GIT_COMMITTER_EMAIL", "test@test.com")
        .output()
        .unwrap();

    fn normalize(value: &mut serde_json::Value) {
        match value {
            serde_json::Value::Object(map) => {
                map.remove("elapsed_ms");
                map.remove("head_sha");
                if let Some(telemetry) = map
                    .get_mut("_meta")
                    .and_then(|meta| meta.get_mut("telemetry"))
                    .and_then(|telemetry| telemetry.as_object_mut())
                {
                    telemetry.remove("analysis_run_id");
                }
                for v in map.values_mut() {
                    normalize(v);
                }
            }
            serde_json::Value::Array(items) => {
                for v in items {
                    normalize(v);
                }
            }
            _ => {}
        }
    }

    let mut canonicalized: Vec<String> = std::iter::repeat_with(|| {
        let output = run_fallow_raw(&[
            "audit",
            "--root",
            dir.path().to_str().unwrap(),
            "--base",
            "HEAD~1",
            "--format",
            "json",
            "--quiet",
        ]);
        assert!(
            output.code == 0 || output.code == 1,
            "audit run should not crash: stdout={}\nstderr={}",
            output.stdout,
            output.stderr
        );
        let mut value = parse_json(&output);
        normalize(&mut value);
        serde_json::to_string(&value).expect("re-serialize canonical json")
    })
    .take(3)
    .collect();

    let first = canonicalized.remove(0);
    for (idx, run) in canonicalized.iter().enumerate() {
        assert_eq!(
            &first,
            run,
            "audit parallel run #{} differed from run #0",
            idx + 1
        );
    }
}

#[test]
fn audit_json_has_summary_with_changes() {
    let dir = create_audit_fixture("summary");

    fs::write(
        dir.path().join("src/new.ts"),
        "export const newThing = 'added';\n",
    )
    .unwrap();

    Command::new("git")
        .args(["add", "."])
        .current_dir(dir.path())
        .output()
        .unwrap();
    Command::new("git")
        .args(["-c", "commit.gpgsign=false", "commit", "-m", "add new file"])
        .current_dir(dir.path())
        .env("GIT_AUTHOR_NAME", "test")
        .env("GIT_AUTHOR_EMAIL", "test@test.com")
        .env("GIT_COMMITTER_NAME", "test")
        .env("GIT_COMMITTER_EMAIL", "test@test.com")
        .output()
        .unwrap();

    let output = run_fallow_raw(&[
        "audit",
        "--root",
        dir.path().to_str().unwrap(),
        "--base",
        "HEAD~1",
        "--format",
        "json",
        "--quiet",
    ]);

    assert!(
        output.code == 0 || output.code == 1,
        "audit should not crash, got exit {}. stderr: {}",
        output.code,
        output.stderr
    );

    let json = parse_json(&output);
    assert!(
        json.get("summary").is_some(),
        "audit JSON should have summary"
    );
    let summary = &json["summary"];
    assert!(
        summary.get("dead_code_issues").is_some(),
        "summary should have dead_code_issues"
    );
}

/// Create a fixture whose legacy file already has several unused exports,
/// then branch and touch that file without introducing new issues.
///
/// Returns the `TempDir` guard. The fixture is on a branch named
/// `feature`; the default branch is `main`.
fn create_audit_baseline_fixture() -> TempDir {
    let tmp = TempDir::new().expect("failed to create temp dir");
    let dir = tmp.path();
    fs::create_dir_all(dir.join("src")).unwrap();

    fs::write(
        dir.join("package.json"),
        r#"{"name": "audit-baseline-test", "main": "src/index.ts"}"#,
    )
    .unwrap();
    fs::write(
        dir.join("tsconfig.json"),
        r#"{"compilerOptions":{"target":"ES2022","module":"ESNext","moduleResolution":"bundler"},"include":["src"]}"#,
    )
    .unwrap();

    fs::write(
        dir.join("src/legacy.ts"),
        "export const used = 1;\n\
         export const unusedA = 'a';\n\
         export const unusedB = 'b';\n\
         export const unusedC = 'c';\n\
         export const unusedD = 'd';\n\
         export const unusedE = 'e';\n",
    )
    .unwrap();
    fs::write(
        dir.join("src/index.ts"),
        "import { used } from './legacy';\nconsole.log(used);\n",
    )
    .unwrap();

    let git = |args: &[&str]| {
        Command::new("git")
            .args(args)
            .current_dir(dir)
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            .env("GIT_AUTHOR_NAME", "test")
            .env("GIT_AUTHOR_EMAIL", "test@test.com")
            .env("GIT_COMMITTER_NAME", "test")
            .env("GIT_COMMITTER_EMAIL", "test@test.com")
            .output()
            .expect("git command failed")
    };

    git(&["init", "-b", "main"]);
    git(&["add", "."]);
    git(&["-c", "commit.gpgsign=false", "commit", "-m", "initial"]);
    git(&["checkout", "-b", "feature"]);

    let legacy = fs::read_to_string(dir.join("src/legacy.ts")).unwrap();
    fs::write(dir.join("src/legacy.ts"), format!("{legacy}// touched\n")).unwrap();
    git(&["add", "."]);
    git(&["-c", "commit.gpgsign=false", "commit", "-m", "touch legacy"]);

    tmp
}

#[test]
fn audit_default_gate_ignores_inherited_issues() {
    let tmp = create_audit_baseline_fixture();
    let output = run_fallow_raw(&[
        "audit",
        "--root",
        tmp.path().to_str().unwrap(),
        "--base",
        "main",
        "--format",
        "json",
        "--quiet",
    ]);

    assert_eq!(
        output.code, 0,
        "audit should pass when touched file has only inherited issues. stderr: {}",
        output.stderr
    );
    let json = parse_json(&output);
    assert_eq!(json["verdict"].as_str(), Some("pass"));
    let dead_code_issues = json["summary"]["dead_code_issues"]
        .as_u64()
        .expect("summary.dead_code_issues should be present");
    assert!(
        dead_code_issues >= 5,
        "expected at least 5 pre-existing unused exports, got {dead_code_issues}"
    );
    assert_eq!(
        json["attribution"]["dead_code_introduced"].as_u64(),
        Some(0)
    );
    assert!(
        json["attribution"]["dead_code_inherited"]
            .as_u64()
            .is_some_and(|count| count >= 5),
        "expected inherited dead-code attribution"
    );
    let inherited_exports = json["dead_code"]["unused_exports"]
        .as_array()
        .expect("dead_code.unused_exports should be an array");
    assert!(
        inherited_exports
            .iter()
            .all(|item| item["introduced"] == false),
        "all touched legacy exports should be annotated as inherited"
    );
}

#[test]
fn audit_new_only_inherits_shifted_duplicate_group() {
    let tmp = TempDir::new().expect("failed to create temp dir");
    let dir = tmp.path();

    let duplicate = "export function sharedBlock(x: number): number {\n\
          const a = x + 1;\n\
          const b = a * 2;\n\
          const c = b - 3;\n\
          const d = c * c;\n\
          const e = d + a;\n\
          const f = e - b;\n\
          const g = f + c;\n\
          const h = g * d;\n\
          const i = h - e;\n\
          return a + b + c + d + e + f + g + h + i;\n\
        }\n";
    fs::write(dir.join("fileB.ts"), duplicate).unwrap();

    use std::fmt::Write as _;
    let mut shifted_source = String::new();
    for n in 1..=120 {
        writeln!(shifted_source, "export const v{n} = {n};").unwrap();
    }
    shifted_source.push_str(duplicate);
    fs::write(dir.join("fileA.ts"), &shifted_source).unwrap();

    git(dir, &["init", "-b", "main"]);
    // The clone fingerprint is hashed over the raw fragment text, so CRLF vs LF
    // shifts it. `fallow audit` spawns its own `git worktree add` for the base
    // snapshot, which inherits the runner's global git config (Windows defaults
    // to `core.autocrlf=true`), so the checked-out base would get CRLF while the
    // head file written via `fs::write` keeps LF, making the inherited clone look
    // introduced. Pin the repo to LF so base and head fingerprints match.
    git(dir, &["config", "core.autocrlf", "false"]);
    commit_all(dir, "initial");
    git(dir, &["checkout", "-b", "edit"]);

    fs::write(
        dir.join("fileA.ts"),
        format!("export const NEW_TOP_CONST = 0;\n{shifted_source}"),
    )
    .unwrap();
    commit_all(dir, "shift unchanged duplicate");

    let output = run_fallow_raw(&[
        "audit",
        "--root",
        dir.to_str().unwrap(),
        "--base",
        "main",
        "--gate",
        "new-only",
        "--format",
        "json",
        "--quiet",
        "--no-cache",
        "--performance",
        "--dupes-mode",
        "strict",
        "--dupes-min-tokens",
        "10",
        "--dupes-min-lines",
        "3",
    ]);

    assert_eq!(
        output.code, 0,
        "audit should pass when only line numbers changed for an inherited duplicate. stdout: {}\nstderr: {}",
        output.stdout, output.stderr
    );
    let json = parse_json(&output);
    assert_eq!(json["base_snapshot_skipped"].as_bool(), Some(false));
    assert_eq!(
        json["attribution"]["duplication_introduced"].as_u64(),
        Some(0)
    );
    assert!(
        json["attribution"]["duplication_inherited"]
            .as_u64()
            .is_some_and(|count| count > 0),
        "expected inherited duplicate attribution"
    );

    let groups = json["duplication"]["clone_groups"]
        .as_array()
        .expect("duplication.clone_groups should be an array");
    assert!(!groups.is_empty(), "expected at least one clone group");
    assert!(
        groups.iter().all(|group| group["introduced"] == false),
        "all duplicate groups should be marked inherited"
    );
    assert!(
        groups.iter().any(|group| {
            group["instances"].as_array().is_some_and(|instances| {
                let has_shifted_file = instances
                    .iter()
                    .any(|instance| instance["file"].as_str() == Some("fileA.ts"));
                let has_peer_file = instances
                    .iter()
                    .any(|instance| instance["file"].as_str() == Some("fileB.ts"));
                has_shifted_file && has_peer_file
            })
        }),
        "expected a clone group spanning fileA.ts and fileB.ts"
    );
}

#[test]
fn audit_gate_all_reports_preexisting_issues() {
    let tmp = create_audit_baseline_fixture();
    fs::write(tmp.path().join("fallow.toml"), "[audit]\ngate = \"all\"\n").unwrap();
    let output = run_fallow_raw(&[
        "audit",
        "--root",
        tmp.path().to_str().unwrap(),
        "--base",
        "main",
        "--config",
        tmp.path().join("fallow.toml").to_str().unwrap(),
        "--format",
        "json",
        "--quiet",
    ]);

    assert_eq!(
        output.code, 1,
        "audit should fail when audit.gate=all and touched file has pre-existing issues. stderr: {}",
        output.stderr
    );
    let json = parse_json(&output);
    assert_eq!(json["verdict"].as_str(), Some("fail"));
    assert_eq!(json["attribution"]["gate"].as_str(), Some("all"));
    assert_eq!(
        json["attribution"]["dead_code_introduced"].as_u64(),
        Some(0),
        "gate=all should skip base attribution work"
    );
    assert_eq!(
        json["attribution"]["dead_code_inherited"].as_u64(),
        Some(0),
        "gate=all should skip base attribution work"
    );
    assert!(
        json["dead_code"]["unused_exports"][0]
            .get("introduced")
            .is_none(),
        "gate=all should not annotate per-issue introduced fields without a base snapshot"
    );
}

#[test]
fn audit_gate_cli_flag_overrides_default() {
    let tmp = create_audit_baseline_fixture();
    let output = run_fallow_raw(&[
        "audit",
        "--root",
        tmp.path().to_str().unwrap(),
        "--base",
        "main",
        "--gate",
        "all",
        "--format",
        "json",
        "--quiet",
    ]);

    assert_eq!(
        output.code, 1,
        "--gate all should fail on inherited findings. stderr: {}",
        output.stderr
    );
    let json = parse_json(&output);
    assert_eq!(json["verdict"].as_str(), Some("fail"));
    assert_eq!(json["attribution"]["gate"].as_str(), Some("all"));
    assert_eq!(
        json["attribution"]["dead_code_introduced"].as_u64(),
        Some(0)
    );
    assert_eq!(json["attribution"]["dead_code_inherited"].as_u64(), Some(0));
}

#[test]
fn audit_help_documents_gate() {
    let output = run_fallow_raw(&["audit", "--help"]);
    assert_eq!(output.code, 0, "audit --help should succeed");
    assert!(
        output.stdout.contains("--gate <GATE>"),
        "--help should include --gate, got:\n{}",
        output.stdout
    );
    assert!(
        output.stdout.contains("new-only") && output.stdout.contains("introduced"),
        "--help should document new-only semantics, got:\n{}",
        output.stdout
    );
}

#[test]
fn audit_base_preserves_node_modules_tsconfig_extends_context() {
    let tmp = TempDir::new().expect("failed to create temp dir");
    let dir = tmp.path();
    fs::create_dir_all(dir.join("src")).unwrap();
    fs::write(dir.join(".gitignore"), "node_modules\n.fallow\n").unwrap();
    fs::write(
        dir.join("package.json"),
        r#"{"name":"audit-rn-alias","main":"src/index.ts","dependencies":{"@react-native/typescript-config":"1.0.0"}}"#,
    )
    .unwrap();
    fs::write(
        dir.join("tsconfig.json"),
        r#"{"extends":"./node_modules/@react-native/typescript-config/tsconfig.json","compilerOptions":{"baseUrl":".","paths":{"@/*":["src/*"]}},"include":["src"]}"#,
    )
    .unwrap();
    fs::write(
        dir.join("src/index.ts"),
        "import { used } from '@/feature';\nconsole.log(used);\n",
    )
    .unwrap();
    fs::write(
        dir.join("src/feature.ts"),
        "export const used = 1;\nexport const legacyUnused = 2;\n",
    )
    .unwrap();

    git(dir, &["init", "-b", "main"]);
    commit_all(dir, "initial");

    let rn_config = dir.join("node_modules/@react-native/typescript-config");
    fs::create_dir_all(&rn_config).unwrap();
    fs::write(
        rn_config.join("tsconfig.json"),
        r#"{"compilerOptions":{"jsx":"react-native","moduleResolution":"bundler"}}"#,
    )
    .unwrap();

    fs::write(
        dir.join("src/feature.ts"),
        "export const used = 1;\nexport const legacyUnused = 2;\nexport const introduced = 3;\n",
    )
    .unwrap();
    commit_all(dir, "introduce new export");

    let output = run_fallow_raw(&[
        "audit",
        "--root",
        dir.to_str().unwrap(),
        "--base",
        "HEAD~1",
        "--format",
        "json",
        "--quiet",
        "--no-cache",
    ]);

    assert!(
        !output.stderr.contains("Broken tsconfig chain")
            && !output.stderr.contains("node_modules directory not found"),
        "audit base worktree should retain installed tsconfig context. stderr: {}",
        output.stderr
    );
    let json = parse_json(&output);
    assert_eq!(
        json["dead_code"]["summary"]["unresolved_imports"].as_u64(),
        Some(0),
        "tsconfig alias should resolve in the current analysis"
    );
    assert_eq!(
        json["attribution"]["dead_code_introduced"].as_u64(),
        Some(1),
        "only the genuinely new export should be attributed to the changeset"
    );
}

#[test]
fn audit_new_unlisted_dependency_import_site_is_introduced() {
    let tmp = TempDir::new().expect("failed to create temp dir");
    let dir = tmp.path();
    fs::create_dir_all(dir.join("src")).unwrap();
    fs::write(
        dir.join("package.json"),
        r#"{"name":"audit-unlisted","main":"src/index.ts","dependencies":{}}"#,
    )
    .unwrap();
    fs::write(
        dir.join("tsconfig.json"),
        r#"{"compilerOptions":{"target":"ES2022","module":"ESNext","moduleResolution":"bundler"},"include":["src"]}"#,
    )
    .unwrap();
    fs::write(
        dir.join("src/a.ts"),
        "import leftPad from 'left-pad';\nexport const a = leftPad('a', 2);\n",
    )
    .unwrap();
    fs::write(
        dir.join("src/index.ts"),
        "import { a } from './a';\nconsole.log(a);\n",
    )
    .unwrap();
    git(dir, &["init", "-b", "main"]);
    commit_all(dir, "initial");

    fs::write(
        dir.join("src/b.ts"),
        "import leftPad from 'left-pad';\nexport const b = leftPad('b', 2);\n",
    )
    .unwrap();
    fs::write(
        dir.join("src/index.ts"),
        "import { a } from './a';\nimport { b } from './b';\nconsole.log(a, b);\n",
    )
    .unwrap();
    commit_all(dir, "add b");

    let output = run_fallow_raw(&[
        "audit",
        "--root",
        dir.to_str().unwrap(),
        "--base",
        "HEAD~1",
        "--format",
        "json",
        "--quiet",
    ]);

    assert_eq!(
        output.code, 1,
        "new unlisted import site should fail new-only audit. stdout: {}\nstderr: {}",
        output.stdout, output.stderr
    );
    let json = parse_json(&output);
    assert_eq!(json["verdict"].as_str(), Some("fail"));
    assert_eq!(
        json["attribution"]["dead_code_introduced"].as_u64(),
        Some(1)
    );
    assert_eq!(
        json["dead_code"]["unlisted_dependencies"][0]["introduced"],
        true
    );
}

#[test]
fn audit_empty_catalog_group_changed_manifest_is_introduced() {
    let tmp = TempDir::new().expect("failed to create temp dir");
    let dir = tmp.path();
    fs::create_dir_all(dir.join("packages/app")).unwrap();
    fs::write(
        dir.join("package.json"),
        r#"{"name":"audit-empty-catalog-group","private":true,"workspaces":["packages/*"]}"#,
    )
    .unwrap();
    fs::write(
        dir.join("packages/app/package.json"),
        r#"{"name":"app","private":true,"main":"src/index.ts","dependencies":{"vue":"catalog:vue3"}}"#,
    )
    .unwrap();
    fs::create_dir_all(dir.join("packages/app/src")).unwrap();
    fs::write(
        dir.join("packages/app/src/index.ts"),
        "import { ref } from 'vue';\nconsole.log(ref);\n",
    )
    .unwrap();
    fs::write(
        dir.join("pnpm-workspace.yaml"),
        "packages:\n  - 'packages/*'\n\ncatalogs:\n  vue3:\n    vue: ^3.4.0\n",
    )
    .unwrap();
    git(dir, &["init", "-b", "main"]);
    commit_all(dir, "initial");

    fs::write(
        dir.join("pnpm-workspace.yaml"),
        "packages:\n  - 'packages/*'\n\ncatalogs:\n  legacy: {}\n  vue3:\n    old-react: ^17.0.2\n    vue: ^3.4.0\n",
    )
    .unwrap();

    let output = run_fallow_raw(&[
        "audit",
        "--root",
        dir.to_str().unwrap(),
        "--base",
        "HEAD",
        "--format",
        "json",
        "--quiet",
        "--no-cache",
    ]);

    assert_eq!(
        output.code, 0,
        "new warning-level catalog hygiene should not fail audit. stdout: {}\nstderr: {}",
        output.stdout, output.stderr
    );
    let json = parse_json(&output);
    assert_eq!(json["verdict"].as_str(), Some("warn"));
    assert_eq!(
        json["attribution"]["dead_code_introduced"].as_u64(),
        Some(2)
    );
    assert_eq!(
        json["dead_code"]["unused_catalog_entries"][0]["entry_name"].as_str(),
        Some("old-react")
    );
    assert_eq!(
        json["dead_code"]["unused_catalog_entries"][0]["introduced"],
        true
    );
    assert_eq!(
        json["dead_code"]["empty_catalog_groups"][0]["catalog_name"].as_str(),
        Some("legacy")
    );
    assert_eq!(
        json["dead_code"]["empty_catalog_groups"][0]["introduced"],
        true
    );
}

#[test]
fn audit_invalid_client_export_is_introduced() {
    let tmp = TempDir::new().expect("failed to create temp dir");
    let dir = tmp.path();
    fs::create_dir_all(dir.join("app")).unwrap();
    fs::write(
        dir.join("package.json"),
        r#"{"name":"audit-invalid-client-export","private":true,"dependencies":{"next":"15.0.0","react":"19.0.0"}}"#,
    )
    .unwrap();
    fs::write(
        dir.join("app/page.tsx"),
        "\"use client\";\nexport default function Page() { return null; }\n",
    )
    .unwrap();
    git(dir, &["init", "-b", "main"]);
    commit_all(dir, "initial");

    // Introduce a server-only export inside the existing "use client" file.
    fs::write(
        dir.join("app/page.tsx"),
        "\"use client\";\nexport const metadata = { title: \"Home\" };\nexport default function Page() { return null; }\n",
    )
    .unwrap();

    let output = run_fallow_raw(&[
        "audit",
        "--root",
        dir.to_str().unwrap(),
        "--base",
        "HEAD",
        "--format",
        "json",
        "--quiet",
        "--no-cache",
    ]);

    assert_eq!(
        output.code, 0,
        "new warning-level invalid client export should not fail audit. stdout: {}\nstderr: {}",
        output.stdout, output.stderr
    );
    let json = parse_json(&output);
    assert_eq!(
        json["dead_code"]["invalid_client_exports"][0]["export_name"].as_str(),
        Some("metadata")
    );
    assert_eq!(
        json["dead_code"]["invalid_client_exports"][0]["introduced"],
        true
    );
}

#[test]
fn audit_dependency_location_change_is_introduced() {
    let tmp = TempDir::new().expect("failed to create temp dir");
    let dir = tmp.path();
    fs::create_dir_all(dir.join("src")).unwrap();
    fs::write(
        dir.join("package.json"),
        r#"{"name":"audit-dep-move","main":"src/index.ts","devDependencies":{"left-pad":"1.0.0"}}"#,
    )
    .unwrap();
    fs::write(dir.join("src/index.ts"), "console.log('hi');\n").unwrap();
    git(dir, &["init", "-b", "main"]);
    commit_all(dir, "initial");

    fs::write(
        dir.join("package.json"),
        r#"{"name":"audit-dep-move","main":"src/index.ts","dependencies":{"left-pad":"1.0.0"}}"#,
    )
    .unwrap();
    commit_all(dir, "move dependency");

    let output = run_fallow_raw(&[
        "audit",
        "--root",
        dir.to_str().unwrap(),
        "--base",
        "HEAD~1",
        "--format",
        "json",
        "--quiet",
    ]);

    assert_eq!(
        output.code, 1,
        "moving an unused package into dependencies should be introduced. stdout: {}\nstderr: {}",
        output.stdout, output.stderr
    );
    let json = parse_json(&output);
    assert_eq!(json["verdict"].as_str(), Some("fail"));
    assert_eq!(
        json["attribution"]["dead_code_introduced"].as_u64(),
        Some(1)
    );
    assert_eq!(
        json["dead_code"]["unused_dependencies"][0]["introduced"],
        true
    );
}

#[test]
fn audit_with_dead_code_baseline_filters_preexisting_issues() {
    let tmp = create_audit_baseline_fixture();
    let dir = tmp.path();
    let baseline_path = dir.join(".fallow-dead-code-baseline.json");

    let git = |args: &[&str]| {
        Command::new("git")
            .args(args)
            .current_dir(dir)
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            .env("GIT_AUTHOR_NAME", "test")
            .env("GIT_AUTHOR_EMAIL", "test@test.com")
            .env("GIT_COMMITTER_NAME", "test")
            .env("GIT_COMMITTER_EMAIL", "test@test.com")
            .output()
            .expect("git command failed")
    };
    git(&["checkout", "main"]);
    let save = run_fallow_raw(&[
        "dead-code",
        "--root",
        dir.to_str().unwrap(),
        "--save-baseline",
        baseline_path.to_str().unwrap(),
        "--format",
        "json",
        "--quiet",
    ]);
    assert!(
        save.code == 0 || save.code == 1,
        "save-baseline should not crash, got {}: {}",
        save.code,
        save.stderr
    );
    assert!(
        baseline_path.exists(),
        "baseline file should have been written"
    );
    git(&["checkout", "feature"]);

    let output = run_fallow_raw(&[
        "audit",
        "--root",
        dir.to_str().unwrap(),
        "--base",
        "main",
        "--dead-code-baseline",
        baseline_path.to_str().unwrap(),
        "--format",
        "json",
        "--quiet",
    ]);

    assert_eq!(
        output.code, 0,
        "audit with dead-code baseline should pass (no new issues). stdout: {}\nstderr: {}",
        output.stdout, output.stderr
    );
    let json = parse_json(&output);
    assert_eq!(
        json["verdict"].as_str(),
        Some("pass"),
        "verdict should be pass when all pre-existing issues are baselined"
    );
    assert_eq!(
        json["summary"]["dead_code_issues"].as_u64(),
        Some(0),
        "baseline should filter all pre-existing unused exports"
    );
}

#[test]
fn audit_rejects_global_baseline_flag() {
    let tmp = create_audit_baseline_fixture();
    let output = run_fallow_raw(&[
        "--baseline",
        "anything.json",
        "audit",
        "--root",
        tmp.path().to_str().unwrap(),
        "--base",
        "main",
        "--format",
        "json",
        "--quiet",
    ]);

    assert_eq!(
        output.code, 2,
        "global --baseline on audit should exit 2. stderr: {}",
        output.stderr
    );
    let combined = format!("{}{}", output.stdout, output.stderr);
    assert!(
        combined.contains("--dead-code-baseline")
            || combined.contains("--health-baseline")
            || combined.contains("--dupes-baseline"),
        "error should point users at per-analysis flags, got: {combined}"
    );
}

#[test]
fn audit_rejects_global_save_baseline_flag() {
    let tmp = create_audit_baseline_fixture();
    let output = run_fallow_raw(&[
        "--save-baseline",
        "anywhere.json",
        "audit",
        "--root",
        tmp.path().to_str().unwrap(),
        "--base",
        "main",
        "--format",
        "json",
        "--quiet",
    ]);

    assert_eq!(
        output.code, 2,
        "global --save-baseline on audit should exit 2. stderr: {}",
        output.stderr
    );
    let combined = format!("{}{}", output.stdout, output.stderr);
    assert!(
        combined.contains("--dead-code-baseline")
            || combined.contains("--health-baseline")
            || combined.contains("--dupes-baseline"),
        "error should point users at per-analysis flags, got: {combined}"
    );
}

#[test]
fn audit_badge_format_exits_2() {
    let dir = create_audit_fixture("badge");
    let output = run_fallow_raw(&[
        "audit",
        "--root",
        dir.path().to_str().unwrap(),
        "--base",
        "HEAD",
        "--format",
        "badge",
        "--quiet",
    ]);
    assert_eq!(
        output.code, 2,
        "audit with --format badge should exit 2 (unsupported)"
    );
}

/// `--max-crap` on audit must flow into the health sub-analysis so that a
/// changed file with a high-complexity untested function triggers the
/// failing verdict.
#[test]
fn audit_max_crap_flag_fails_when_threshold_crossed() {
    let dir = create_audit_fixture("crap");

    write_branchy_change(dir.path());

    let output = run_fallow_raw(&[
        "audit",
        "--root",
        dir.path().to_str().unwrap(),
        "--base",
        "HEAD~1",
        "--max-crap",
        "1",
        "--format",
        "json",
        "--quiet",
    ]);
    assert_eq!(
        output.code, 1,
        "audit should fail when --max-crap is crossed. stderr: {}",
        output.stderr
    );
    let json = parse_json(&output);
    assert_eq!(
        json["verdict"].as_str(),
        Some("fail"),
        "verdict should be fail when CRAP threshold is crossed"
    );
}

#[test]
fn audit_respects_health_threshold_override() {
    let dir = create_audit_fixture("health-threshold-override");
    fs::write(
        dir.path().join(".fallowrc.json"),
        r#"{
  "health": {
    "thresholdOverrides": [
      {
        "files": ["src/index.ts"],
        "functions": ["branchy"],
        "maxCyclomatic": 20,
        "maxCognitive": 20,
        "maxCrap": 100
      }
    ]
  }
}
"#,
    )
    .unwrap();
    write_branchy_change(dir.path());

    let output = run_fallow_raw(&[
        "audit",
        "--root",
        dir.path().to_str().unwrap(),
        "--base",
        "HEAD~1",
        "--max-crap",
        "1",
        "--format",
        "json",
        "--quiet",
    ]);
    assert_eq!(
        output.code, 0,
        "audit should pass when health override raises local thresholds. stderr: {}",
        output.stderr
    );
    let json = parse_json(&output);
    assert_eq!(json["verdict"].as_str(), Some("pass"));
}

fn audit_with_env(root: &Path, env: &[(&str, &str)]) -> common::CommandOutput {
    let bin = fallow_bin();
    let mut cmd = Command::new(&bin);
    cmd.args([
        "audit",
        "--root",
        root.to_str().unwrap(),
        "--base",
        "HEAD",
        "--format",
        "json",
        "--quiet",
    ])
    .env("RUST_LOG", "")
    .env("NO_COLOR", "1");
    for (key, value) in env {
        cmd.env(key, value);
    }
    let output = cmd.output().expect("failed to run fallow binary");
    common::CommandOutput {
        stdout: String::from_utf8_lossy(&output.stdout).to_string(),
        stderr: String::from_utf8_lossy(&output.stderr).to_string(),
        code: output.status.code().unwrap_or(-1),
    }
}

/// Regression test for issue #301. When git invokes hooks (`pre-commit`,
/// `pre-push`), it sets `GIT_INDEX_FILE=.git/index` (relative path) plus
/// related repo-state vars. Before the fix in #301, fallow inherited these
/// into its own git invocations and `git worktree add` failed because the
/// relative index path no longer resolved from the temporary worktree dir.
///
/// The test runs `fallow audit` under each of the ambient repo-state vars
/// individually and asserts the audit succeeds, mirroring the leak shapes a
/// hook subprocess actually sees.
#[test]
fn audit_succeeds_when_ambient_git_env_vars_leak_from_a_hook() {
    let dir = create_audit_fixture("hook_env_leak");
    let root = dir.path();

    let abs_index = root.join(".git/index").to_string_lossy().to_string();
    let cases: &[(&str, &str)] = &[
        ("GIT_INDEX_FILE", ".git/index"),
        ("GIT_INDEX_FILE", abs_index.as_str()),
        ("GIT_DIR", ".git"),
        ("GIT_WORK_TREE", "."),
        ("GIT_OBJECT_DIRECTORY", ".git/objects"),
        ("GIT_COMMON_DIR", ".git"),
        ("GIT_PREFIX", ""),
    ];

    for (key, value) in cases {
        let output = audit_with_env(root, &[(key, value)]);
        assert_eq!(
            output.code, 0,
            "audit must exit 0 with {key}={value:?} set; stderr: {}",
            output.stderr
        );
        let json = parse_json(&output);
        assert!(
            json["verdict"].is_string(),
            "audit JSON should still include a verdict with {key}={value:?} set"
        );
    }
}

#[test]
fn audit_coverage_and_coverage_root_feed_crap_scoring() {
    let dir = create_audit_fixture("coverage-root");
    write_branchy_change(dir.path());

    let without_coverage = run_fallow_raw(&[
        "audit",
        "--root",
        dir.path().to_str().unwrap(),
        "--base",
        "HEAD~1",
        "--max-crap",
        "10",
        "--format",
        "json",
        "--quiet",
    ]);
    assert_eq!(
        without_coverage.code, 1,
        "static CRAP estimate should fail before Istanbul coverage is supplied. stderr: {}",
        without_coverage.stderr
    );

    let coverage_path = dir.path().join("artifacts/coverage-final.json");
    write_branchy_istanbul_coverage(&coverage_path, "/ci/workspace/src/index.ts");

    let with_coverage = run_fallow_raw(&[
        "audit",
        "--root",
        dir.path().to_str().unwrap(),
        "--base",
        "HEAD~1",
        "--max-crap",
        "10",
        "--coverage",
        coverage_path.to_str().unwrap(),
        "--coverage-root",
        "/ci/workspace",
        "--format",
        "json",
        "--quiet",
    ]);
    assert_eq!(
        with_coverage.code, 0,
        "Istanbul coverage should lower CRAP below the audit threshold. stderr: {}",
        with_coverage.stderr
    );
    let json = parse_json(&with_coverage);
    assert_eq!(json["verdict"].as_str(), Some("pass"));
}

#[test]
fn audit_rejects_relative_coverage_root() {
    let dir = create_audit_fixture("coverage-root-relative-rejected");

    let output = run_fallow_raw(&[
        "audit",
        "--root",
        dir.path().to_str().unwrap(),
        "--base",
        "HEAD~1",
        "--coverage-root",
        "src",
        "--format",
        "json",
        "--quiet",
    ]);
    assert_eq!(
        output.code, 2,
        "relative --coverage-root should be rejected before audit runs. stderr: {}",
        output.stderr
    );
    let json = parse_json(&output);
    assert_eq!(json["error"], serde_json::json!(true));
    let message = json["message"].as_str().expect("message should be present");
    assert!(
        message.contains("--coverage-root expects an absolute path")
            && message.contains("got 'src'"),
        "unexpected error message: {message}"
    );
}

#[test]
fn audit_coverage_relative_path_resolves_against_root_through_base_snapshot() {
    let dir = create_audit_fixture("coverage-relative");
    write_branchy_change(dir.path());

    let coverage_path = dir.path().join("artifacts/coverage-final.json");
    let branchy_source = dir.path().join("src/index.ts");
    write_branchy_istanbul_coverage(&coverage_path, &branchy_source.to_string_lossy());

    let with_relative = run_fallow_raw(&[
        "audit",
        "--root",
        dir.path().to_str().unwrap(),
        "--base",
        "HEAD~1",
        "--max-crap",
        "10",
        "--coverage",
        "artifacts/coverage-final.json",
        "--format",
        "json",
        "--quiet",
    ]);
    assert_eq!(
        with_relative.code, 0,
        "relative --coverage must resolve against --root through both the HEAD pass and the base-snapshot recursion. stderr: {}",
        with_relative.stderr
    );
    let json = parse_json(&with_relative);
    assert_eq!(json["verdict"].as_str(), Some("pass"));
}

#[test]
fn audit_coverage_env_fallback_feeds_crap_scoring() {
    let dir = create_audit_fixture("coverage-env");
    write_branchy_change(dir.path());

    let coverage_path = dir.path().join("artifacts/env-coverage.json");
    let branchy_source = dir.path().join("src/index.ts");
    write_branchy_istanbul_coverage(&coverage_path, &branchy_source.to_string_lossy());

    let without_env = run_fallow_raw(&[
        "audit",
        "--root",
        dir.path().to_str().unwrap(),
        "--base",
        "HEAD~1",
        "--max-crap",
        "10",
        "--format",
        "json",
        "--quiet",
    ]);
    assert_eq!(
        without_env.code, 1,
        "static CRAP estimate should fail before FALLOW_COVERAGE is supplied. stderr: {}",
        without_env.stderr
    );

    let output = run_fallow_raw_with_env(
        &[
            "audit",
            "--root",
            dir.path().to_str().unwrap(),
            "--base",
            "HEAD~1",
            "--max-crap",
            "10",
            "--format",
            "json",
            "--quiet",
        ],
        &[("FALLOW_COVERAGE", coverage_path.as_path())],
    );
    assert_eq!(
        output.code, 0,
        "FALLOW_COVERAGE should feed audit's health sub-analysis. stderr: {}",
        output.stderr
    );
    let json = parse_json(&output);
    assert_eq!(json["verdict"].as_str(), Some("pass"));
}

/// Run `fallow audit` against `root` with string env vars set on the child
/// process. The path-typed `run_fallow_raw_with_env` cannot carry a git ref
/// value, so this builds the command directly.
fn run_audit_string_env(
    root: &std::path::Path,
    extra_args: &[&str],
    env: &[(&str, &str)],
) -> common::CommandOutput {
    let mut cmd = Command::new(fallow_bin());
    cmd.env("RUST_LOG", "").env("NO_COLOR", "1");
    for (key, value) in env {
        cmd.env(key, value);
    }
    cmd.args(["audit", "--root"]);
    cmd.arg(root);
    cmd.args(["--format", "json", "--quiet"]);
    cmd.args(extra_args);
    let output = cmd.output().expect("failed to run fallow binary");
    common::CommandOutput {
        stdout: String::from_utf8_lossy(&output.stdout).to_string(),
        stderr: String::from_utf8_lossy(&output.stderr).to_string(),
        code: output.status.code().unwrap_or(-1),
    }
}

/// Add a second commit so `HEAD~1` resolves, then return the fixture.
fn audit_fixture_with_two_commits() -> TempDir {
    let tmp = create_audit_fixture("env-base");
    fs::write(
        tmp.path().join("src/utils.ts"),
        "export const used = () => 43;\nexport const unused = () => 0;\n",
    )
    .unwrap();
    commit_all(tmp.path(), "second commit");
    tmp
}

#[test]
fn audit_honors_fallow_audit_base_env_when_no_flag() {
    let dir = audit_fixture_with_two_commits();
    let output = run_audit_string_env(dir.path(), &[], &[("FALLOW_AUDIT_BASE", "HEAD~1")]);

    assert_eq!(
        output.code, 0,
        "audit with FALLOW_AUDIT_BASE should run. stderr: {}",
        output.stderr
    );
    let json = parse_json(&output);
    assert_eq!(
        json["base_ref"].as_str(),
        Some("HEAD~1"),
        "FALLOW_AUDIT_BASE should set the base ref"
    );
    assert_eq!(
        json["base_description"].as_str(),
        Some("FALLOW_AUDIT_BASE=HEAD~1"),
        "env-set base should carry its provenance"
    );
}

#[test]
fn audit_base_flag_wins_over_fallow_audit_base_env() {
    let dir = audit_fixture_with_two_commits();
    let output = run_audit_string_env(
        dir.path(),
        &["--base", "HEAD"],
        &[("FALLOW_AUDIT_BASE", "HEAD~1")],
    );

    assert_eq!(
        output.code, 0,
        "explicit --base HEAD has no changes, should pass. stderr: {}",
        output.stderr
    );
    let json = parse_json(&output);
    assert_eq!(
        json["base_ref"].as_str(),
        Some("HEAD"),
        "the --base flag must win over FALLOW_AUDIT_BASE"
    );
    assert!(
        json.get("base_description").is_none() || json["base_description"].is_null(),
        "an explicit --base carries no provenance description"
    );
}

#[test]
fn audit_rejects_malformed_fallow_audit_base_env() {
    let dir = audit_fixture_with_two_commits();
    let output = run_audit_string_env(dir.path(), &[], &[("FALLOW_AUDIT_BASE", "bad;ref")]);

    assert_eq!(
        output.code, 2,
        "a malformed FALLOW_AUDIT_BASE must exit 2, not be silently ignored. stderr: {}",
        output.stderr
    );
    let json = parse_json(&output);
    assert_eq!(json["error"].as_bool(), Some(true));
    assert!(
        json["message"]
            .as_str()
            .is_some_and(|m| m.contains("FALLOW_AUDIT_BASE")),
        "the error should name the offending env var, got: {}",
        json["message"]
    );
}

// Base-reuse predicate characterization tests
//
// These tests pin the behavior of `can_reuse_current_as_base` end-to-end
// through the `fallow audit --gate new-only` path. Each test establishes a
// committed base and a committed head, then asserts on the JSON attribution
// fields `dead_code_introduced` and `dead_code_inherited` to confirm whether
// the reuse predicate correctly skipped the base-snapshot rebuild.
//
// They serve as the safety net for refactors of the underlying helpers
// (for example, batching the per-file `git show` calls).

/// A whitespace-only reformat of a TS file must be treated as equivalent by
/// the tokenizer and allow the base snapshot to be reused. The audit should
/// report zero introduced dead-code findings.
#[test]
fn audit_whitespace_only_change_reports_no_introduced_findings() {
    let dir = create_audit_fixture("reuse-whitespace");
    let root = dir.path();
    let base_sha = {
        let output = Command::new("git")
            .args(["rev-parse", "HEAD"])
            .current_dir(root)
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .output()
            .expect("git rev-parse should succeed");
        String::from_utf8_lossy(&output.stdout).trim().to_string()
    };

    // Reformat src/utils.ts with whitespace only (no semantic change).
    fs::write(
        root.join("src/utils.ts"),
        "export const used = () => 42;\n\n\nexport const unused = () => 0;\n",
    )
    .unwrap();
    commit_all(root, "reformat utils");

    let output = run_fallow_raw(&[
        "audit",
        "--root",
        root.to_str().unwrap(),
        "--base",
        &base_sha,
        "--format",
        "json",
        "--quiet",
    ]);

    assert!(
        output.code == 0 || output.code == 1,
        "audit should not crash on whitespace-only change. stderr: {}",
        output.stderr
    );
    let json = parse_json(&output);
    assert_eq!(
        json["attribution"]["dead_code_introduced"].as_u64(),
        Some(0),
        "whitespace-only change must introduce zero dead-code findings. full json: {}",
        serde_json::to_string_pretty(&json).unwrap_or_default()
    );
    // The pre-existing `unused` export should be inherited, not introduced.
    assert!(
        json["attribution"]["dead_code_inherited"]
            .as_u64()
            .is_some_and(|n| n >= 1),
        "pre-existing unused export must appear as inherited"
    );
}

/// Adding a genuinely new unused export must be classified as introduced.
#[test]
fn audit_semantic_change_reports_introduced_finding() {
    let dir = create_audit_fixture("reuse-semantic");
    let root = dir.path();
    let base_sha = {
        let output = Command::new("git")
            .args(["rev-parse", "HEAD"])
            .current_dir(root)
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .output()
            .expect("git rev-parse should succeed");
        String::from_utf8_lossy(&output.stdout).trim().to_string()
    };

    // Add a new unused export.
    fs::write(
        root.join("src/utils.ts"),
        "export const used = () => 42;\nexport const unused = () => 0;\nexport const extra = 1;\n",
    )
    .unwrap();
    commit_all(root, "add extra unused export");

    let output = run_fallow_raw(&[
        "audit",
        "--root",
        root.to_str().unwrap(),
        "--base",
        &base_sha,
        "--format",
        "json",
        "--quiet",
    ]);

    assert!(
        output.code == 0 || output.code == 1,
        "audit should not crash. stderr: {}",
        output.stderr
    );
    let json = parse_json(&output);
    assert!(
        json["attribution"]["dead_code_introduced"]
            .as_u64()
            .is_some_and(|n| n >= 1),
        "new unused export must be attributed as introduced. full json: {}",
        serde_json::to_string_pretty(&json).unwrap_or_default()
    );
}

/// Changing only a Markdown README must not introduce dead-code findings.
/// `is_non_behavioral_doc` classifies `.md` as non-behavioral, so the reuse
/// predicate returns true for a doc-only diff.
#[test]
fn audit_doc_only_change_reports_no_introduced_findings() {
    let dir = create_audit_fixture("reuse-doc");
    let root = dir.path();
    let base_sha = {
        let output = Command::new("git")
            .args(["rev-parse", "HEAD"])
            .current_dir(root)
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .output()
            .expect("git rev-parse should succeed");
        String::from_utf8_lossy(&output.stdout).trim().to_string()
    };

    fs::write(root.join("README.md"), "# My project\nUpdated docs.\n").unwrap();
    commit_all(root, "update readme");

    let output = run_fallow_raw(&[
        "audit",
        "--root",
        root.to_str().unwrap(),
        "--base",
        &base_sha,
        "--format",
        "json",
        "--quiet",
    ]);

    assert!(
        output.code == 0 || output.code == 1,
        "audit should not crash on doc-only change. stderr: {}",
        output.stderr
    );
    let json = parse_json(&output);
    assert_eq!(
        json["attribution"]["dead_code_introduced"].as_u64(),
        Some(0),
        "doc-only change must introduce zero dead-code findings. full json: {}",
        serde_json::to_string_pretty(&json).unwrap_or_default()
    );
}

/// Adding a brand-new TS file with an unused export forces a real base-snapshot
/// computation (the file does not exist in base, so `BaseFileReader::read`
/// returns None and the reuse predicate returns false). The new export should be
/// attributed as introduced.
#[test]
fn audit_new_file_is_treated_as_behavioral() {
    let dir = create_audit_fixture("reuse-newfile");
    let root = dir.path();
    let base_sha = {
        let output = Command::new("git")
            .args(["rev-parse", "HEAD"])
            .current_dir(root)
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .output()
            .expect("git rev-parse should succeed");
        String::from_utf8_lossy(&output.stdout).trim().to_string()
    };

    // Add a new file with an unused export; it has no counterpart in base.
    fs::write(
        root.join("src/new.ts"),
        "export const brandNew = 'nobody uses me';\n",
    )
    .unwrap();
    commit_all(root, "add new file with unused export");

    let output = run_fallow_raw(&[
        "audit",
        "--root",
        root.to_str().unwrap(),
        "--base",
        &base_sha,
        "--format",
        "json",
        "--quiet",
    ]);

    assert!(
        output.code == 0 || output.code == 1,
        "audit should complete successfully even when a new file forces a base-snapshot rebuild. stderr: {}",
        output.stderr
    );
    let json = parse_json(&output);
    assert!(
        json["attribution"]["dead_code_introduced"]
            .as_u64()
            .is_some_and(|n| n >= 1),
        "new unused export in a new file must be attributed as introduced. full json: {}",
        serde_json::to_string_pretty(&json).unwrap_or_default()
    );
}

/// Whitespace-only edits across many `.ts` files in one commit exercise the
/// batched base-file reader: the reuse predicate reads the base version of each
/// changed file sequentially through one `git cat-file --batch` process (the
/// previous implementation spawned one `git show` per file). Each file is
/// token-equivalent to its base, so the reuse check should hold and the audit
/// should introduce zero findings. This pins correctness of multiple
/// sequential reads through a single batch process (trailing-newline
/// consumption, lockstep request/response).
#[test]
fn audit_reuse_check_handles_many_equivalent_files() {
    let dir = create_audit_fixture("reuse-many");
    let root = dir.path();

    // Seed 12 source files that are imported in a chain so each is reachable
    // (no pre-existing unused-file findings), then commit them as the base.
    const FILE_COUNT: usize = 12;
    for i in 0..FILE_COUNT {
        fs::write(
            root.join(format!("src/mod{i}.ts")),
            format!("export const value{i} = {i};\nexport const helper{i} = () => value{i};\n"),
        )
        .unwrap();
    }
    // Wire every module into the import graph via index.ts so none is orphaned.
    use std::fmt::Write as _;
    let mut index = String::from("import { used } from './utils';\nused();\n");
    for i in 0..FILE_COUNT {
        writeln!(index, "import {{ helper{i} }} from './mod{i}';").unwrap();
        writeln!(index, "helper{i}();").unwrap();
    }
    fs::write(root.join("src/index.ts"), &index).unwrap();
    commit_all(root, "seed many modules");

    let base_sha = {
        let output = Command::new("git")
            .args(["rev-parse", "HEAD"])
            .current_dir(root)
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .output()
            .expect("git rev-parse should succeed");
        String::from_utf8_lossy(&output.stdout).trim().to_string()
    };

    // Apply whitespace-only edits to every module in one commit. Each file
    // stays token-equivalent to its base, so the reuse predicate must accept
    // all of them across one batch process.
    for i in 0..FILE_COUNT {
        fs::write(
            root.join(format!("src/mod{i}.ts")),
            format!(
                "export const value{i}  =  {i};\n\nexport const helper{i} = ()   => value{i};\n"
            ),
        )
        .unwrap();
    }
    commit_all(root, "whitespace-only edits across modules");

    let output = run_fallow_raw(&[
        "audit",
        "--root",
        root.to_str().unwrap(),
        "--base",
        &base_sha,
        "--format",
        "json",
        "--quiet",
    ]);

    assert!(
        output.code == 0,
        "audit over many whitespace-only edits should succeed with no introduced findings. code: {}, stderr: {}",
        output.code,
        output.stderr
    );
    let json = parse_json(&output);
    assert_eq!(
        json["attribution"]["dead_code_introduced"].as_u64(),
        Some(0),
        "whitespace-only edits across many files must introduce zero dead-code findings. full json: {}",
        serde_json::to_string_pretty(&json).unwrap_or_default()
    );
}

/// Changing only `package.json` is neither an analysis-input file nor a
/// non-behavioral doc (`.json` passes neither check), so the reuse predicate
/// treats it as behavioral. The audit must complete and produce a coherent
/// verdict; this test checks exit success rather than a specific attribution
/// count because the JSON change may or may not affect dead-code counts.
#[test]
fn audit_json_only_change_is_behavioral() {
    let dir = create_audit_fixture("reuse-json");
    let root = dir.path();
    let base_sha = {
        let output = Command::new("git")
            .args(["rev-parse", "HEAD"])
            .current_dir(root)
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .output()
            .expect("git rev-parse should succeed");
        String::from_utf8_lossy(&output.stdout).trim().to_string()
    };

    // Remove the unused dependency from package.json: a plausible behavioral change.
    fs::write(
        root.join("package.json"),
        r#"{"name": "audit-test", "main": "src/index.ts", "dependencies": {}}"#,
    )
    .unwrap();
    commit_all(root, "remove unused dep from package.json");

    let output = run_fallow_raw(&[
        "audit",
        "--root",
        root.to_str().unwrap(),
        "--base",
        &base_sha,
        "--format",
        "json",
        "--quiet",
    ]);

    // The audit must complete and produce a parseable verdict; it may pass or
    // fail depending on analysis results, but must not crash (exit 2+).
    assert!(
        output.code == 0 || output.code == 1,
        "audit on a package.json-only change must complete without crashing. stderr: {}\nstdout: {}",
        output.stderr,
        output.stdout
    );
    let json = parse_json(&output);
    assert!(
        json.get("verdict").is_some(),
        "audit must produce a verdict field in JSON output. full json: {}",
        serde_json::to_string_pretty(&json).unwrap_or_default()
    );
    // The attribution block must be present.
    assert!(
        json.get("attribution").is_some(),
        "audit must produce an attribution block even when package.json is the only change"
    );
}

/// A Next.js project whose base barrel re-exports only a `"use client"`
/// component. The feature branch adds a server-only re-export to that barrel,
/// turning it into a mixed client/server barrel: the finding is NEW relative to
/// the base, so audit must annotate it `introduced: true`.
fn create_mixed_barrel_audit_fixture() -> TempDir {
    let tmp = TempDir::new().expect("failed to create temp dir");
    let dir = tmp.path();
    fs::create_dir_all(dir.join("app/components")).unwrap();

    fs::write(
        dir.join("package.json"),
        r#"{"name":"audit-mixed-barrel","dependencies":{"next":"^14.0.0","react":"^18.0.0","server-only":"^0.0.1"}}"#,
    )
    .unwrap();
    fs::write(
        dir.join("tsconfig.json"),
        r#"{"compilerOptions":{"target":"ES2022","module":"ESNext","moduleResolution":"bundler","jsx":"preserve"},"include":["app"]}"#,
    )
    .unwrap();
    fs::write(
        dir.join("app/components/Button.tsx"),
        "\"use client\";\nexport function Button() {\n  return null;\n}\n",
    )
    .unwrap();
    fs::write(
        dir.join("app/components/fetchUser.ts"),
        "import \"server-only\";\nexport function fetchUser() {\n  return { id: 1 };\n}\n",
    )
    .unwrap();
    // Base barrel: client-only re-export, NOT a mixed barrel yet.
    fs::write(
        dir.join("app/components/index.ts"),
        "export { Button } from \"./Button\";\n",
    )
    .unwrap();

    let git = |args: &[&str]| {
        Command::new("git")
            .args(args)
            .current_dir(dir)
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            .env("GIT_AUTHOR_NAME", "test")
            .env("GIT_AUTHOR_EMAIL", "test@test.com")
            .env("GIT_COMMITTER_NAME", "test")
            .env("GIT_COMMITTER_EMAIL", "test@test.com")
            .output()
            .expect("git command failed")
    };

    git(&["init", "-b", "main"]);
    git(&["add", "."]);
    git(&["-c", "commit.gpgsign=false", "commit", "-m", "initial"]);
    git(&["checkout", "-b", "feature"]);

    // Feature branch: add the server-only re-export, creating the mix.
    fs::write(
        dir.join("app/components/index.ts"),
        "export { Button } from \"./Button\";\nexport { fetchUser } from \"./fetchUser\";\n",
    )
    .unwrap();
    git(&["add", "."]);
    git(&[
        "-c",
        "commit.gpgsign=false",
        "commit",
        "-m",
        "add server-only re-export to barrel",
    ]);

    tmp
}

#[test]
fn audit_annotates_newly_added_mixed_barrel_as_introduced() {
    let tmp = create_mixed_barrel_audit_fixture();
    let output = run_fallow_raw(&[
        "audit",
        "--root",
        tmp.path().to_str().unwrap(),
        "--base",
        "main",
        "--format",
        "json",
        "--quiet",
    ]);

    let json = parse_json(&output);
    let barrels = json["dead_code"]["mixed_client_server_barrels"]
        .as_array()
        .expect("dead_code.mixed_client_server_barrels should be an array");
    assert_eq!(
        barrels.len(),
        1,
        "exactly one mixed client/server barrel expected. full json: {}",
        serde_json::to_string_pretty(&json).unwrap_or_default()
    );
    assert_eq!(
        barrels[0]["introduced"], true,
        "the newly-mixed barrel must be annotated introduced: true"
    );
}

/// A Next.js project whose base file has a correctly-positioned leading
/// `"use client"` directive. The feature branch adds an import ABOVE the
/// directive, demoting it to an ordinary expression statement the RSC bundler
/// ignores: the finding is NEW relative to the base, so audit must annotate it
/// `introduced: true`.
fn create_misplaced_directive_audit_fixture() -> TempDir {
    let tmp = TempDir::new().expect("failed to create temp dir");
    let dir = tmp.path();
    fs::create_dir_all(dir.join("app")).unwrap();

    fs::write(
        dir.join("package.json"),
        r#"{"name":"audit-misplaced-directive","dependencies":{"next":"^14.0.0","react":"^18.0.0"}}"#,
    )
    .unwrap();
    fs::write(
        dir.join("tsconfig.json"),
        r#"{"compilerOptions":{"target":"ES2022","module":"ESNext","moduleResolution":"bundler","jsx":"preserve"},"include":["app"]}"#,
    )
    .unwrap();
    fs::write(dir.join("app/helper.ts"), "export const helper = 1;\n").unwrap();
    // Base: the directive is correctly positioned at the top of the file.
    fs::write(
        dir.join("app/page.tsx"),
        "\"use client\";\nimport { helper } from \"./helper\";\nexport default function Page() {\n  return helper;\n}\n",
    )
    .unwrap();

    let git = |args: &[&str]| {
        Command::new("git")
            .args(args)
            .current_dir(dir)
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            .env("GIT_AUTHOR_NAME", "test")
            .env("GIT_AUTHOR_EMAIL", "test@test.com")
            .env("GIT_COMMITTER_NAME", "test")
            .env("GIT_COMMITTER_EMAIL", "test@test.com")
            .output()
            .expect("git command failed")
    };

    git(&["init", "-b", "main"]);
    git(&["add", "."]);
    git(&["-c", "commit.gpgsign=false", "commit", "-m", "initial"]);
    git(&["checkout", "-b", "feature"]);

    // Feature branch: move an import above the directive, demoting it.
    fs::write(
        dir.join("app/page.tsx"),
        "import { helper } from \"./helper\";\n\"use client\";\nexport default function Page() {\n  return helper;\n}\n",
    )
    .unwrap();
    git(&["add", "."]);
    git(&[
        "-c",
        "commit.gpgsign=false",
        "commit",
        "-m",
        "move import above use client directive",
    ]);

    tmp
}

#[test]
fn audit_annotates_newly_added_misplaced_directive_as_introduced() {
    let tmp = create_misplaced_directive_audit_fixture();
    let output = run_fallow_raw(&[
        "audit",
        "--root",
        tmp.path().to_str().unwrap(),
        "--base",
        "main",
        "--format",
        "json",
        "--quiet",
    ]);

    let json = parse_json(&output);
    let directives = json["dead_code"]["misplaced_directives"]
        .as_array()
        .expect("dead_code.misplaced_directives should be an array");
    assert_eq!(
        directives.len(),
        1,
        "exactly one misplaced directive expected. full json: {}",
        serde_json::to_string_pretty(&json).unwrap_or_default()
    );
    assert_eq!(
        directives[0]["introduced"], true,
        "the newly-misplaced directive must be annotated introduced: true"
    );
}

// ----------------------------------------------------------------------------
// E5 agent-contract loop (walkthrough guide + walkthrough-file post-validation)
// ----------------------------------------------------------------------------

/// A fixture with two boundary zones (`ui`, `db`) where the diff introduces a
/// new cross-zone edge (ui -> db), so the decision surface emits exactly one
/// real, anchored coupling/boundary decision. The base has no such edge.
fn create_boundary_walkthrough_fixture() -> TempDir {
    let tmp = TempDir::new().expect("temp dir");
    let dir = tmp.path();
    fs::create_dir_all(dir.join("src/ui")).unwrap();
    fs::create_dir_all(dir.join("src/db")).unwrap();

    fs::write(
        dir.join("package.json"),
        r#"{"name": "wt-test", "main": "src/ui/page.ts"}"#,
    )
    .unwrap();
    // Boundary config: ui may import only itself (so importing db is a
    // disallowed cross-zone edge).
    fs::write(
        dir.join(".fallowrc.json"),
        r#"{
  "entry": ["src/ui/page.ts"],
  "boundaries": {
    "zones": [
      { "name": "ui", "patterns": ["src/ui/**"] },
      { "name": "db", "patterns": ["src/db/**"] }
    ],
    "rules": [
      { "from": "ui", "allow": [] }
    ]
  }
}"#,
    )
    .unwrap();
    fs::write(dir.join("src/db/conn.ts"), "export const conn = () => 1;\n").unwrap();
    // Base page.ts does NOT import db.
    fs::write(
        dir.join("src/ui/page.ts"),
        "export const render = () => 'hi';\n",
    )
    .unwrap();

    git(dir, &["init", "-b", "main"]);
    commit_all(dir, "initial");

    // HEAD: page.ts now imports db -> a new cross-zone edge ui->db.
    fs::write(
        dir.join("src/ui/page.ts"),
        "import { conn } from '../db/conn';\nexport const render = () => conn();\n",
    )
    .unwrap();
    commit_all(dir, "ui imports db");

    tmp
}

fn run_walkthrough_guide(root: &Path) -> serde_json::Value {
    let output = run_fallow_raw(&[
        "review",
        "--root",
        root.to_str().unwrap(),
        "--base",
        "main~1",
        "--walkthrough-guide",
        "--format",
        "json",
        "--quiet",
    ]);
    assert_eq!(
        output.code, 0,
        "walkthrough-guide always exits 0. stderr: {}",
        output.stderr
    );
    parse_json(&output)
}

fn run_walkthrough_file(root: &Path, file: &Path) -> serde_json::Value {
    let output = run_fallow_raw(&[
        "review",
        "--root",
        root.to_str().unwrap(),
        "--base",
        "main~1",
        "--walkthrough-file",
        file.to_str().unwrap(),
        "--format",
        "json",
        "--quiet",
    ]);
    assert_eq!(
        output.code, 0,
        "walkthrough-file always exits 0. stderr: {}",
        output.stderr
    );
    parse_json(&output)
}

#[test]
fn e5_walkthrough_guide_pins_a_deterministic_snapshot_hash() {
    let tmp = create_boundary_walkthrough_fixture();
    let guide = run_walkthrough_guide(tmp.path());
    assert_eq!(guide["kind"], "review-walkthrough-guide");
    assert_eq!(guide["command"], "review-walkthrough-guide");
    let hash = guide["graph_snapshot_hash"]
        .as_str()
        .expect("guide pins a graph_snapshot_hash");
    assert!(hash.starts_with("graph:"), "hash is namespaced: {hash}");
    // The digest is graph-derived; the injection note states PR prose is untrusted.
    assert!(
        guide["injection_note"]
            .as_str()
            .unwrap_or_default()
            .contains("untrusted"),
        "injection note documents untrusted PR prose"
    );
    // Re-run on the same tree: the hash is byte-stable (deterministic).
    let again = run_walkthrough_guide(tmp.path());
    assert_eq!(again["graph_snapshot_hash"], guide["graph_snapshot_hash"]);
}

/// The guide emits per-hunk `change_anchors` from the committed diff, and a
/// judgment citing one (with NO signal_id) is ACCEPTED with `anchor_kind: change`,
/// the weaker region-level anchor. End-to-end through the real binary, so the
/// emitted anchor set equals the set validated on reentry.
#[test]
fn e5_change_anchor_judgment_accepts_anchor_kind_change() {
    let tmp = create_boundary_walkthrough_fixture();
    let guide = run_walkthrough_guide(tmp.path());
    let hash = guide["graph_snapshot_hash"].as_str().unwrap().to_string();
    let anchors = guide["change_anchors"]
        .as_array()
        .expect("the guide carries change_anchors");
    assert!(
        !anchors.is_empty(),
        "the page.ts edit must emit at least one change anchor. guide: {}",
        serde_json::to_string_pretty(&guide).unwrap_or_default()
    );
    let anchor_id = anchors[0]["change_anchor"].as_str().unwrap().to_string();
    assert!(anchor_id.starts_with("chg:"), "namespaced: {anchor_id}");

    let agent = serde_json::json!({
        "graph_snapshot_hash": hash,
        "judgments": [
            { "change_anchor": anchor_id, "framing": "This region trades a direct import for a seam." }
        ]
    });
    let agent_path = tmp.path().join("agent_change.json");
    fs::write(&agent_path, serde_json::to_string(&agent).unwrap()).unwrap();

    let validation = run_walkthrough_file(tmp.path(), &agent_path);
    assert_eq!(validation["stale"], false, "matching hash is not stale");
    assert_eq!(
        validation["accepted_count"],
        1,
        "the change-anchored judgment accepts. validation: {}",
        serde_json::to_string_pretty(&validation).unwrap_or_default()
    );
    assert_eq!(validation["accepted"][0]["anchor_kind"], "change");
    assert_eq!(validation["accepted"][0]["signal_id"], "");
    assert_eq!(validation["accepted"][0]["deterministic"], false);

    // A fabricated change anchor is rejected (anti-hallucination for the region anchor).
    let bogus = serde_json::json!({
        "graph_snapshot_hash": guide["graph_snapshot_hash"],
        "judgments": [ { "change_anchor": "chg:deadbeefdeadbeef", "framing": "made up" } ]
    });
    let bogus_path = tmp.path().join("agent_bogus.json");
    fs::write(&bogus_path, serde_json::to_string(&bogus).unwrap()).unwrap();
    let rejected = run_walkthrough_file(tmp.path(), &bogus_path);
    assert_eq!(rejected["rejected_count"], 1, "fabricated region rejects");
    assert_eq!(rejected["rejected"][0]["reason"], "unknown-change-anchor");
}

/// Done-condition (a): a clean agent JSON citing only emitted signal_ids with
/// the correct snapshot hash is ACCEPTED with zero unanchored findings.
#[test]
fn e5_clean_agent_json_is_accepted_zero_unanchored() {
    let tmp = create_boundary_walkthrough_fixture();
    let guide = run_walkthrough_guide(tmp.path());
    let hash = guide["graph_snapshot_hash"].as_str().unwrap().to_string();
    let emitted = guide["digest"]["decisions"]["emitted_signal_ids"]
        .as_array()
        .expect("digest carries the emitted signal_id allowlist");
    assert!(
        !emitted.is_empty(),
        "the boundary change must emit at least one anchored signal. guide: {}",
        serde_json::to_string_pretty(&guide).unwrap_or_default()
    );
    let real_id = emitted[0].as_str().unwrap().to_string();

    let agent = serde_json::json!({
        "graph_snapshot_hash": hash,
        "judgments": [
            { "signal_id": real_id, "framing": "Intended coupling.", "concern": "coupling" }
        ]
    });
    let agent_path = tmp.path().join("agent.json");
    fs::write(&agent_path, serde_json::to_string(&agent).unwrap()).unwrap();

    let validation = run_walkthrough_file(tmp.path(), &agent_path);
    assert_eq!(validation["kind"], "review-walkthrough-validation");
    assert_eq!(validation["stale"], false, "matching hash is not stale");
    assert_eq!(
        validation["accepted_count"], 1,
        "the anchored judgment accepts"
    );
    assert_eq!(validation["rejected_count"], 0, "no rejections");
    assert_eq!(
        validation["unanchored_count"], 0,
        "zero unanchored findings"
    );
    // The framing is fenced as non-deterministic.
    assert_eq!(validation["accepted"][0]["deterministic"], false);
}

/// Done-condition (b): an injected unanchored finding is REJECTED.
#[test]
fn e5_injected_unanchored_signal_is_rejected() {
    let tmp = create_boundary_walkthrough_fixture();
    let guide = run_walkthrough_guide(tmp.path());
    let hash = guide["graph_snapshot_hash"].as_str().unwrap().to_string();

    let agent = serde_json::json!({
        "graph_snapshot_hash": hash,
        "judgments": [
            { "signal_id": "sig:deadbeefdeadbeef", "framing": "hallucinated, no graph anchor" }
        ]
    });
    let agent_path = tmp.path().join("agent.json");
    fs::write(&agent_path, serde_json::to_string(&agent).unwrap()).unwrap();

    let validation = run_walkthrough_file(tmp.path(), &agent_path);
    assert_eq!(validation["stale"], false);
    assert_eq!(
        validation["accepted_count"], 0,
        "the fabricated id never accepts"
    );
    assert_eq!(validation["rejected_count"], 1, "it is rejected");
    assert_eq!(validation["rejected"][0]["reason"], "unanchored-signal-id");
}

/// Done-condition (c): stale JSON (old snapshot hash, e.g. the tree moved) is
/// REFUSED.
#[test]
fn e5_stale_snapshot_hash_is_refused() {
    let tmp = create_boundary_walkthrough_fixture();
    let guide = run_walkthrough_guide(tmp.path());
    let emitted = guide["digest"]["decisions"]["emitted_signal_ids"]
        .as_array()
        .unwrap();
    let real_id = emitted[0].as_str().unwrap().to_string();

    // The agent echoes a STALE hash (the tree moved since the guide was emitted),
    // even though it cites a real signal id.
    let agent = serde_json::json!({
        "graph_snapshot_hash": "graph:0000000000000000",
        "judgments": [
            { "signal_id": real_id, "framing": "would be valid, but the snapshot moved" }
        ]
    });
    let agent_path = tmp.path().join("agent.json");
    fs::write(&agent_path, serde_json::to_string(&agent).unwrap()).unwrap();

    let validation = run_walkthrough_file(tmp.path(), &agent_path);
    assert_eq!(
        validation["stale"], true,
        "the old hash is refused as stale"
    );
    assert_eq!(
        validation["accepted_count"], 0,
        "nothing accepts when stale"
    );
    assert_eq!(validation["rejected"][0]["reason"], "stale-snapshot");
}

// ---------------------------------------------------------------------------
// W2 (#347): `fallow review --walkthrough` human/markdown renderer.
// ---------------------------------------------------------------------------

/// Run `fallow review --walkthrough` (default human) and return the raw output.
fn run_walkthrough_human(root: &Path) -> common::CommandOutput {
    run_fallow_raw(&[
        "review",
        "--root",
        root.to_str().unwrap(),
        "--base",
        "main~1",
        "--walkthrough",
    ])
}

/// Strip the per-run `analysis_run_id` so two walkthrough-guide JSON envelopes
/// from separate processes can be compared (the id is random telemetry meta, not
/// part of the guide contract).
fn strip_run_id(mut value: serde_json::Value) -> serde_json::Value {
    if let Some(telemetry) = value
        .get_mut("_meta")
        .and_then(|m| m.get_mut("telemetry"))
        .and_then(|t| t.as_object_mut())
    {
        telemetry.remove("analysis_run_id");
    }
    value
}

#[test]
fn w2_walkthrough_human_renders_stages_and_badges() {
    let tmp = create_boundary_walkthrough_fixture();
    let output = run_walkthrough_human(tmp.path());
    assert_eq!(
        output.code, 0,
        "walkthrough always exits 0. stderr: {}",
        output.stderr
    );
    // The Review Focus header is orientation on stderr.
    assert!(
        output.stderr.contains("Review Focus"),
        "stderr carries the Review Focus header. stderr: {}",
        output.stderr
    );
    // The tour body is on stdout: at least one stage and the changed file row.
    assert!(
        output.stdout.contains("Stage"),
        "stdout carries a stage header. stdout: {}",
        output.stdout
    );
    assert!(
        output.stdout.contains("page.ts"),
        "the changed page.ts file is in the tour. stdout: {}",
        output.stdout
    );
    // The ui->db boundary decision synthesizes a COUPLING badge.
    assert!(
        output.stdout.contains("COUPLING"),
        "the boundary decision renders a COUPLING badge. stdout: {}",
        output.stdout
    );
}

#[test]
fn w2_walkthrough_json_is_byte_identical_to_guide() {
    let tmp = create_boundary_walkthrough_fixture();
    let walkthrough_json = run_fallow_raw(&[
        "review",
        "--root",
        tmp.path().to_str().unwrap(),
        "--base",
        "main~1",
        "--walkthrough",
        "--format",
        "json",
        "--quiet",
    ]);
    assert_eq!(walkthrough_json.code, 0, "exits 0");
    let guide_json = run_walkthrough_guide(tmp.path());

    let from_walkthrough = parse_json(&walkthrough_json);
    // Same agent-contract kind + deterministic snapshot hash.
    assert_eq!(from_walkthrough["kind"], "review-walkthrough-guide");
    assert_eq!(
        from_walkthrough["graph_snapshot_hash"],
        guide_json["graph_snapshot_hash"],
    );
    // The whole envelope matches once the random per-run id is stripped: the
    // json branch reuses the one guide JSON path, no second serializer.
    assert_eq!(
        strip_run_id(from_walkthrough),
        strip_run_id(guide_json),
        "--walkthrough --format json must reuse the guide JSON path verbatim",
    );
}

#[test]
fn w2_walkthrough_markdown_renders_plain_paste_artifact() {
    let tmp = create_boundary_walkthrough_fixture();
    let output = run_fallow_raw(&[
        "review",
        "--root",
        tmp.path().to_str().unwrap(),
        "--base",
        "main~1",
        "--walkthrough",
        "--format",
        "markdown",
        "--quiet",
    ]);
    assert_eq!(output.code, 0, "exits 0. stderr: {}", output.stderr);
    assert!(
        output.stdout.starts_with("## Fallow Review"),
        "markdown leads with the H2 header. stdout: {}",
        output.stdout
    );
    assert!(
        output.stdout.contains("### Stage"),
        "markdown has a stage section. stdout: {}",
        output.stdout
    );
    assert!(
        output.stdout.contains("`COUPLING`"),
        "badges render as inline code spans. stdout: {}",
        output.stdout
    );
    assert!(
        !output.stdout.contains('\u{1b}'),
        "markdown carries no ANSI escapes. stdout: {}",
        output.stdout
    );
}

#[test]
fn w2_walkthrough_exits_zero_even_when_verdict_fails() {
    let tmp = create_audit_baseline_fixture();
    fs::write(tmp.path().join("fallow.toml"), "[audit]\ngate = \"all\"\n").unwrap();
    let config = tmp.path().join("fallow.toml");

    // Sanity: the plain audit on this fixture really does fail.
    let audit = run_fallow_raw(&[
        "audit",
        "--root",
        tmp.path().to_str().unwrap(),
        "--base",
        "main",
        "--config",
        config.to_str().unwrap(),
        "--format",
        "json",
        "--quiet",
    ]);
    assert_eq!(audit.code, 1, "the underlying audit verdict is fail");

    // All three walkthrough render modes stay exit 0 (advisory surface).
    for format in ["human", "markdown", "json"] {
        let output = run_fallow_raw(&[
            "review",
            "--root",
            tmp.path().to_str().unwrap(),
            "--base",
            "main",
            "--config",
            config.to_str().unwrap(),
            "--walkthrough",
            "--format",
            format,
            "--quiet",
        ]);
        assert_eq!(
            output.code, 0,
            "--walkthrough --format {format} must exit 0 on a fail verdict. stderr: {}",
            output.stderr
        );
    }
}

#[test]
fn w2_walkthrough_cleared_panel_collapses_then_expands() {
    let tmp = create_audit_baseline_fixture();
    // Default human render: the Cleared panel is a single collapsed line.
    let collapsed = run_fallow_raw(&[
        "review",
        "--root",
        tmp.path().to_str().unwrap(),
        "--base",
        "main",
        "--walkthrough",
    ]);
    assert_eq!(collapsed.code, 0, "exits 0. stderr: {}", collapsed.stderr);

    // The baseline fixture may or may not de-prioritize a unit; only assert the
    // collapse/expand contract when a Cleared panel is present.
    if collapsed.stdout.contains("Cleared (") {
        assert!(
            collapsed.stdout.contains("--show-cleared"),
            "collapsed Cleared panel points at --show-cleared. stdout: {}",
            collapsed.stdout
        );
        let expanded = run_fallow_raw(&[
            "review",
            "--root",
            tmp.path().to_str().unwrap(),
            "--base",
            "main",
            "--walkthrough",
            "--show-cleared",
        ]);
        assert_eq!(expanded.code, 0, "exits 0");
        assert!(
            !expanded.stdout.contains("pass --show-cleared to expand"),
            "the expanded panel drops the collapse hint. stdout: {}",
            expanded.stdout
        );
    }
}

#[test]
fn w2_walkthrough_viewed_state_round_trips() {
    let tmp = create_boundary_walkthrough_fixture();
    // First render to learn the current snapshot hash.
    let guide = run_walkthrough_guide(tmp.path());
    let hash = guide["graph_snapshot_hash"].as_str().unwrap().to_string();

    // Write a viewed-state ledger keyed on the changed file.
    let state_dir = tmp.path().join(".fallow");
    fs::create_dir_all(&state_dir).unwrap();
    let state = serde_json::json!({
        "version": 1,
        "schema": "walkthrough-viewed-marks",
        "graph_snapshot_hash": hash,
        "entries": { "src/ui/page.ts": { "viewed_at": "2026-01-01T00:00:00Z" } }
    });
    fs::write(
        state_dir.join("walkthrough-state.json"),
        serde_json::to_string(&state).unwrap(),
    )
    .unwrap();

    let output = run_walkthrough_human(tmp.path());
    assert_eq!(output.code, 0, "exits 0. stderr: {}", output.stderr);
    assert!(
        output.stdout.contains("viewed"),
        "the matched file renders a viewed mark. stdout: {}",
        output.stdout
    );
}

#[test]
fn w2_walkthrough_stale_viewed_state_is_ignored_not_deleted() {
    let tmp = create_boundary_walkthrough_fixture();
    let state_dir = tmp.path().join(".fallow");
    fs::create_dir_all(&state_dir).unwrap();
    let state_path = state_dir.join("walkthrough-state.json");
    // A deliberately WRONG hash: the marks must be ignored on render.
    let state = serde_json::json!({
        "version": 1,
        "schema": "walkthrough-viewed-marks",
        "graph_snapshot_hash": "graph:staleeeeeeeeeeee",
        "entries": { "src/ui/page.ts": { "viewed_at": "2026-01-01T00:00:00Z" } }
    });
    fs::write(&state_path, serde_json::to_string(&state).unwrap()).unwrap();

    let output = run_walkthrough_human(tmp.path());
    assert_eq!(output.code, 0, "exits 0");
    assert!(
        !output.stdout.contains("\u{2713} viewed"),
        "a stale viewed mark must not render. stdout: {}",
        output.stdout
    );
    // The ledger on disk is NOT deleted (carry-forward).
    assert!(
        state_path.exists(),
        "a stale ledger is carried forward, never deleted"
    );
    let on_disk: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&state_path).unwrap()).unwrap();
    assert!(
        on_disk["entries"]["src/ui/page.ts"].is_object(),
        "the stale entry survives on disk"
    );
}

#[test]
fn w2_walkthrough_missing_and_garbled_state_still_exit_zero() {
    let tmp = create_boundary_walkthrough_fixture();
    // No state file: first-run common case.
    let fresh = run_walkthrough_human(tmp.path());
    assert_eq!(fresh.code, 0, "missing state file still exits 0");
    assert!(
        !fresh.stdout.contains("\u{2713} viewed"),
        "no viewed marks without a ledger"
    );

    // A garbled state file must not hard-error the render.
    let state_dir = tmp.path().join(".fallow");
    fs::create_dir_all(&state_dir).unwrap();
    fs::write(state_dir.join("walkthrough-state.json"), b"{ not json").unwrap();
    let garbled = run_walkthrough_human(tmp.path());
    assert_eq!(garbled.code, 0, "garbled state file still exits 0");
}

#[test]
fn w2_walkthrough_mark_viewed_writes_local_ledger() {
    let tmp = create_boundary_walkthrough_fixture();
    let output = run_fallow_raw(&[
        "review",
        "--root",
        tmp.path().to_str().unwrap(),
        "--base",
        "main~1",
        "--walkthrough",
        "--mark-viewed",
        "src/ui/page.ts",
    ]);
    assert_eq!(output.code, 0, "exits 0. stderr: {}", output.stderr);
    let state_path = tmp.path().join(".fallow/walkthrough-state.json");
    assert!(state_path.exists(), "--mark-viewed writes the ledger");
    let state: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&state_path).unwrap()).unwrap();
    assert_eq!(state["version"], 1);
    assert_eq!(state["schema"], "walkthrough-viewed-marks");
    assert!(
        state["entries"]["src/ui/page.ts"].is_object(),
        "the marked file is recorded. state: {state}"
    );
}

#[test]
fn w2_walkthrough_conflicts_with_guide_and_file() {
    let tmp = create_boundary_walkthrough_fixture();
    // --walkthrough + --walkthrough-guide is a clap arg conflict (usage error,
    // distinct from the exit-0 success path).
    let with_guide = run_fallow_raw(&[
        "review",
        "--root",
        tmp.path().to_str().unwrap(),
        "--base",
        "main~1",
        "--walkthrough",
        "--walkthrough-guide",
    ]);
    assert_ne!(
        with_guide.code, 0,
        "--walkthrough + --walkthrough-guide is rejected by clap"
    );

    let with_file = run_fallow_raw(&[
        "review",
        "--root",
        tmp.path().to_str().unwrap(),
        "--base",
        "main~1",
        "--walkthrough",
        "--walkthrough-file",
        "agent.json",
    ]);
    assert_ne!(
        with_file.code, 0,
        "--walkthrough + --walkthrough-file is rejected by clap"
    );
}

/// A fixture whose HEAD diff mixes a load-bearing exported source file (consumed
/// by an UNCHANGED consumer outside the diff), plain source churn, and a
/// NON-source migration file. Exercises the file-accounting + membership fixes:
/// the migration must be surfaced as excluded (not dropped), and no file may
/// appear in both a stage and the Cleared panel.
fn create_mixed_walkthrough_fixture() -> TempDir {
    let tmp = TempDir::new().expect("temp dir");
    let dir = tmp.path();
    fs::create_dir_all(dir.join("src")).unwrap();
    fs::create_dir_all(dir.join("migrations")).unwrap();

    fs::write(
        dir.join("package.json"),
        r#"{"name": "wt-mixed", "main": "src/app.ts"}"#,
    )
    .unwrap();
    fs::write(dir.join(".fallowrc.json"), r#"{"entry": ["src/app.ts"]}"#).unwrap();

    // The consumer imports the contract; it is NOT touched by the HEAD diff, so
    // `lib.ts` is consumed out-of-diff -> load-bearing.
    fs::write(
        dir.join("src/app.ts"),
        "import { value } from './lib';\nexport const run = () => value();\n",
    )
    .unwrap();
    fs::write(dir.join("src/lib.ts"), "export const value = () => 1;\n").unwrap();
    git(dir, &["init", "-b", "main"]);
    commit_all(dir, "initial");

    // HEAD: change the load-bearing contract, add plain source churn, and add a
    // non-source migration file (which must be surfaced as excluded).
    fs::write(
        dir.join("src/lib.ts"),
        "export const value = () => 2;\nexport const extra = () => 3;\n",
    )
    .unwrap();
    fs::write(
        dir.join("src/helper.ts"),
        "export const help = () => 'x';\n",
    )
    .unwrap();
    fs::write(
        dir.join("migrations/0001_init.sql"),
        "CREATE TABLE t (id INTEGER);\n",
    )
    .unwrap();
    commit_all(dir, "change lib, add helper + migration");
    tmp
}

// F1/F2: the rendered Review Focus count reconciles staged + cleared + excluded,
// and the non-source migration file is surfaced as excluded, never dropped.
#[test]
fn w2_walkthrough_surfaces_non_source_and_reconciles_counts() {
    let tmp = create_mixed_walkthrough_fixture();
    let output = run_walkthrough_human(tmp.path());
    assert_eq!(output.code, 0, "exits 0. stderr: {}", output.stderr);
    // The non-source migration is surfaced honestly, not silently dropped.
    assert!(
        output.stderr.contains("non-source not reviewed"),
        "the .sql migration must be surfaced as excluded. stderr: {}",
        output.stderr
    );
    // The accounting sub-line names the staged bucket.
    assert!(
        output.stderr.contains("in stages"),
        "the header breakdown names the staged bucket. stderr: {}",
        output.stderr
    );
    // The migration file never appears as a reviewable stage row in the tour body.
    assert!(
        !output.stdout.contains("0001_init.sql"),
        "the non-source migration is not a stage row. stdout: {}",
        output.stdout
    );
}

// F3: a --mark-viewed file is removed from its stage and shown only under
// Cleared (each file in exactly one place: stage XOR cleared).
#[test]
fn w2_walkthrough_viewed_file_collapses_into_cleared_only() {
    let tmp = create_mixed_walkthrough_fixture();
    // Mark the load-bearing file viewed, then render with the Cleared panel open.
    let marked = run_fallow_raw(&[
        "review",
        "--root",
        tmp.path().to_str().unwrap(),
        "--base",
        "main~1",
        "--walkthrough",
        "--mark-viewed",
        "src/lib.ts",
    ]);
    assert_eq!(
        marked.code, 0,
        "mark-viewed exits 0. stderr: {}",
        marked.stderr
    );

    let output = run_fallow_raw(&[
        "review",
        "--root",
        tmp.path().to_str().unwrap(),
        "--base",
        "main~1",
        "--walkthrough",
        "--show-cleared",
    ]);
    assert_eq!(output.code, 0, "exits 0. stderr: {}", output.stderr);
    let body = &output.stdout;
    let cleared_at = body
        .find("Cleared")
        .unwrap_or_else(|| panic!("cleared panel present. stdout: {body}"));
    let (stage_section, cleared_section) = body.split_at(cleared_at);
    // The viewed file is gone from the stage section and present only in Cleared.
    assert!(
        !stage_section.contains("lib.ts"),
        "the viewed file left its stage. stage section: {stage_section}"
    );
    assert!(
        cleared_section.contains("lib.ts"),
        "the viewed file shows only under Cleared. cleared section: {cleared_section}"
    );
}
