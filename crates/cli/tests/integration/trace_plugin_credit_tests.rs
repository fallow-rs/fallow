//! `--trace` and `--trace-file` name the framework plugin that marks an
//! export as used.
//!
//! Before, the trace read only import references. An export that a plugin
//! credits, such as a Next.js page `default` or a Serverless handler, traced
//! as `is_used: false` with no reason, although the analysis did not report it.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests and benches use unwrap and expect to keep fixture setup concise"
)]

use crate::common::{fixture_path, parse_json, run_fallow_in_root};
use serde_json::{Value, json};
use std::path::Path;
use tempfile::TempDir;

fn write(root: &Path, path: &str, contents: &str) {
    let path = root.join(path);
    std::fs::create_dir_all(path.parent().expect("parent")).expect("create dir");
    std::fs::write(path, contents).expect("write file");
}

fn next_project() -> TempDir {
    let dir = TempDir::new().expect("temp project");
    let root = dir.path();
    write(
        root,
        "package.json",
        r#"{ "name": "app", "private": true, "dependencies": { "next": "^15.0.0", "react": "^19.0.0" } }"#,
    );
    write(
        root,
        "app/page.tsx",
        "export default function Page(): string {\n  return 'page';\n}\n\nexport const helper = 1;\n",
    );
    dir
}

fn trace(root: &Path, args: &[&str]) -> Value {
    let mut all = args.to_vec();
    all.extend(["--format", "json", "--quiet", "--no-cache"]);
    parse_json(&run_fallow_in_root("dead-code", root, &all))
}

#[test]
fn an_export_that_a_plugin_rule_credits_traces_as_used() {
    let dir = next_project();
    let root = dir.path();

    let page = trace(root, &["--trace", "app/page.tsx:default"]);
    assert_eq!(page["is_used"], true, "{page:#}");
    assert_eq!(page["plugin_credits"][0]["plugin"], "nextjs", "{page:#}");
    assert!(
        page["reason"].as_str().unwrap().contains("nextjs"),
        "{page:#}"
    );

    let helper = trace(root, &["--trace", "app/page.tsx:helper"]);
    assert_eq!(helper["is_used"], false, "{helper:#}");
    assert!(helper.get("plugin_credits").is_none(), "{helper:#}");
}

#[test]
fn a_file_trace_lists_the_plugin_credit_of_each_export() {
    let dir = next_project();
    let file = trace(dir.path(), &["--trace-file", "app/page.tsx"]);
    let exports = file["exports"].as_array().expect("exports");
    let page = exports
        .iter()
        .find(|export| export["name"] == "default")
        .expect("default export");
    assert_eq!(page["plugin_credits"][0]["plugin"], "nextjs", "{file:#}");
    let helper = exports
        .iter()
        .find(|export| export["name"] == "helper")
        .expect("helper export");
    assert!(helper.get("plugin_credits").is_none(), "{file:#}");
}

#[test]
fn a_serverless_handler_export_traces_as_used_under_include_entry_exports() {
    let root = fixture_path("serverless-framework-plugin");

    let main = trace(
        &root,
        &["--include-entry-exports", "--trace", "src/users.ts:main"],
    );
    assert_eq!(main["is_used"], true, "{main:#}");
    assert_eq!(
        main["plugin_credits"],
        json!([{
            "plugin": "serverless-framework",
            "pattern": "src/users.{js,mjs,cjs,ts,mts,cts}",
        }]),
        "{main:#}"
    );

    // Without the option, the analysis skips the entry file as a whole, and
    // the config-file credit does not apply.
    let default_mode = trace(&root, &["--trace", "src/users.ts:main"]);
    assert!(
        default_mode.get("plugin_credits").is_none(),
        "{default_mode:#}"
    );
}

#[test]
fn a_named_re_export_that_a_plugin_rule_credits_traces_as_used() {
    let dir = next_project();
    let root = dir.path();
    write(
        root,
        "app/api/route.ts",
        "export { GET } from '../../lib/handler';\n",
    );
    write(
        root,
        "lib/handler.ts",
        "export const GET = (): Response => new Response('ok');\n",
    );

    let route = trace(root, &["--trace", "app/api/route.ts:GET"]);
    assert_eq!(route["is_used"], true, "{route:#}");
    assert_eq!(route["plugin_credits"][0]["plugin"], "nextjs", "{route:#}");
}
