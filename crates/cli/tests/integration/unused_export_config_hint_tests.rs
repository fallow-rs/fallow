#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests and benches use unwrap and expect to keep fixture setup concise"
)]

//! Unused exports in a generated file or in a shadcn/ui folder keep their
//! findings and get the config entry that hides them (issue #4912).
//!
//! JSON: each such finding has an `add-to-config` action with an
//! `ignoreExports` rule. A generated file has no auto-fixable action and no
//! inline suppression, because its generator writes the file again.
//! Human: one hint line per group of three findings or more.

use crate::common::{parse_json, run_fallow};
use serde_json::Value;

const FIXTURE: &str = "unused-export-config-hints";

fn unused_exports() -> Vec<Value> {
    let output = run_fallow(
        "dead-code",
        FIXTURE,
        &["--unused-exports", "--format", "json", "--quiet"],
    );
    parse_json(&output)["unused_exports"]
        .as_array()
        .unwrap()
        .clone()
}

fn actions_of<'a>(findings: &'a [Value], path: &str) -> Vec<&'a Value> {
    findings
        .iter()
        .filter(|finding| finding["path"] == path)
        .flat_map(|finding| finding["actions"].as_array().unwrap())
        .collect()
}

fn ignore_exports_files(actions: &[&Value]) -> Vec<String> {
    actions
        .iter()
        .filter(|action| action["type"] == "add-to-config")
        .map(|action| {
            assert_eq!(action["config_key"], "ignoreExports");
            assert_eq!(action["auto_fixable"], false);
            assert_eq!(action["value"][0]["exports"], serde_json::json!(["*"]));
            action["value"][0]["file"].as_str().unwrap().to_owned()
        })
        .collect()
}

#[test]
fn a_generated_file_finding_names_its_ignore_exports_rule_and_is_not_auto_fixable() {
    let findings = unused_exports();
    let actions = actions_of(&findings, "src/api/messages.graphql.ts");

    assert_eq!(
        ignore_exports_files(&actions),
        ["src/api/messages.graphql.ts", "src/api/messages.graphql.ts"],
        "each of the two findings names the file"
    );
    assert!(
        actions.iter().all(|action| action["auto_fixable"] == false),
        "a generator writes the file again, so no action is auto-fixable: {actions:?}"
    );
    assert!(
        actions
            .iter()
            .all(|action| action["type"] != "suppress-line"),
        "the generator removes an inline suppression too: {actions:?}"
    );
}

#[test]
fn an_unused_type_in_a_generated_file_is_not_auto_fixable() {
    let output = run_fallow(
        "dead-code",
        FIXTURE,
        &["--unused-types", "--format", "json", "--quiet"],
    );
    let types = parse_json(&output)["unused_types"]
        .as_array()
        .unwrap()
        .clone();
    let actions = actions_of(&types, "src/api/rooms.graphql.ts");

    assert_eq!(ignore_exports_files(&actions), ["src/api/rooms.graphql.ts"]);
    assert!(
        actions.iter().all(|action| action["auto_fixable"] == false),
        "`fallow fix` keeps a generated file, so no action is auto-fixable: {actions:?}"
    );
}

#[test]
fn a_ui_folder_finding_names_the_folder_rule_and_stays_auto_fixable() {
    let findings = unused_exports();
    let actions = actions_of(&findings, "src/components/ui/dialog.tsx");

    assert_eq!(
        ignore_exports_files(&actions),
        [
            "src/components/ui/**",
            "src/components/ui/**",
            "src/components/ui/**"
        ]
    );
    assert!(
        actions
            .iter()
            .any(|action| action["type"] == "remove-export" && action["auto_fixable"] == true),
        "a person owns the copied file, so the removal stays available"
    );
}

#[test]
fn a_file_that_a_person_wrote_gets_no_config_action() {
    let findings = unused_exports();
    let actions = actions_of(&findings, "src/helpers.ts");

    assert!(!actions.is_empty());
    assert!(ignore_exports_files(&actions).is_empty());
}

#[test]
fn the_human_report_prints_one_hint_for_each_group() {
    let output = run_fallow("dead-code", FIXTURE, &["--unused-exports", "--quiet"]);

    let generated = r#"3 of these exports are in 2 generated files. A generator writes generated code again, so `fallow fix` keeps it. To hide these findings, add to the fallow config: "ignoreExports": [{ "file": "**/*.graphql.ts", "exports": ["*"] }]. Check that the pattern matches only generated files."#;
    let ui = r#"3 of these exports are in the shadcn/ui folder `src/components/ui`. To keep each part of a copied component, add to the fallow config: "ignoreExports": [{ "file": "src/components/ui/**", "exports": ["*"] }]"#;
    assert_eq!(
        output.stdout.matches(generated).count(),
        1,
        "{}",
        output.stdout
    );
    assert_eq!(output.stdout.matches(ui).count(), 1, "{}", output.stdout);
}
