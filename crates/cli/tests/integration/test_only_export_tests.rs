#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests use unwrap and expect to keep fixture setup concise"
)]

use std::path::Path;

use serde_json::Value;

use crate::common::{copy_fixture, parse_json, run_fallow_in_root};

const FIXTURE: &str = "test-only-exports";

fn dead_code(root: &Path, extra: &[&str]) -> Value {
    let mut args = vec!["--format", "json", "--quiet", "--no-cache"];
    args.extend_from_slice(extra);
    parse_json(&run_fallow_in_root("dead-code", root, &args))
}

fn names(json: &Value, key: &str) -> Vec<String> {
    let mut names: Vec<String> = json[key]
        .as_array()
        .map(|findings| {
            findings
                .iter()
                .map(|finding| finding["export_name"].as_str().unwrap().to_owned())
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    names
}

fn finding<'a>(json: &'a Value, name: &str) -> &'a Value {
    json["test_only_exports"]
        .as_array()
        .expect("test_only_exports array")
        .iter()
        .find(|finding| finding["export_name"] == name)
        .unwrap_or_else(|| panic!("no test-only-export finding for {name}"))
}

fn write_config(root: &Path, config: &str) {
    std::fs::write(root.join(".fallowrc.json"), config).expect("write config");
}

#[test]
fn reports_values_that_only_the_test_side_uses() {
    let dir = copy_fixture(FIXTURE);
    let json = dead_code(dir.path(), &[]);
    assert_eq!(
        names(&json, "test_only_exports"),
        [
            "fixtureOnly",
            "helperOnly",
            "storyOnly",
            "testOnly",
            "testOnlyFar",
            "toolSeam",
            "viaBarrel"
        ],
        "a production use, a package entry, an unused-file importer, a kept \
         tag, @expected-unused and a type export must stay silent"
    );
    assert!(
        !names(&json, "unused_exports").contains(&"testOnly".to_owned()),
        "a default-scope run reports a test-only use only as test-only-export"
    );
}

#[test]
fn lists_test_importers_and_colocation() {
    let dir = copy_fixture(FIXTURE);
    let json = dead_code(dir.path(), &[]);

    let colocated = finding(&json, "testOnly");
    assert_eq!(colocated["path"], "src/lib.ts");
    assert_eq!(colocated["line"], 3);
    assert_eq!(
        colocated["test_importers"]["files"],
        serde_json::json!(["src/lib.test.ts"])
    );
    assert_eq!(colocated["colocated_only"], true);

    let far = finding(&json, "testOnlyFar");
    assert_eq!(
        far["test_importers"]["files"],
        serde_json::json!(["tests/lib.test.ts"])
    );
    assert_eq!(far["colocated_only"], false);

    let helper = finding(&json, "helperOnly");
    assert_eq!(
        helper["test_importers"]["files"],
        serde_json::json!(["src/testing/helpers.ts"]),
        "a test helper is a test-side importer"
    );

    let fixture = finding(&json, "fixtureOnly");
    assert_eq!(
        fixture["test_importers"]["files"],
        serde_json::json!(["src/__fixtures__/sample.ts"])
    );

    assert_eq!(finding(&json, "storyOnly")["colocated_only"], true);
}

#[test]
fn barrel_import_reports_the_declaration_only() {
    let dir = copy_fixture(FIXTURE);
    let json = dead_code(dir.path(), &[]);
    let paths: Vec<&str> = json["test_only_exports"]
        .as_array()
        .expect("test_only_exports array")
        .iter()
        .filter(|finding| finding["export_name"] == "viaBarrel")
        .map(|finding| finding["path"].as_str().unwrap())
        .collect();
    assert_eq!(
        paths,
        ["src/barrel/impl.ts"],
        "a test that imports through a barrel gets one finding, on the declaration"
    );
}

