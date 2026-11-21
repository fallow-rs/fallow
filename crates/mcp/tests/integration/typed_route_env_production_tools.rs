#![allow(
    clippy::expect_used,
    reason = "tests use expect to keep fixture setup concise"
)]

//! Each typed MCP route reads the `FALLOW_PRODUCTION*` variables exactly
//! when the CLI command of the tool reads them. Each test sets one variable
//! on the server process and compares the typed route with the CLI command
//! that runs under the same variable. The CLI route of an MCP tool runs that
//! command as a child process, which inherits the environment of the server.
//!
//! Each `fallow-api` runner names its own programmatic entry, and the
//! resolver reads the variables from the environment policy row of the
//! command of that entry. These tests run the real runner of each tool, so a
//! runner with the entry of another command fails them.

use std::path::Path;
use std::process::Command;

use crate::typed_route_env_coverage::McpServer;

const PRODUCTION_VARIABLES: [&str; 4] = [
    "FALLOW_PRODUCTION",
    "FALLOW_PRODUCTION_DEAD_CODE",
    "FALLOW_PRODUCTION_HEALTH",
    "FALLOW_PRODUCTION_DUPES",
];

/// The JSON output of one `fallow` command under `env` only, with no other
/// production variable from the test process.
fn cli_json(root: &Path, env: &[(&str, &str)], args: &[&str]) -> serde_json::Value {
    let mut command = Command::new(crate::fallow_binary());
    for name in PRODUCTION_VARIABLES {
        command.env_remove(name);
    }
    command.envs(env.iter().copied());
    let output = command
        .current_dir(root)
        .arg("--root")
        .arg(root)
        .args(args)
        .args(["--format", "json", "--quiet", "--no-cache"])
        .output()
        .expect("run fallow");
    serde_json::from_slice(&output.stdout).unwrap_or_else(|err| {
        panic!(
            "fallow {args:?} must print JSON ({err}): {}",
            String::from_utf8_lossy(&output.stdout)
        )
    })
}

/// The JSON payload of one MCP tool call on a server started under `env`.
fn typed_json(
    env: &[(&str, &str)],
    tool: &str,
    root: &Path,
    extra: &serde_json::Value,
) -> serde_json::Value {
    let mut arguments = serde_json::json!({"root": root.display().to_string(), "no_cache": true});
    for (key, value) in extra.as_object().expect("object") {
        arguments[key] = value.clone();
    }
    let mut server = McpServer::start_with_env(env);
    let result = server.call_raw(tool, &arguments);
    assert_ne!(result["isError"], true, "{tool} must succeed: {result}");
    let text = result["content"][0]["text"].as_str().expect("text content");
    serde_json::from_str(text).expect("tool payload")
}

fn write(root: &Path, path: &str, contents: &str) {
    let path = root.join(path);
    std::fs::create_dir_all(path.parent().expect("parent")).expect("create dir");
    std::fs::write(path, contents).expect("write file");
}

/// A zone with one source file, one helper and one test file that imports
/// the helper. Production mode leaves the test file out of the graph.
fn write_zone_project(root: &Path) {
    write(
        root,
        "package.json",
        r#"{"name":"production-zone","private":true,"main":"src/app/index.ts"}"#,
    );
    write(
        root,
        ".fallowrc.json",
        r#"{"boundaries":{"zones":[{"name":"app","patterns":["src/app/**"]}],"rules":[]}}"#,
    );
    write(
        root,
        "src/app/index.ts",
        "import { b } from \"./b\";\nexport const a = b;\n",
    );
    write(root, "src/app/b.ts", "export const b = 1;\n");
    write(
        root,
        "src/app/a.test.ts",
        "import { b } from \"./b\";\nexport const t = b;\n",
    );
}

fn zone_file_count(envelope: &serde_json::Value) -> u64 {
    envelope["boundaries"]["zones"][0]["file_count"]
        .as_u64()
        .unwrap_or_else(|| panic!("file_count: {envelope}"))
}

