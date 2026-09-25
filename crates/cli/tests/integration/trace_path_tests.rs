//! `fallow trace --path <FROM> <TO>` behavior.
//!
//! The two answers that are NOT errors are the ones worth pinning: an
//! unreachable pair and a same-module pair both report zero hops, and only
//! `reachable` separates them.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "integration tests keep fixture setup concise"
)]

use crate::common::{parse_json, run_fallow_in_root};
use tempfile::tempdir;

/// `index -> feature -> db`, plus `types` reached from `feature` by a
/// type-only import and `orphan` reached by nobody.
fn write_project(root: &std::path::Path) {
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(
        root.join("package.json"),
        r#"{"name":"trace-path-fixture","type":"module"}"#,
    )
    .unwrap();
    std::fs::write(root.join("tsconfig.json"), r#"{"include":["src"]}"#).unwrap();
    std::fs::write(root.join(".fallowrc.json"), r#"{"entry":["src/index.ts"]}"#).unwrap();
    std::fs::write(
        root.join("src/index.ts"),
        "import { boot } from './feature';\nboot();\n",
    )
    .unwrap();
    std::fs::write(
        root.join("src/feature.ts"),
        "import { query } from './db';\nimport type { Row } from './types';\nexport const boot = (): Row => query();\n",
    )
    .unwrap();
    std::fs::write(
        root.join("src/db.ts"),
        "export const query = () => ({ id: 1 });\n",
    )
    .unwrap();
    std::fs::write(
        root.join("src/types.ts"),
        "export type Row = { id: number };\n",
    )
    .unwrap();
    std::fs::write(
        root.join("src/orphan.ts"),
        "export const unused = () => 0;\n",
    )
    .unwrap();
}

#[test]
fn exact_paths_win_over_suffix_matches_and_ambiguous_abbreviations_fail() {
    let dir = tempdir().unwrap();
    write_project(dir.path());
    std::fs::create_dir_all(dir.path().join("packages/x/src")).unwrap();
    std::fs::write(
        dir.path().join("packages/x/src/feature.ts"),
        "export const boot = () => 0;\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("src/index.ts"),
        "import { boot } from './feature';\nimport { boot as duplicate } from '../packages/x/src/feature';\nboot(); duplicate();\n",
    )
    .unwrap();

    for from in [
        "src/feature.ts".to_string(),
        dir.path()
            .join("src/feature.ts")
            .to_string_lossy()
            .into_owned(),
    ] {
        let output = run_fallow_in_root(
            "trace",
            dir.path(),
            &["--path", &from, "src/db.ts", "--format", "json"],
        );
        assert_eq!(output.code, 0, "{}", output.stderr);
        let value = parse_json(&output);
        assert_eq!(value["from"], "src/feature.ts");
        assert_eq!(value["reachable"], true);
    }

    for (from, to) in [("feature.ts", "src/db.ts"), ("src/index.ts", "feature.ts")] {
        let output = run_fallow_in_root(
            "trace",
            dir.path(),
            &["--path", from, to, "--format", "json"],
        );
        assert_eq!(output.code, 2, "{}", output.stdout);
        assert!(
            parse_json(&output)["message"]
                .as_str()
                .unwrap()
                .contains("matches multiple modules")
        );
    }

    let trace_path = dir.path().join("stack.txt");
    std::fs::write(
        &trace_path,
        "Error: broken\n    at boot (src/feature.ts:3:1)\n",
    )
    .unwrap();
    let output = run_fallow_in_root(
        "trace-error",
        dir.path(),
        &["stack.txt", "--format", "json"],
    );
    assert_eq!(output.code, 0, "{}", output.stderr);
    let value = parse_json(&output);
    assert_eq!(value["frames"][0]["resolution"], "resolved");
    assert_eq!(
        value["frames"][0]["candidates"][0]["file"],
        "src/feature.ts"
    );
}

#[test]
fn reports_the_hop_chain_with_each_hops_import_line() {
    let dir = tempdir().unwrap();
    write_project(dir.path());

    let output = run_fallow_in_root(
        "trace",
        dir.path(),
        &["--path", "src/index.ts", "src/db.ts", "--format", "json"],
    );

    assert_eq!(output.code, 0, "stderr:\n{}", output.stderr);
    let value = parse_json(&output);
    assert_eq!(value["kind"], "trace");
    assert_eq!(value["schema_version"], "1");
    assert_eq!(value["reachable"], true);
    assert_eq!(value["hops"], 2);
    assert_eq!(value["path"][0]["from"], "src/index.ts");
    assert_eq!(value["path"][0]["to"], "src/feature.ts");
    assert_eq!(value["path"][0]["import_line"], 1);
    assert_eq!(value["path"][0]["type_only"], false);
    assert_eq!(value["path"][1]["from"], "src/feature.ts");
    assert_eq!(value["path"][1]["to"], "src/db.ts");
    assert_eq!(value["path"][1]["import_line"], 1);
}

#[test]
fn an_unreachable_pair_is_an_answer_not_an_error() {
    let dir = tempdir().unwrap();
    write_project(dir.path());

    let output = run_fallow_in_root(
        "trace",
        dir.path(),
        &["--path", "src/db.ts", "src/index.ts", "--format", "json"],
    );

    assert_eq!(output.code, 0, "stderr:\n{}", output.stderr);
    let value = parse_json(&output);
    assert_eq!(value["reachable"], false);
    assert_eq!(value["hops"], 0);
    assert_eq!(value["path"].as_array().unwrap().len(), 0);
    assert!(
        value["reason"]
            .as_str()
            .unwrap()
            .contains("no import path from src/db.ts to src/index.ts"),
        "reason was {:?}",
        value["reason"]
    );
}

#[test]
fn the_same_module_on_both_sides_is_zero_hops_but_reachable() {
    let dir = tempdir().unwrap();
    write_project(dir.path());

    let output = run_fallow_in_root(
        "trace",
        dir.path(),
        &["--path", "src/db.ts", "src/db.ts", "--format", "json"],
    );

    assert_eq!(output.code, 0, "stderr:\n{}", output.stderr);
    let value = parse_json(&output);
    assert_eq!(value["reachable"], true);
    assert_eq!(value["hops"], 0);
    assert_eq!(value["path"].as_array().unwrap().len(), 0);
}

#[test]
fn a_type_only_hop_is_reported_rather_than_skipped() {
    let dir = tempdir().unwrap();
    write_project(dir.path());

    let output = run_fallow_in_root(
        "trace",
        dir.path(),
        &[
            "--path",
            "src/feature.ts",
            "src/types.ts",
            "--format",
            "json",
        ],
    );

    assert_eq!(output.code, 0, "stderr:\n{}", output.stderr);
    let value = parse_json(&output);
    assert_eq!(value["reachable"], true);
    assert_eq!(value["hops"], 1);
    assert_eq!(value["path"][0]["type_only"], true);
    assert!(
        value["reason"].as_str().unwrap().contains("type-only"),
        "reason was {:?}",
        value["reason"]
    );
}

#[test]
fn an_endpoint_outside_the_graph_exits_two_and_names_which_side() {
    let dir = tempdir().unwrap();
    write_project(dir.path());

    let missing_from = run_fallow_in_root(
        "trace",
        dir.path(),
        &["--path", "src/nope.ts", "src/db.ts", "--format", "json"],
    );
    assert_eq!(missing_from.code, 2);
    assert!(
        missing_from
            .stdout
            .contains("--path from module 'src/nope.ts'"),
        "stdout was {}",
        missing_from.stdout
    );

    let missing_to = run_fallow_in_root(
        "trace",
        dir.path(),
        &["--path", "src/db.ts", "src/nope.ts", "--format", "json"],
    );
    assert_eq!(missing_to.code, 2);
    assert!(
        missing_to.stdout.contains("--path to module 'src/nope.ts'"),
        "stdout was {}",
        missing_to.stdout
    );
}

#[test]
fn repeated_runs_return_a_byte_identical_route() {
    let dir = tempdir().unwrap();
    write_project(dir.path());

    let args = &["--path", "src/index.ts", "src/db.ts", "--format", "json"];
    let first = parse_json(&run_fallow_in_root("trace", dir.path(), args));
    let second = parse_json(&run_fallow_in_root("trace", dir.path(), args));

    assert_eq!(first["path"], second["path"]);
    assert_eq!(first["hops"], second["hops"]);
}

/// `hops: 0` is the answer for BOTH an unreachable pair and the same module on
/// both sides, so the human header cannot leave `reachable` to the JSON.
#[test]
fn the_human_header_separates_unreachable_from_same_module() {
    let dir = tempdir().unwrap();
    write_project(dir.path());

    let unreachable = run_fallow_in_root(
        "trace",
        dir.path(),
        &["--path", "src/orphan.ts", "src/db.ts", "--quiet"],
    );
    assert_eq!(unreachable.code, 0, "stderr:\n{}", unreachable.stderr);
    assert!(
        unreachable.stdout.contains("reachable: no"),
        "stdout was {}",
        unreachable.stdout
    );
    assert!(
        unreachable.stdout.contains("hops:      0"),
        "stdout was {}",
        unreachable.stdout
    );

    let same = run_fallow_in_root(
        "trace",
        dir.path(),
        &["--path", "src/db.ts", "src/db.ts", "--quiet"],
    );
    assert_eq!(same.code, 0, "stderr:\n{}", same.stderr);
    assert!(
        same.stdout.contains("reachable: yes"),
        "stdout was {}",
        same.stdout
    );
    assert!(
        same.stdout.contains("hops:      0"),
        "stdout was {}",
        same.stdout
    );
}

/// A failure that names an unknown module without saying how to find a known
/// one leaves the caller guessing at the path spelling.
#[test]
fn an_unknown_endpoint_carries_a_remedy() {
    let dir = tempdir().unwrap();
    write_project(dir.path());

    let human = run_fallow_in_root("trace", dir.path(), &["--path", "src/nope.ts", "src/db.ts"]);
    assert_eq!(human.code, 2);
    assert!(
        human
            .stderr
            .contains("hint: pass a path relative to the project root"),
        "stderr was {}",
        human.stderr
    );

    let json = run_fallow_in_root(
        "trace",
        dir.path(),
        &["--path", "src/nope.ts", "src/db.ts", "--format", "json"],
    );
    assert_eq!(json.code, 2);
    let value: serde_json::Value = serde_json::from_str(json.stdout.trim()).unwrap();
    assert!(
        value["help"]
            .as_str()
            .is_some_and(|help| help.contains("fallow list --files")),
        "the JSON error must carry the remedy: {}",
        json.stdout
    );
}

#[test]
fn human_output_anchors_the_line_on_the_importing_file() {
    let dir = tempdir().unwrap();
    write_project(dir.path());

    let output = run_fallow_in_root(
        "trace",
        dir.path(),
        &["--path", "src/index.ts", "src/feature.ts"],
    );

    assert_eq!(output.code, 0, "stderr:\n{}", output.stderr);
    assert!(
        output.stdout.contains("src/index.ts:1 -> src/feature.ts"),
        "stdout was {}",
        output.stdout
    );
}

#[test]
fn the_path_form_rejects_the_call_chain_flags() {
    let dir = tempdir().unwrap();
    write_project(dir.path());

    let output = run_fallow_in_root(
        "trace",
        dir.path(),
        &[
            "--path",
            "src/index.ts",
            "src/db.ts",
            "--callers",
            "--format",
            "json",
        ],
    );

    assert_eq!(output.code, 2);
    assert!(
        output.stdout.contains("cannot be used with"),
        "stdout was {}",
        output.stdout
    );
}