#[test]
fn finding_carries_actions_and_identity() {
    let dir = copy_fixture(FIXTURE);
    let json = dead_code(dir.path(), &[]);
    let finding = finding(&json, "testOnly");

    let actions = finding["actions"].as_array().expect("actions array");
    let types: Vec<&str> = actions
        .iter()
        .map(|action| action["type"].as_str().unwrap())
        .collect();
    assert_eq!(
        types,
        [
            "remove-export-and-tests",
            "test-through-interface",
            "suppress-line"
        ]
    );
    assert_eq!(actions[0]["auto_fixable"], false);
    assert_eq!(actions[1]["auto_fixable"], false);
    assert!(
        finding["finding_id"]
            .as_str()
            .unwrap()
            .starts_with("dc1:test-only-export:"),
        "finding_id: {}",
        finding["finding_id"]
    );
}

#[test]
fn test_only_use_keeps_expected_unused_live() {
    let dir = copy_fixture(FIXTURE);
    let json = dead_code(dir.path(), &[]);
    let stale: Vec<&Value> = json["stale_suppressions"]
        .as_array()
        .map(|entries| entries.iter().collect())
        .unwrap_or_default();
    assert!(
        stale
            .iter()
            .all(|entry| !entry.to_string().contains("expectedUnusedTestOnly")),
        "a test-only use must not make @expected-unused stale: {stale:?}"
    );
}

#[test]
fn keep_export_tags_decides_internal_exports() {
    let dir = copy_fixture(FIXTURE);
    write_config(
        dir.path(),
        r#"{ "keepExportTags": ["public"], "rules": { "test-only-exports": "error" } }"#,
    );
    let json = dead_code(dir.path(), &[]);
    assert!(
        names(&json, "test_only_exports").contains(&"internalTestOnly".to_owned()),
        "without @internal in keepExportTags, a test-only @internal export reports"
    );
}

#[test]
fn rule_is_off_by_default() {
    let dir = copy_fixture(FIXTURE);
    write_config(dir.path(), "{}");
    let json = dead_code(dir.path(), &[]);
    assert!(names(&json, "test_only_exports").is_empty());
    assert!(
        !names(&json, "unused_exports").contains(&"testOnly".to_owned()),
        "the default severity changes no unused-export result"
    );
}

#[test]
fn production_scope_reports_unused_export_only() {
    let dir = copy_fixture(FIXTURE);
    let json = dead_code(dir.path(), &["--production"]);
    assert!(
        names(&json, "test_only_exports").is_empty(),
        "a production run reports no test-only-export"
    );
    assert!(names(&json, "unused_exports").contains(&"testOnly".to_owned()));
}

#[test]
fn line_suppression_hides_the_finding() {
    let dir = copy_fixture(FIXTURE);
    let lib = dir.path().join("src/lib.ts");
    let source = std::fs::read_to_string(&lib).unwrap().replace(
        "export const testOnly =",
        "// fallow-ignore-next-line test-only-export\nexport const testOnly =",
    );
    std::fs::write(&lib, source).unwrap();
    let json = dead_code(dir.path(), &[]);
    assert!(!names(&json, "test_only_exports").contains(&"testOnly".to_owned()));
    let stale = json["stale_suppressions"].to_string();
    assert!(
        !stale.contains("test-only-export"),
        "the suppression is consumed: {stale}"
    );
}

#[test]
fn sarif_uses_the_rule_id() {
    let dir = copy_fixture(FIXTURE);
    let output = run_fallow_in_root(
        "dead-code",
        dir.path(),
        &["--format", "sarif", "--quiet", "--no-cache"],
    );
    let sarif = parse_json(&output);
    let rule_ids: Vec<&str> = sarif["runs"][0]["results"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|result| result["ruleId"].as_str())
        .collect();
    assert!(
        rule_ids.contains(&"fallow/test-only-export"),
        "rule ids: {rule_ids:?}"
    );
}

#[test]
fn explain_describes_the_rule() {
    let dir = copy_fixture(FIXTURE);
    let output = run_fallow_in_root("explain", dir.path(), &["test-only-export"]);
    assert_eq!(output.code, 0, "stderr: {}", output.stderr);
    assert!(output.stdout.contains("unused-export"), "{}", output.stdout);
}