#[test]
fn list_boundaries_typed_route_reads_fallow_production() {
    let project = tempfile::tempdir().expect("project dir");
    let root = project.path();
    write_zone_project(root);
    let env = [("FALLOW_PRODUCTION", "1")];

    let cli = cli_json(root, &env, &["list", "--boundaries"]);
    let typed = typed_json(&env, "list_boundaries", root, &serde_json::json!({}));
    assert_eq!(zone_file_count(&cli), 2, "the CLI leaves the test file out");
    assert_eq!(zone_file_count(&typed), zone_file_count(&cli));
}

/// `fallow list` reads the variables before the config, so a variable that is
/// off turns off the `production` config field.
#[test]
fn list_boundaries_typed_route_lets_an_off_fallow_production_beat_the_config() {
    let project = tempfile::tempdir().expect("project dir");
    let root = project.path();
    write_zone_project(root);
    write(
        root,
        ".fallowrc.json",
        r#"{"production":true,"boundaries":{"zones":[{"name":"app","patterns":["src/app/**"]}],"rules":[]}}"#,
    );
    let env = [("FALLOW_PRODUCTION", "0")];

    let cli = cli_json(root, &env, &["list", "--boundaries"]);
    let typed = typed_json(&env, "list_boundaries", root, &serde_json::json!({}));
    assert_eq!(
        zone_file_count(&cli),
        3,
        "the variable turns production off"
    );
    assert_eq!(zone_file_count(&typed), zone_file_count(&cli));
}

/// `fallow trace --path` ignores the production variables, so the typed
/// `trace_import_path` route must ignore them too.
#[test]
fn trace_import_path_ignores_fallow_production_like_the_cli() {
    let project = tempfile::tempdir().expect("project dir");
    let root = project.path();
    write_zone_project(root);
    let env = [("FALLOW_PRODUCTION", "1")];

    let cli = cli_json(
        root,
        &env,
        &["trace", "--path", "src/app/a.test.ts", "src/app/b.ts"],
    );
    let typed = typed_json(
        &env,
        "trace_import_path",
        root,
        &serde_json::json!({"from": "src/app/a.test.ts", "to": "src/app/b.ts"}),
    );
    assert_eq!(cli["reachable"], true, "{cli}");
    assert_eq!(typed["reachable"], cli["reachable"], "{typed}");
    assert_eq!(typed["hops"], cli["hops"], "{typed}");
}

/// `fallow trace-error` ignores the production variables, so the typed
/// `trace_error` route must ignore them too.
#[test]
fn trace_error_ignores_fallow_production_like_the_cli() {
    let project = tempfile::tempdir().expect("project dir");
    write_zone_project(project.path());
    let root = std::fs::canonicalize(project.path()).expect("canonical root");
    let trace = format!(
        "Error: boom\n    at t ({}:1:30)\n",
        root.join("src/app/a.test.ts").display()
    );
    let scratch = tempfile::tempdir().expect("scratch dir");
    let trace_file = scratch.path().join("trace.txt");
    std::fs::write(&trace_file, &trace).expect("write trace");
    let env = [("FALLOW_PRODUCTION", "1")];

    let cli = cli_json(
        &root,
        &env,
        &["trace-error", &trace_file.display().to_string()],
    );
    let typed = typed_json(
        &env,
        "trace_error",
        &root,
        &serde_json::json!({"trace": trace}),
    );
    assert_eq!(cli["frames"][0]["origin"], "in_project", "{cli}");
    assert_eq!(typed["frames"][0]["origin"], cli["frames"][0]["origin"]);
    assert_eq!(
        typed["frames"][0]["resolution"],
        cli["frames"][0]["resolution"]
    );
}

/// A helper and a package that only a test file uses. The `vitest`
/// dependency makes the test file an entry point, so production mode
/// leaves the helper and the package without a user.
fn write_test_only_project(root: &Path) {
    write(
        root,
        "package.json",
        r#"{"name":"production-trace","private":true,"main":"src/index.ts","dependencies":{"lodash":"4.17.21"},"devDependencies":{"vitest":"1.0.0"}}"#,
    );
    write(root, "src/index.ts", "export const used = 1;\n");
    write(root, "src/helper.ts", "export const helper = 2;\n");
    write(
        root,
        "src/helper.test.ts",
        "import { helper } from \"./helper\";\nimport { chunk } from \"lodash\";\nexport const t = chunk([helper], 1);\n",
    );
}

