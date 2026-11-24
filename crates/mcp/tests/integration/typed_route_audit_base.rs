#![allow(
    clippy::expect_used,
    reason = "tests use expect to keep fixture setup concise"
)]

//! The typed route and the CLI route of the MCP `audit` tool pick the same
//! base: the `base` parameter, then `FALLOW_AUDIT_BASE`, then the detected
//! base. The variables are set on the server process, so the typed route
//! reads them from its environment snapshot and the CLI route from the
//! environment that the child process inherits. `explain_skipped` sends the
//! call to the CLI route. The typed `decision_surface` tool uses the same
//! order, so `FALLOW_CHANGED_SINCE` does not replace its base either.

use std::path::Path;

use crate::typed_route_env_coverage::McpServer;

use fallow_api::git_fixture::fixture_git as git;

fn commit(root: &Path, message: &str) {
    git(root, &["add", "."]);
    git(
        root,
        &[
            "-c",
            "user.email=test@example.com",
            "-c",
            "user.name=Test",
            "-c",
            "commit.gpgsign=false",
            "commit",
            "-q",
            "-m",
            message,
        ],
    );
}

/// A repository on the branch `work` with two commits. The tag `first` and a
/// local `main` branch name the first commit, so the audit detects `main`.
fn base_repo() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("repo");
    let root = dir.path();
    std::fs::create_dir_all(root.join("src")).expect("src");
    std::fs::write(
        root.join("package.json"),
        r#"{"name":"base-order","type":"module","main":"src/index.ts"}"#,
    )
    .expect("package.json");
    std::fs::write(root.join("src/index.ts"), "console.log('entry');\n").expect("entry");
    git(root, &["init", "-q", "-b", "work"]);
    commit(root, "first");
    git(root, &["tag", "first"]);
    git(root, &["branch", "main"]);
    std::fs::write(root.join("src/feature.ts"), "export const unused = 1;\n").expect("feature");
    commit(root, "second");
    dir
}

/// The `base_ref` of an `audit` call on a server started under `env`. An
/// empty value unsets a variable of the test process.
fn audit_base(root: &Path, env: &[(&str, &str)], extra: &serde_json::Value) -> String {
    let mut arguments = serde_json::json!({
        "root": root.display().to_string(),
        "no_cache": true,
        "gate": "all",
    });
    for (key, value) in extra.as_object().expect("object") {
        arguments[key] = value.clone();
    }
    let mut server = McpServer::start_with_env(env);
    let result = server.call_raw("audit", &arguments);
    let text = result["content"][0]["text"].as_str().expect("text content");
    let payload: serde_json::Value = serde_json::from_str(text).expect("tool payload");
    payload["base_ref"]
        .as_str()
        .unwrap_or_else(|| panic!("audit must name a base: {payload}"))
        .to_owned()
}

fn both_routes(root: &Path, env: &[(&str, &str)], params: &serde_json::Value) -> [String; 2] {
    let mut cli = params.clone();
    cli["explain_skipped"] = serde_json::Value::Bool(true);
    [audit_base(root, env, params), audit_base(root, env, &cli)]
}

#[test]
fn typed_and_cli_routes_prefer_the_base_parameter() {
    let repo = base_repo();
    let env = [
        ("FALLOW_AUDIT_BASE", "main"),
        ("FALLOW_CHANGED_SINCE", "HEAD"),
    ];
    let bases = both_routes(repo.path(), &env, &serde_json::json!({ "base": "first" }));
    assert_eq!(bases, ["first", "first"]);
}

#[test]
fn typed_and_cli_routes_prefer_fallow_audit_base_over_fallow_changed_since() {
    let repo = base_repo();
    let env = [
        ("FALLOW_AUDIT_BASE", "first"),
        ("FALLOW_CHANGED_SINCE", "HEAD"),
    ];
    let bases = both_routes(repo.path(), &env, &serde_json::json!({}));
    assert_eq!(bases, ["first", "first"]);
}

#[test]
fn typed_and_cli_routes_prefer_the_detected_base_over_fallow_changed_since() {
    let repo = base_repo();
    let env = [("FALLOW_AUDIT_BASE", ""), ("FALLOW_CHANGED_SINCE", "HEAD")];
    let bases = both_routes(repo.path(), &env, &serde_json::json!({}));
    assert_eq!(bases, ["main", "main"]);
}

/// [`base_repo`] with a third commit that adds the dependency `left-pad`.
/// A decision surface against `first` or `main` has a decision for the new
/// dependency. A decision surface against `HEAD` has no change and no
/// decision.
fn dependency_repo() -> tempfile::TempDir {
    let dir = base_repo();
    let root = dir.path();
    let package = root.join("node_modules/left-pad");
    std::fs::create_dir_all(&package).expect("node_modules");
    std::fs::write(
        package.join("package.json"),
        r#"{"name":"left-pad","version":"1.0.0","main":"index.js"}"#,
    )
    .expect("left-pad manifest");
    std::fs::write(package.join("index.js"), "module.exports = () => 1;\n").expect("left-pad");
    std::fs::write(
        root.join("package.json"),
        "{\n  \"name\": \"base-order\",\n  \"type\": \"module\",\n  \"main\": \"src/index.ts\",\n  \"dependencies\": {\n    \"left-pad\": \"^1.3.0\"\n  }\n}\n",
    )
    .expect("package.json");
    std::fs::write(
        root.join("src/index.ts"),
        "import leftPad from 'left-pad';\nconsole.log(leftPad('x', 2));\n",
    )
    .expect("entry");
    commit(root, "add left-pad");
    dir
}

/// The `signal_key` of each decision of a `decision_surface` call on a
/// server started under `env`.
fn decision_keys(root: &Path, env: &[(&str, &str)]) -> Vec<String> {
    let arguments = serde_json::json!({
        "root": root.display().to_string(),
        "no_cache": true,
    });
    let mut server = McpServer::start_with_env(env);
    let result = server.call_raw("decision_surface", &arguments);
    let text = result["content"][0]["text"].as_str().expect("text content");
    let payload: serde_json::Value = serde_json::from_str(text).expect("tool payload");
    payload["decisions"]
        .as_array()
        .unwrap_or_else(|| panic!("decision_surface must list decisions: {payload}"))
        .iter()
        .filter_map(|decision| decision["signal_key"].as_str().map(str::to_owned))
        .collect()
}

#[test]
fn typed_decision_surface_prefers_fallow_audit_base_over_fallow_changed_since() {
    let repo = dependency_repo();
    let env = [
        ("FALLOW_AUDIT_BASE", "first"),
        ("FALLOW_CHANGED_SINCE", "HEAD"),
    ];
    let keys = decision_keys(repo.path(), &env);
    assert!(
        keys.iter().any(|key| key == "package.json::left-pad"),
        "{keys:?}"
    );
}

#[test]
fn typed_decision_surface_prefers_the_detected_base_over_fallow_changed_since() {
    let repo = dependency_repo();
    let env = [("FALLOW_AUDIT_BASE", ""), ("FALLOW_CHANGED_SINCE", "HEAD")];
    let keys = decision_keys(repo.path(), &env);
    assert!(
        keys.iter().any(|key| key == "package.json::left-pad"),
        "{keys:?}"
    );
}
