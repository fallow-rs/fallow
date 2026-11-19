#![allow(
    clippy::expect_used,
    reason = "tests use expect to keep fixture setup concise"
)]

//! Each typed MCP route reads the `FALLOW_PRODUCTION*` variables exactly
//! when the CLI command of the tool reads them. Each test sets one variable
//! on the server process and compares the typed route with the CLI command
//! that runs under the same variable. The CLI route of an MCP tool runs that
//! command as a child process, which inherits the environment of the server.

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

/// `fallow list` turns production mode on from the variables and never off,
/// so a variable that is off leaves the `production` config field in charge.
#[test]
fn list_boundaries_typed_route_keeps_the_config_when_fallow_production_is_off() {
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
    assert_eq!(zone_file_count(&cli), 2, "the config production mode wins");
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

fn git(root: &Path, args: &[&str]) {
    let status = Command::new("git")
        .current_dir(root)
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .args([
            "-c",
            "user.name=fallow",
            "-c",
            "user.email=fallow@example.com",
            "-c",
            "commit.gpgsign=false",
        ])
        .args(args)
        .status()
        .expect("run git");
    assert!(status.success(), "git {args:?} failed");
}

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

/// A clone between two story files. Production mode for duplication leaves
/// story files out.
#[test]
fn find_dupes_typed_route_reads_fallow_production_dupes() {
    const BODY: &str = "export function build(items: number[]): number {
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
    let project = tempfile::tempdir().expect("project dir");
    let root = project.path();
    write(
        root,
        "package.json",
        r#"{"name":"production-dupes","private":true,"main":"src/index.ts"}"#,
    );
    write(root, "src/index.ts", "export const used = 1;\n");
    write(root, "src/one.stories.ts", BODY);
    write(root, "src/two.stories.ts", BODY);
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