/// The `from_file` values of the `direct_references` of an export trace.
fn reference_files(trace: &serde_json::Value) -> Vec<String> {
    trace["direct_references"]
        .as_array()
        .unwrap_or_else(|| panic!("direct_references: {trace}"))
        .iter()
        .filter_map(|reference| reference["from_file"].as_str().map(str::to_owned))
        .collect()
}

/// The CLI route of `trace_export` runs `fallow dead-code --trace`, which
/// reads `FALLOW_PRODUCTION`.
#[test]
fn trace_export_typed_route_reads_fallow_production() {
    let project = tempfile::tempdir().expect("project dir");
    let root = project.path();
    write_test_only_project(root);
    let env = [("FALLOW_PRODUCTION", "1")];

    let cli = cli_json(
        root,
        &env,
        &["dead-code", "--trace", "src/helper.ts:helper"],
    );
    let typed = typed_json(
        &env,
        "trace_export",
        root,
        &serde_json::json!({"file": "src/helper.ts", "export_name": "helper"}),
    );
    assert_eq!(
        reference_files(&cli),
        Vec::<String>::new(),
        "the CLI leaves the test file out: {cli}"
    );
    assert_eq!(reference_files(&typed), reference_files(&cli), "{typed}");
    assert_eq!(typed["is_used"], cli["is_used"], "{typed}");
}

/// The CLI route of `trace_file` runs `fallow dead-code --trace-file`, which
/// reads `FALLOW_PRODUCTION`.
#[test]
fn trace_file_typed_route_reads_fallow_production() {
    let project = tempfile::tempdir().expect("project dir");
    let root = project.path();
    write_test_only_project(root);
    let env = [("FALLOW_PRODUCTION", "1")];

    let cli = cli_json(root, &env, &["dead-code", "--trace-file", "src/helper.ts"]);
    let typed = typed_json(
        &env,
        "trace_file",
        root,
        &serde_json::json!({"file": "src/helper.ts"}),
    );
    assert_eq!(
        cli["imported_by"],
        serde_json::json!([]),
        "the CLI leaves the test file out: {cli}"
    );
    assert_eq!(typed["imported_by"], cli["imported_by"], "{typed}");
    assert_eq!(typed["is_reachable"], cli["is_reachable"], "{typed}");
}

/// The CLI route of `trace_dependency` runs `fallow dead-code
/// --trace-dependency`, which reads `FALLOW_PRODUCTION`.
#[test]
fn trace_dependency_typed_route_reads_fallow_production() {
    let project = tempfile::tempdir().expect("project dir");
    let root = project.path();
    write_test_only_project(root);
    let env = [("FALLOW_PRODUCTION", "1")];

    let cli = cli_json(root, &env, &["dead-code", "--trace-dependency", "lodash"]);
    let typed = typed_json(
        &env,
        "trace_dependency",
        root,
        &serde_json::json!({"package_name": "lodash"}),
    );
    assert_eq!(
        cli["imported_by"],
        serde_json::json!([]),
        "the CLI leaves the test file out: {cli}"
    );
    assert_eq!(typed["imported_by"], cli["imported_by"], "{typed}");
    assert_eq!(typed["is_used"], cli["is_used"], "{typed}");
}

/// Two zones and a test file in `app` that imports `lib` against the rules
/// and reads a feature flag. Production mode leaves the test file out.
fn write_test_violation_project(root: &Path) {
    write(
        root,
        "package.json",
        r#"{"name":"production-arch","private":true,"main":"src/app/index.ts","devDependencies":{"vitest":"1.0.0"}}"#,
    );
    write(
        root,
        ".fallowrc.json",
        r#"{"boundaries":{"zones":[{"name":"app","patterns":["src/app/**"]},{"name":"lib","patterns":["src/lib/**"]}],"rules":[{"from":"app","allow":[]}]}}"#,
    );
    write(root, "src/app/index.ts", "export const a = 1;\n");
    write(root, "src/lib/l.ts", "export const l = 1;\n");
    write(
        root,
        "src/app/a.test.ts",
        "import { l } from \"../lib/l\";\nif (process.env.FEATURE_NEW_UI) { console.log(l); }\nexport const t = l;\n",
    );
}

