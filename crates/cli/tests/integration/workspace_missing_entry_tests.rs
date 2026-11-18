#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests use unwrap and expect to keep fixture setup concise"
)]

use std::path::Path;

use crate::common::{parse_json, run_fallow_in_root};

fn write(root: &Path, relative: &str, content: &str) {
    let path = root.join(relative);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, content).unwrap();
}

fn package_names(json: &serde_json::Value, key: &str, field: &str) -> Vec<String> {
    json[key]
        .as_array()
        .unwrap_or_else(|| panic!("{key} is an array: {json}"))
        .iter()
        .map(|entry| entry[field].as_str().unwrap().to_owned())
        .collect()
}

/// A native build creates the `main` file of `@scope/native`, so the file is
/// missing before the build. The import still names a declared workspace
/// package, so the dependency is used. See #4717.
#[test]
fn import_of_workspace_package_with_missing_main_credits_the_dependency() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let root = dir.path();
    write(
        root,
        "package.json",
        r#"{"name":"root","private":true,"workspaces":["native","app"]}"#,
    );
    write(
        root,
        "native/package.json",
        r#"{"name":"@scope/native","version":"1.0.0","main":"index.js","types":"index.d.ts"}"#,
    );
    write(
        root,
        "app/package.json",
        r#"{"name":"app","version":"1.0.0","main":"src/index.ts","dependencies":{"@scope/native":"workspace:*"}}"#,
    );
    write(
        root,
        "app/src/index.ts",
        "import { Scanner } from \"@scope/native\";\n\nexport const scanner = Scanner;\n",
    );

    let output = run_fallow_in_root(
        "dead-code",
        root,
        &["--format", "json", "--quiet", "--no-cache"],
    );
    let json = parse_json(&output);

    assert!(
        package_names(&json, "unused_dependencies", "package_name").is_empty(),
        "{json}"
    );
    assert_eq!(
        package_names(&json, "unresolved_imports", "specifier"),
        ["@scope/native"],
        "{json}"
    );
}
