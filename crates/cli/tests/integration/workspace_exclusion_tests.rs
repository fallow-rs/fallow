#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests and benches use unwrap and expect to keep fixture setup concise"
)]

//! `--workspace '!<nested>'` excludes a workspace below a selected workspace.

use std::collections::BTreeSet;
use std::fs;

use crate::common::{parse_json, run_fallow_in_root};
use tempfile::TempDir;

const EXCLUDE_PLUGIN: [&str; 4] = ["--workspace", "apps/web", "--workspace", "!apps/web/plugin"];

/// A monorepo with `apps/web`, the nested workspace `apps/web/plugin`, and
/// `apps/docs`. Each workspace reads one feature flag, has one suppression in
/// its entry, and has one unused file.
fn nested_monorepo() -> TempDir {
    let dir = tempfile::tempdir().expect("temp dir");
    let root = dir.path();
    fs::write(
        root.join("package.json"),
        r#"{"name":"ws-root","private":true,"workspaces":["apps/**"]}"#,
    )
    .unwrap();
    for (rel, name) in [
        ("apps/web", "web"),
        ("apps/web/plugin", "plugin"),
        ("apps/docs", "docs"),
    ] {
        let ws = root.join(rel);
        fs::create_dir_all(ws.join("src")).unwrap();
        fs::write(
            ws.join("package.json"),
            format!(r#"{{"name":"{name}","main":"src/index.ts"}}"#),
        )
        .unwrap();
        fs::write(
            ws.join("src/index.ts"),
            "const flag = process.env.FEATURE_ONE;\n// fallow-ignore-next-line unused-export\nexport const kept = flag ? 1 : 0;\n",
        )
        .unwrap();
        fs::write(ws.join("src/unused.ts"), "export const x = 1;\n").unwrap();
    }
    dir
}

fn run(root: &std::path::Path, subcommand: &str, extra: &[&str]) -> serde_json::Value {
    let mut args = vec!["--no-cache", "--format", "json", "--quiet"];
    args.extend_from_slice(extra);
    parse_json(&run_fallow_in_root(subcommand, root, &args))
}

fn paths(items: &serde_json::Value) -> Vec<String> {
    items
        .as_array()
        .expect("array of items")
        .iter()
        .filter_map(|item| item["path"].as_str().map(str::to_owned))
        .collect()
}

#[test]
fn an_excluded_nested_workspace_is_not_in_the_suppression_inventory() {
    let dir = nested_monorepo();
    let json = run(dir.path(), "suppressions", &EXCLUDE_PLUGIN);
    assert_eq!(paths(&json["files"]), vec!["apps/web/src/index.ts"]);
}

#[test]
fn an_excluded_nested_workspace_has_no_flag_rows() {
    let dir = nested_monorepo();
    let json = run(dir.path(), "flags", &EXCLUDE_PLUGIN);
    let rows: BTreeSet<String> = paths(&json["feature_flags"]).into_iter().collect();
    assert_eq!(rows, BTreeSet::from(["apps/web/src/index.ts".to_owned()]));
}

#[test]
fn an_excluded_nested_workspace_has_no_dead_code_findings() {
    let dir = nested_monorepo();
    let json = run(dir.path(), "dead-code", &EXCLUDE_PLUGIN);
    let files = paths(&json["unused_files"]);
    assert!(
        files.contains(&"apps/web/src/unused.ts".to_owned()),
        "{files:?}"
    );
    assert!(
        !files
            .iter()
            .any(|path| path.starts_with("apps/web/plugin/")),
        "{files:?}"
    );
}

#[test]
fn an_unnamed_nested_workspace_stays_in_scope() {
    let dir = nested_monorepo();
    let json = run(dir.path(), "suppressions", &["--workspace", "apps/web"]);
    let files = paths(&json["files"]);
    assert!(
        files.contains(&"apps/web/src/index.ts".to_owned()),
        "{files:?}"
    );
    assert!(
        files.contains(&"apps/web/plugin/src/index.ts".to_owned()),
        "{files:?}"
    );
}

#[test]
fn an_excluded_nested_workspace_is_not_analyzed_by_health() {
    let dir = nested_monorepo();
    let selected = run(dir.path(), "health", &["--workspace", "apps/web"]);
    let excluded = run(dir.path(), "health", &EXCLUDE_PLUGIN);
    assert_eq!(selected["summary"]["files_analyzed"], 4, "{selected}");
    assert_eq!(excluded["summary"]["files_analyzed"], 2, "{excluded}");
}