/// The CLI route of `check_architecture` runs `fallow architecture`, which
/// reads `FALLOW_PRODUCTION`.
#[test]
fn check_architecture_typed_route_reads_fallow_production() {
    let project = tempfile::tempdir().expect("project dir");
    let root = project.path();
    write_test_violation_project(root);
    let env = [("FALLOW_PRODUCTION", "1")];

    let cli = cli_json(root, &env, &["architecture"]);
    let typed = typed_json(&env, "check_architecture", root, &serde_json::json!({}));
    assert_eq!(
        cli["boundary_violations"],
        serde_json::json!([]),
        "the CLI leaves the test file out: {cli}"
    );
    assert_eq!(
        typed["boundary_violations"], cli["boundary_violations"],
        "{typed}"
    );
}

/// The CLI route of `feature_flags` runs `fallow flags`, which reads
/// `FALLOW_PRODUCTION`.
#[test]
fn feature_flags_typed_route_reads_fallow_production() {
    let project = tempfile::tempdir().expect("project dir");
    let root = project.path();
    write_test_violation_project(root);
    let env = [("FALLOW_PRODUCTION", "1")];

    let cli = cli_json(root, &env, &["flags"]);
    let typed = typed_json(&env, "feature_flags", root, &serde_json::json!({}));
    assert_eq!(
        cli["total_flags"], 0,
        "the CLI leaves the test file out: {cli}"
    );
    assert_eq!(typed["total_flags"], cli["total_flags"], "{typed}");
}

use fallow_api::git_fixture::fixture_git as git;

fn unused_file_paths(section: &serde_json::Value) -> Vec<String> {
    section["unused_files"]
        .as_array()
        .unwrap_or_else(|| panic!("unused_files: {section}"))
        .iter()
        .filter_map(|finding| finding["path"].as_str().map(str::to_owned))
        .collect()
}

/// The change adds a test file that nothing imports. Production mode for
/// dead code leaves it out of the audit.
#[test]
fn audit_typed_route_reads_fallow_production_dead_code() {
    let project = tempfile::tempdir().expect("project dir");
    let root = project.path();
    write(
        root,
        "package.json",
        r#"{"name":"production-audit","private":true,"main":"src/index.ts"}"#,
    );
    write(root, "src/index.ts", "export const used = 1;\n");
    git(root, &["init", "-q"]);
    git(root, &["add", "-A"]);
    git(root, &["commit", "-q", "-m", "base"]);
    write(root, "src/orphan.test.ts", "export const helper = 2;\n");
    git(root, &["add", "-A"]);
    let env = [("FALLOW_PRODUCTION_DEAD_CODE", "1")];

    let cli = cli_json(root, &env, &["audit", "--base", "HEAD"]);
    let typed = typed_json(&env, "audit", root, &serde_json::json!({"base": "HEAD"}));
    assert_eq!(
        unused_file_paths(&cli["dead_code"]),
        Vec::<String>::new(),
        "the CLI leaves the test file out: {cli}"
    );
    assert_eq!(
        unused_file_paths(&typed["dead_code"]),
        unused_file_paths(&cli["dead_code"])
    );
}

