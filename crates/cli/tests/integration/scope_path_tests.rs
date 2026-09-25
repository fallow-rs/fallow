#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests and benches use unwrap and expect to keep fixture setup concise"
)]

use std::fs;

use crate::common::{git, parse_json, run_fallow_in_root};
use tempfile::TempDir;

const DUPLICATED_MODULE: &str = r"export function add(a: number, b: number): number {
  return a + b;
}
export function sub(a: number, b: number): number {
  return a - b;
}
export function mul(a: number, b: number): number {
  return a * b;
}
export function div(a: number, b: number): number {
  return a / b;
}
";

/// Temp project with a duplicated pair under `src/` and a unique module
/// under `other/`. Returns the `TempDir` guard so the directory outlives the
/// caller.
fn create_scope_fixture() -> TempDir {
    let tmp = TempDir::new().expect("failed to create temp dir");
    let dir = tmp.path();
    fs::create_dir_all(dir.join("src")).unwrap();
    fs::create_dir_all(dir.join("other")).unwrap();
    fs::write(dir.join("package.json"), r#"{"name":"scope-test"}"#).unwrap();
    fs::write(dir.join("src/a.ts"), DUPLICATED_MODULE).unwrap();
    fs::write(dir.join("src/b.ts"), DUPLICATED_MODULE).unwrap();
    fs::write(dir.join("other/c.ts"), "export const solo = 1;\n").unwrap();
    tmp
}

/// Temp project with a clone pair straddling the `src/` / `other/` boundary,
/// for the 1-in-rest-out filter variant: a group must survive when ANY
/// instance touches the scope, mirroring the workspace and diff filters.
fn create_cross_scope_fixture() -> TempDir {
    let tmp = TempDir::new().expect("failed to create temp dir");
    let dir = tmp.path();
    fs::create_dir_all(dir.join("src")).unwrap();
    fs::create_dir_all(dir.join("other")).unwrap();
    fs::write(dir.join("package.json"), r#"{"name":"cross-scope-test"}"#).unwrap();
    fs::write(dir.join("src/a.ts"), DUPLICATED_MODULE).unwrap();
    fs::write(dir.join("other/c.ts"), DUPLICATED_MODULE).unwrap();
    tmp
}

fn has_clone_group_with_files(json: &serde_json::Value, expected: &[&str]) -> bool {
    json["clone_groups"]
        .as_array()
        .unwrap()
        .iter()
        .any(|group| {
            expected.iter().all(|file| {
                group["instances"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|instance| instance["file"].as_str() == Some(*file))
            })
        })
}

/// Rendered paths use forward slashes on every platform, but a renderer that
/// regressed to the native separator would make a `!contains("other/c.ts")`
/// assertion pass while the file was on screen. Normalising the captured text
/// once keeps the negative assertions meaningful on Windows: a leaked
/// `other\c.ts` still trips them.
fn slashed(text: &str) -> String {
    text.replace('\\', "/")
}

#[test]
fn dupes_dir_scope_reports_only_scoped_clones() {
    let tmp = create_scope_fixture();
    let scoped = run_fallow_in_root("dupes", tmp.path(), &["--format", "json", "src"]);
    let json = parse_json(&scoped);
    assert!(
        has_clone_group_with_files(&json, &["src/a.ts", "src/b.ts"]),
        "scoped run should report the src clone. stdout: {}",
        scoped.stdout
    );

    let empty = run_fallow_in_root("dupes", tmp.path(), &["--format", "json", "other"]);
    let json = parse_json(&empty);
    assert_eq!(
        json["clone_groups"].as_array().unwrap().len(),
        0,
        "out-of-clone scope should report no groups. stdout: {}",
        empty.stdout
    );
}

#[test]
fn dupes_file_scope_reports_touching_groups() {
    let tmp = create_scope_fixture();
    let output = run_fallow_in_root("dupes", tmp.path(), &["--format", "json", "src/a.ts"]);
    let json = parse_json(&output);
    assert!(
        has_clone_group_with_files(&json, &["src/a.ts", "src/b.ts"]),
        "file scope should keep groups touching the file. stdout: {}",
        output.stdout
    );
}

#[test]
fn dupes_scope_keeps_groups_touching_scope_from_outside() {
    let tmp = create_cross_scope_fixture();
    let from_src = run_fallow_in_root("dupes", tmp.path(), &["--format", "json", "src"]);
    let json = parse_json(&from_src);
    assert!(
        has_clone_group_with_files(&json, &["src/a.ts", "other/c.ts"]),
        "scope should keep groups with any instance inside it. stdout: {}",
        from_src.stdout
    );

    let from_other = run_fallow_in_root("dupes", tmp.path(), &["--format", "json", "other"]);
    let json = parse_json(&from_other);
    assert!(
        has_clone_group_with_files(&json, &["src/a.ts", "other/c.ts"]),
        "scope should keep the same group from the other side. stdout: {}",
        from_other.stdout
    );
}

#[test]
fn dupes_missing_path_is_exit_two() {
    let tmp = create_scope_fixture();
    let output = run_fallow_in_root("dupes", tmp.path(), &["nosuchdir"]);
    assert_eq!(
        output.code, 2,
        "missing PATH should exit 2. stderr: {}",
        output.stderr
    );
    assert!(
        output.stdout.contains("does not exist") || output.stderr.contains("does not exist"),
        "missing PATH should name the problem. stdout: {} stderr: {}",
        output.stdout,
        output.stderr
    );
}

#[test]
fn dupes_outside_root_path_is_exit_two() {
    let tmp = create_scope_fixture();
    let outside = tmp.path().parent().expect("temp dir has a parent");
    let output = run_fallow_in_root(
        "dupes",
        tmp.path(),
        &[outside.to_str().expect("parent is utf-8")],
    );
    assert_eq!(
        output.code, 2,
        "outside-root PATH should exit 2. stderr: {}",
        output.stderr
    );
    assert!(
        output.stdout.contains("outside the project root")
            || output.stderr.contains("outside the project root"),
        "outside-root PATH should name the problem. stdout: {} stderr: {}",
        output.stdout,
        output.stderr
    );
}

#[test]
fn check_file_scope_narrows_to_the_file() {
    let tmp = create_scope_fixture();
    let output = run_fallow_in_root("check", tmp.path(), &["src/a.ts"]);
    let stdout = slashed(&output.stdout);
    assert!(
        stdout.contains("src/a.ts"),
        "scoped check should report the file. stdout: {stdout}"
    );
    assert!(
        !stdout.contains("src/b.ts") && !stdout.contains("other/c.ts"),
        "scoped check should hide other files. stdout: {stdout}"
    );
}

#[test]
fn check_dir_scope_narrows_to_the_directory() {
    let tmp = create_scope_fixture();
    let output = run_fallow_in_root("check", tmp.path(), &["src"]);
    let stdout = slashed(&output.stdout);
    assert!(
        stdout.contains("src/a.ts") || stdout.contains("src/b.ts"),
        "scoped check should report src findings. stdout: {stdout}"
    );
    assert!(
        !stdout.contains("other/c.ts"),
        "scoped check should hide other/ findings. stdout: {stdout}"
    );
}

#[test]
fn bare_combined_path_scope_narrows_report() {
    let tmp = create_scope_fixture();
    let bin = crate::common::fallow_bin();
    let output = std::process::Command::new(&bin)
        .arg("--root")
        .arg(tmp.path())
        .arg("src")
        .env("RUST_LOG", "")
        .env("NO_COLOR", "1")
        .output()
        .expect("failed to run fallow binary");
    let stdout = slashed(&String::from_utf8_lossy(&output.stdout));
    assert!(
        stdout.contains("src/a.ts") || stdout.contains("src/b.ts"),
        "scoped bare run should report src findings. stdout: {stdout}"
    );
    assert!(
        !stdout.contains("other/c.ts"),
        "scoped bare run should hide other/ findings. stdout: {stdout}"
    );
}

#[test]
fn list_files_scope_narrows_inventory() {
    let tmp = create_scope_fixture();
    let output = run_fallow_in_root("list", tmp.path(), &["--files", "src"]);
    assert_eq!(
        output.code, 0,
        "list should succeed. stderr: {}",
        output.stderr
    );
    let stdout = slashed(&output.stdout);
    assert!(
        stdout.contains("src/a.ts") && stdout.contains("src/b.ts"),
        "scoped list should show src files. stdout: {stdout}"
    );
    assert!(
        !stdout.contains("other/c.ts"),
        "scoped list should hide other/ files. stdout: {stdout}"
    );
}

#[test]
fn health_dir_scope_runs_scoped() {
    let tmp = create_scope_fixture();
    let output = run_fallow_in_root("health", tmp.path(), &["--file-scores", "src"]);
    let stdout = slashed(&output.stdout);
    assert!(
        stdout.contains("src/"),
        "scoped health should mention scoped files. stdout: {stdout}"
    );
    assert!(
        !stdout.contains("other/c.ts"),
        "scoped health should hide other/ files. stdout: {stdout}"
    );
}

#[test]
fn fix_dry_run_scope_limits_plan() {
    let tmp = TempDir::new().expect("failed to create temp dir");
    let dir = tmp.path();
    fs::create_dir_all(dir.join("src")).unwrap();
    fs::create_dir_all(dir.join("other")).unwrap();
    fs::write(
        dir.join("package.json"),
        r#"{"name":"fix-test","main":"src/index.ts"}"#,
    )
    .unwrap();
    fs::write(
        dir.join("src/index.ts"),
        "import { used } from './utils';\nused();\n",
    )
    .unwrap();
    fs::write(
        dir.join("src/utils.ts"),
        "export const used = (): number => 42;\nexport const unusedExtra = (): number => 0;\n",
    )
    .unwrap();
    fs::write(
        dir.join("other/stuff.ts"),
        "export const otherUnused = 1;\n",
    )
    .unwrap();

    let scoped = run_fallow_in_root("fix", dir, &["--dry-run", "--format", "json", "src"]);
    let json = parse_json(&scoped);
    let fixes = json["fixes"].as_array().unwrap();
    assert!(
        !fixes.is_empty(),
        "scoped fix plan should contain fixes. stdout: {}",
        scoped.stdout
    );
    assert!(
        fixes.iter().all(|fix| {
            fix["path"]
                .as_str()
                .is_some_and(|path| path.starts_with("src/"))
        }),
        "scoped fix plan should only touch src files. stdout: {}",
        scoped.stdout
    );

    let empty = run_fallow_in_root("fix", dir, &["--dry-run", "--format", "json", "other"]);
    let json = parse_json(&empty);
    assert_eq!(
        json["fixes"].as_array().unwrap().len(),
        0,
        "out-of-scope fix plan should be empty. stdout: {}",
        empty.stdout
    );
}

#[test]
fn audit_scope_narrows_changed_universe() {
    let tmp = create_scope_fixture();
    let dir = tmp.path();
    git(dir, &["init", "-b", "main"]);
    git(dir, &["add", "."]);
    git(
        dir,
        &["-c", "commit.gpgsign=false", "commit", "-m", "initial"],
    );
    fs::write(
        dir.join("src/a.ts"),
        format!("{DUPLICATED_MODULE}export const touched = 1;\n"),
    )
    .unwrap();
    fs::write(
        dir.join("other/c.ts"),
        "export const solo = 1;\nexport const changed = 2;\n",
    )
    .unwrap();

    let output = run_fallow_in_root("audit", dir, &["--gate", "all", "src"]);
    let combined = slashed(&format!("{}\n{}", output.stdout, output.stderr));
    assert!(
        combined.contains("src/a.ts"),
        "scoped audit should report scoped changes. output: {combined}"
    );
    assert!(
        !combined.contains("other/c.ts"),
        "scoped audit should hide out-of-scope changes. output: {combined}"
    );
}

/// `dup` is exported by `src/x.ts`, `src/y.ts` and `src/z.ts`, and
/// `ignoreFindings` matches `src/x.ts` and `src/y.ts`. The full run reports
/// the duplicate export, because `src/z.ts` is not ignored.
fn create_ignored_duplicate_export_fixture() -> TempDir {
    let tmp = TempDir::new().expect("failed to create temp dir");
    let dir = tmp.path();
    fs::create_dir_all(dir.join("src")).unwrap();
    fs::write(
        dir.join("package.json"),
        r#"{"name":"ignored-duplicate-export","main":"src/index.ts"}"#,
    )
    .unwrap();
    fs::write(
        dir.join(".fallowrc.json"),
        r#"{"ignoreFindings":["src/x.ts","src/y.ts"]}"#,
    )
    .unwrap();
    fs::write(
        dir.join("src/index.ts"),
        "export * from \"./x\";\nexport * from \"./y\";\nexport * from \"./z\";\n",
    )
    .unwrap();
    for name in ["x", "y", "z"] {
        fs::write(
            dir.join(format!("src/{name}.ts")),
            "export const dup = 1;\n",
        )
        .unwrap();
    }
    tmp
}

fn duplicate_export_names(json: &serde_json::Value) -> Vec<String> {
    json["duplicate_exports"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|finding| finding["export_name"].as_str().map(str::to_string))
        .collect()
}

#[test]
fn file_filter_hides_a_duplicate_export_that_only_ignored_files_hold() {
    let tmp = create_ignored_duplicate_export_fixture();
    let dir = tmp.path();

    let full = run_fallow_in_root("dead-code", dir, &["--format", "json", "--quiet"]);
    assert_eq!(
        duplicate_export_names(&parse_json(&full)),
        vec!["dup".to_string()],
        "the full run reports `dup`, because src/z.ts is not ignored: {}",
        full.stdout
    );

    let scoped = run_fallow_in_root(
        "dead-code",
        dir,
        &[
            "--file", "src/x.ts", "--file", "src/y.ts", "--format", "json", "--quiet",
        ],
    );
    assert_eq!(
        duplicate_export_names(&parse_json(&scoped)),
        Vec::<String>::new(),
        "after `--file` only src/x.ts and src/y.ts hold `dup` and `ignoreFindings` \
         matches both, so the finding is hidden: {}",
        scoped.stdout
    );
}
