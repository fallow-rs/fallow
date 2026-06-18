#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "integration tests keep fixture setup concise"
)]

#[path = "common/mod.rs"]
mod common;

use common::{parse_json, run_fallow_in_root};
use tempfile::tempdir;

fn write_project(root: &std::path::Path) {
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(
        root.join("package.json"),
        r#"{"name":"inspect-fixture","type":"module"}"#,
    )
    .unwrap();
    std::fs::write(root.join("tsconfig.json"), r#"{"include":["src"]}"#).unwrap();
    std::fs::write(root.join(".fallowrc.json"), r#"{"entry":["src/index.ts"]}"#).unwrap();
    std::fs::write(
        root.join("src/index.ts"),
        "import { fetchUser } from './api';\nexport const boot = () => fetchUser('1');\n",
    )
    .unwrap();
    std::fs::write(
        root.join("src/api.ts"),
        "export const fetchUser = (id: string) => ({ id });\n",
    )
    .unwrap();
}

#[test]
fn inspect_file_outputs_typed_evidence_bundle() {
    let dir = tempdir().unwrap();
    write_project(dir.path());

    let output = run_fallow_in_root(
        "inspect",
        dir.path(),
        &["--file", "src/api.ts", "--format", "json", "--quiet"],
    );
    assert_eq!(output.code, 0, "inspect should exit 0: {}", output.stderr);

    let json = parse_json(&output);
    assert_eq!(json["kind"].as_str(), Some("inspect_target"));
    assert_eq!(json["target"]["type"].as_str(), Some("file"));
    assert_eq!(json["target"]["file"].as_str(), Some("src/api.ts"));
    assert_eq!(json["identity"]["file"].as_str(), Some("src/api.ts"));
    assert_eq!(
        json["evidence"]["trace_file"]["status"].as_str(),
        Some("ok")
    );
    assert_eq!(
        json["evidence"]["dead_code"]["scope"].as_str(),
        Some("file")
    );
    assert_eq!(
        json["evidence"]["duplication"]["scope"].as_str(),
        Some("project_filtered_to_file")
    );
}

#[test]
fn inspect_symbol_outputs_trace_export_section() {
    let dir = tempdir().unwrap();
    write_project(dir.path());

    let output = run_fallow_in_root(
        "inspect",
        dir.path(),
        &[
            "--symbol",
            "src/api.ts:fetchUser",
            "--format",
            "json",
            "--quiet",
        ],
    );
    assert_eq!(output.code, 0, "inspect should exit 0: {}", output.stderr);

    let json = parse_json(&output);
    assert_eq!(json["kind"].as_str(), Some("inspect_target"));
    assert_eq!(json["target"]["type"].as_str(), Some("symbol"));
    assert_eq!(json["target"]["export_name"].as_str(), Some("fetchUser"));
    assert_eq!(json["identity"]["export_name"].as_str(), Some("fetchUser"));
    assert_eq!(
        json["evidence"]["trace_export"]["status"].as_str(),
        Some("ok")
    );
    assert!(
        json["warnings"]
            .as_array()
            .is_some_and(|items| !items.is_empty())
    );
}