/// The Code Mode `combined` tool runs in process. Bare `fallow` is the
/// command of its CLI route.
#[test]
fn combined_typed_route_reads_fallow_production() {
    let project = tempfile::tempdir().expect("project dir");
    let root = project.path();
    write(
        root,
        "package.json",
        r#"{"name":"production-combined","private":true,"main":"src/index.ts"}"#,
    );
    write(root, "src/index.ts", "export const used = 1;\n");
    write(root, "src/orphan.test.ts", "export const helper = 2;\n");
    let env = [("FALLOW_PRODUCTION", "1")];

    let cli = cli_json(root, &env, &[]);
    let envelope = typed_json(
        &env,
        "code_execute",
        root,
        &serde_json::json!({"code": "return fallow.combined({ no_cache: true });"}),
    );
    assert_eq!(envelope["ok"], true, "{envelope}");
    let typed = &envelope["result"];
    assert_eq!(
        unused_file_paths(&cli["check"]),
        Vec::<String>::new(),
        "the CLI leaves the test file out: {cli}"
    );
    assert_eq!(
        unused_file_paths(&typed["check"]),
        unused_file_paths(&cli["check"])
    );
    assert_eq!(
        typed["health"]["summary"]["files_analyzed"],
        cli["health"]["summary"]["files_analyzed"]
    );
}

/// A function that is long enough for a clone group.
const CLONE_BODY: &str = "export function build(items: number[]): number {
  let total = 0;
  for (const item of items) {
    if (item > 10) {
      total += item * 2;
    } else if (item > 5) {
      total += item + 3;
    } else {
      total -= item;
    }
  }
  const scaled = total * 7;
  const shifted = scaled - 11;
  return shifted > 100 ? shifted / 2 : shifted * 3;
}
";

/// A clone between two story files. Production mode for duplication leaves
/// story files out.
#[test]
fn find_dupes_typed_route_reads_fallow_production_dupes() {
    let project = tempfile::tempdir().expect("project dir");
    let root = project.path();
    write(
        root,
        "package.json",
        r#"{"name":"production-dupes","private":true,"main":"src/index.ts"}"#,
    );
    write(root, "src/index.ts", "export const used = 1;\n");
    write(root, "src/one.stories.ts", CLONE_BODY);
    write(root, "src/two.stories.ts", CLONE_BODY);
    let clone_groups = |envelope: &serde_json::Value| -> u64 {
        envelope["stats"]["clone_groups"]
            .as_u64()
            .unwrap_or_else(|| panic!("clone_groups: {envelope}"))
    };

    let without = cli_json(root, &[], &["dupes"]);
    assert_eq!(clone_groups(&without), 1, "the fixture has one clone");
    let env = [("FALLOW_PRODUCTION_DUPES", "1")];
    let cli = cli_json(root, &env, &["dupes"]);
    let typed = typed_json(&env, "find_dupes", root, &serde_json::json!({}));
    assert_eq!(clone_groups(&cli), 0, "the CLI leaves story files out");
    assert_eq!(clone_groups(&typed), clone_groups(&cli));
}

/// The CLI route of `trace_clone` runs `fallow dupes --trace`, which reads
/// `FALLOW_PRODUCTION_DUPES`. The clone has one instance in a story file.
#[test]
fn trace_clone_typed_route_reads_fallow_production_dupes() {
    let project = tempfile::tempdir().expect("project dir");
    let root = project.path();
    write(
        root,
        "package.json",
        r#"{"name":"production-clone","private":true,"main":"src/index.ts"}"#,
    );
    write(root, "src/index.ts", "export const used = 1;\n");
    write(root, "src/one.ts", CLONE_BODY);
    write(root, "src/two.ts", CLONE_BODY);
    write(root, "src/three.stories.ts", CLONE_BODY);
    let instances = |trace: &serde_json::Value| -> usize {
        trace["clone_groups"][0]["instances"]
            .as_array()
            .unwrap_or_else(|| panic!("instances: {trace}"))
            .len()
    };
    let env = [("FALLOW_PRODUCTION_DUPES", "1")];

    let cli = cli_json(root, &env, &["dupes", "--trace", "src/one.ts:2"]);
    let typed = typed_json(
        &env,
        "trace_clone",
        root,
        &serde_json::json!({"file": "src/one.ts", "line": 2}),
    );
    assert_eq!(
        instances(&cli),
        2,
        "the CLI leaves the story file out: {cli}"
    );
    assert_eq!(instances(&typed), instances(&cli), "{typed}");
}

/// A repository whose change adds a test file that nothing imports and that
/// imports a new dependency. Production mode leaves the test file out, so the
/// dependency has no user.
fn write_changed_test_file_project(root: &Path) {
    write(
        root,
        "package.json",
        r#"{"name":"production-changed","private":true,"main":"src/index.ts"}"#,
    );
    write(root, "src/index.ts", "export const used = 1;\n");
    git(root, &["init", "-q"]);
    git(root, &["add", "-A"]);
    git(root, &["commit", "-q", "-m", "base"]);
    write(
        root,
        "package.json",
        r#"{"name":"production-changed","private":true,"main":"src/index.ts","dependencies":{"lodash":"4.17.21"}}"#,
    );
    write(
        root,
        "src/orphan.test.ts",
        "import { chunk } from \"lodash\";\nexport const helper = chunk([1], 1);\n",
    );
    git(root, &["add", "-A"]);
}

/// The CLI route of `check_changed` runs `fallow dead-code --changed-since`,
/// which reads `FALLOW_PRODUCTION`.
#[test]
fn check_changed_typed_route_reads_fallow_production() {
    let project = tempfile::tempdir().expect("project dir");
    let root = project.path();
    write_changed_test_file_project(root);
    let env = [("FALLOW_PRODUCTION", "1")];

    let cli = cli_json(root, &env, &["dead-code", "--changed-since", "HEAD"]);
    let typed = typed_json(
        &env,
        "check_changed",
        root,
        &serde_json::json!({"since": "HEAD"}),
    );
    assert_eq!(
        unused_file_paths(&cli),
        Vec::<String>::new(),
        "the CLI leaves the test file out: {cli}"
    );
    assert_eq!(
        unused_file_paths(&typed),
        unused_file_paths(&cli),
        "{typed}"
    );
}

/// The CLI route of `project_info` runs `fallow list`, which reads
/// `FALLOW_PRODUCTION`.
#[test]
fn project_info_typed_route_reads_fallow_production() {
    let project = tempfile::tempdir().expect("project dir");
    let root = project.path();
    write_zone_project(root);
    let env = [("FALLOW_PRODUCTION", "1")];

    let cli = cli_json(root, &env, &["list", "--files"]);
    let typed = typed_json(
        &env,
        "project_info",
        root,
        &serde_json::json!({"files": true}),
    );
    assert_eq!(
        cli["file_count"], 2,
        "the CLI leaves the test file out: {cli}"
    );
    assert_eq!(typed["file_count"], cli["file_count"], "{typed}");
    assert_eq!(typed["files"], cli["files"], "{typed}");
}

/// The question of the dependency decision of a decision surface.
fn dependency_question(surface: &serde_json::Value) -> String {
    surface["decisions"]
        .as_array()
        .unwrap_or_else(|| panic!("decisions: {surface}"))
        .iter()
        .find(|decision| decision["category"] == "dependency")
        .and_then(|decision| decision["question"].as_str())
        .unwrap_or_else(|| panic!("a dependency decision: {surface}"))
        .to_owned()
}

/// The typed `decision_surface` tool reads no `FALLOW_PRODUCTION*` variable:
/// the test file stays a user of the new dependency. The tool has no CLI
/// route. `fallow decision-surface` reads the variables.
#[test]
fn decision_surface_typed_route_ignores_fallow_production() {
    let project = tempfile::tempdir().expect("project dir");
    let root = project.path();
    write_changed_test_file_project(root);

    let base = serde_json::json!({"base": "HEAD"});
    let with_env = typed_json(
        &[("FALLOW_PRODUCTION", "1")],
        "decision_surface",
        root,
        &base,
    );
    let without_env = typed_json(
        &[("FALLOW_PRODUCTION", "")],
        "decision_surface",
        root,
        &base,
    );
    assert!(
        dependency_question(&with_env).contains("imported by 1 in-repo module"),
        "{with_env}"
    );
    assert_eq!(
        dependency_question(&with_env),
        dependency_question(&without_env)
    );
}
