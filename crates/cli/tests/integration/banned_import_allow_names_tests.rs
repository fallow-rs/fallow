//! `allowNames` on `banned-import` rules (issue #4072).
//!
//! The list applies to the names that a statically visible import or
//! re-export binds. A namespace, star and side-effect form always reports.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests use expect to keep fixture setup concise"
)]

use std::path::Path;

use crate::common::{CommandOutput, commit_all, git, parse_json, run_fallow_in_root};
use serde_json::{Value, json};
use tempfile::TempDir;

const MATHS_ONLY: &str = r#"["Vector3", "Quaternion"]"#;

fn write_pack(root: &Path, rule_fields: &str) {
    std::fs::write(
        root.join("pack.json"),
        format!(
            r#"{{ "version": 1, "name": "repro", "rules": [
                {{ "id": "maths-only", "kind": "banned-import", "specifiers": ["three"]{rule_fields} }}
            ] }}"#
        ),
    )
    .expect("write pack");
}

fn project(source: &str, rule_fields: &str) -> TempDir {
    let dir = TempDir::new().expect("temp project");
    let root = dir.path();
    std::fs::create_dir_all(root.join("src")).expect("src dir");
    std::fs::write(
        root.join("package.json"),
        r#"{"name":"repro","private":true,"main":"src/tick.ts","dependencies":{"three":"0.187.0"}}"#,
    )
    .expect("package.json");
    std::fs::write(
        root.join(".fallowrc.json"),
        r#"{ "rulePacks": ["pack.json"], "rules": { "policy-violation": "warn" } }"#,
    )
    .expect("config");
    std::fs::write(root.join("src/tick.ts"), source).expect("source");
    write_pack(root, rule_fields);
    dir
}

fn allow(names: &str) -> String {
    format!(r#", "allowNames": {names}"#)
}

fn run_policy(root: &Path, extra: &[&str]) -> CommandOutput {
    let mut args = vec![
        "--policy-violations",
        "--format",
        "json",
        "--quiet",
        "--no-cache",
    ];
    args.extend_from_slice(extra);
    run_fallow_in_root("dead-code", root, &args)
}

/// `(line, matched)` of each policy finding, in report order.
fn findings(json: &Value) -> Vec<(u64, String)> {
    json["policy_violations"]
        .as_array()
        .expect("policy_violations array")
        .iter()
        .map(|finding| {
            (
                finding["line"].as_u64().expect("line"),
                finding["matched"].as_str().expect("matched").to_owned(),
            )
        })
        .collect()
}

fn check(source: &str, names: &str) -> Vec<(u64, String)> {
    let dir = project(source, &allow(names));
    let output = run_policy(dir.path(), &[]);
    assert_eq!(output.code, 0, "stderr: {}", output.stderr);
    findings(&parse_json(&output))
}

fn expected(rows: &[(u64, &str)]) -> Vec<(u64, String)> {
    rows.iter()
        .map(|(line, matched)| (*line, (*matched).to_owned()))
        .collect()
}

#[test]
fn issue_repro_reports_only_the_unlisted_name() {
    let source = "import { Vector3 } from 'three';\nimport { Mesh } from 'three';\n\nexport const up = new Vector3(0, 1, 0);\nexport const body = Mesh;\n";
    assert_eq!(check(source, MATHS_ONLY), expected(&[(2, "three: Mesh")]));
}

#[test]
fn compares_the_source_side_name_of_an_alias() {
    let source =
        "import { Vector3 as V } from 'three';\nimport { Mesh as Vector3 } from 'three';\n";
    assert_eq!(check(source, MATHS_ONLY), expected(&[(2, "three: Mesh")]));
}

#[test]
fn exporting_an_imported_binding_is_a_second_site_with_the_source_side_name() {
    let source = "import { Mesh as Vector3 } from 'three';\nexport { Vector3 };\n";
    assert_eq!(
        check(source, MATHS_ONLY),
        expected(&[(1, "three: Mesh"), (2, "three: Mesh")])
    );
}

#[test]
fn one_statement_gives_one_finding_per_unlisted_binding() {
    let source = "import { Vector3, Mesh, Group } from 'three';\n";
    assert_eq!(
        check(source, MATHS_ONLY),
        expected(&[(1, "three: Mesh"), (1, "three: Group")])
    );
}

#[test]
fn namespace_star_and_side_effect_forms_always_report() {
    let source = "import * as T from 'three';\nimport type * as TT from 'three';\nimport 'three';\nimport {} from 'three';\nexport * from 'three';\nexport * as NS from 'three';\n";
    assert_eq!(
        check(source, MATHS_ONLY),
        expected(&[
            (1, "three: *"),
            (2, "three: *"),
            (3, "three: side-effect"),
            (4, "three: side-effect"),
            (5, "three: *"),
            (6, "three: *"),
        ])
    );
}

#[test]
fn default_import_forms_share_the_default_name() {
    let source = "import D from 'three';\nimport { default as E } from 'three';\nexport { default as F } from 'three';\n";
    assert_eq!(
        check(source, MATHS_ONLY),
        expected(&[
            (1, "three: default"),
            (2, "three: default"),
            (3, "three: default"),
        ])
    );
    assert_eq!(check(source, r#"["default"]"#), expected(&[]));
}

#[test]
fn re_exports_follow_the_list() {
    let source =
        "export { Vector3 as V } from 'three';\nexport { Mesh, Group as G } from 'three';\n";
    assert_eq!(
        check(source, MATHS_ONLY),
        expected(&[(2, "three: Mesh"), (2, "three: Group")])
    );
}

#[test]
fn subpath_uses_the_same_list_and_names_the_written_specifier() {
    let source = "import { OrbitControls } from 'three/addons/x.js';\nimport { Vector3 } from 'three/addons/y.js';\n";
    assert_eq!(
        check(source, MATHS_ONLY),
        expected(&[(1, "three/addons/x.js: OrbitControls")])
    );
}

#[test]
fn type_only_binding_is_a_separate_site_and_ignore_type_only_skips_it() {
    let source = "import { type Mesh, Vector3, Group } from 'three';\n";
    assert_eq!(
        check(source, MATHS_ONLY),
        expected(&[(1, "three: Mesh"), (1, "three: Group")])
    );

    let dir = project(
        source,
        &format!(r#", "allowNames": {MATHS_ONLY}, "ignoreTypeOnly": true"#),
    );
    let output = run_policy(dir.path(), &[]);
    assert_eq!(
        findings(&parse_json(&output)),
        expected(&[(1, "three: Group")])
    );
}

#[test]
fn absent_or_empty_list_keeps_the_raw_specifier_and_the_note() {
    let source = "import { Vector3, Mesh } from 'three';\n";
    for fields in ["", r#", "allowNames": []"#] {
        let dir = project(source, fields);
        let output = run_policy(dir.path(), &[]);
        assert_eq!(output.code, 0, "stderr: {}", output.stderr);
        let json = parse_json(&output);
        assert_eq!(
            findings(&json),
            expected(&[(1, "three"), (1, "three")]),
            "fields: {fields}"
        );
        assert_eq!(
            json["policy_violations"][0]["actions"][0]["note"],
            "Rule `repro/maths-only` from the configured rule packs bans this import. The check is syntactic: it does not follow aliased or re-bound callees, and import matching uses the raw specifier"
        );
    }
}

#[test]
fn allow_names_finding_explains_the_list_in_its_note() {
    let dir = project("import * as T from 'three';\n", &allow(MATHS_ONLY));
    let json = parse_json(&run_policy(dir.path(), &[]));
    assert_eq!(
        json["policy_violations"][0]["actions"][0]["note"],
        "Rule `repro/maths-only` allows only listed names from this specifier. A namespace import, a star re-export and a side-effect import always report, because fallow cannot prove that they stay in the list. Import a listed name by name."
    );
}

#[test]
fn runtime_loads_stay_out_of_scope_with_a_positive_control() {
    let source = "import fs = require('three');\nconst a = require('three');\nconst b = await import('three');\nimport { Mesh } from 'three';\n";
    assert_eq!(check(source, MATHS_ONLY), expected(&[(4, "three: Mesh")]));
}

#[test]
fn suppression_stays_per_line_and_per_rule() {
    // A comment above a one-line import hides every binding of that line.
    let one_line = "// fallow-ignore-next-line policy-violation:repro/maths-only\nimport { Mesh, Group } from 'three';\n";
    assert_eq!(check(one_line, MATHS_ONLY), expected(&[]));

    // A comment above a binding line of a multi-line import hides that binding only.
    let binding_line = "import {\n  // fallow-ignore-next-line policy-violation:repro/maths-only\n  Mesh,\n  Group,\n} from 'three';\n";
    assert_eq!(
        check(binding_line, MATHS_ONLY),
        expected(&[(4, "three: Group")])
    );

    // A comment above the opening line of a multi-line import hides nothing.
    let opening_line = "// fallow-ignore-next-line policy-violation:repro/maths-only\nimport {\n  Mesh,\n} from 'three';\n";
    assert_eq!(
        check(opening_line, MATHS_ONLY),
        expected(&[(3, "three: Mesh")])
    );
}

const BASELINE: &str = "baseline.json";

fn baseline_arg(root: &Path) -> String {
    root.join(BASELINE).display().to_string()
}

fn save_baseline(root: &Path) {
    let baseline = baseline_arg(root);
    let output = run_policy(root, &["--save-baseline", &baseline]);
    assert!(
        Path::new(&baseline).is_file(),
        "the save wrote no baseline: {}",
        output.stderr
    );
}

fn baseline_keys(root: &Path) -> Value {
    let saved: Value =
        serde_json::from_str(&std::fs::read_to_string(root.join(BASELINE)).expect("read baseline"))
            .expect("baseline is JSON");
    saved["policy_violations"].clone()
}

#[test]
fn a_baselined_name_does_not_hide_a_new_name_in_the_same_file() {
    let dir = project("import { Mesh } from 'three';\n", &allow(MATHS_ONLY));
    let root = dir.path();
    save_baseline(root);
    assert_eq!(
        baseline_keys(root),
        json!(["policy-violation:src/tick.ts:repro:maths-only:three%3A Mesh"])
    );

    std::fs::write(
        root.join("src/tick.ts"),
        "import { Mesh, Group } from 'three';\n",
    )
    .expect("edit source");
    let output = run_policy(root, &["--baseline", &baseline_arg(root)]);
    assert_eq!(
        findings(&parse_json(&output)),
        expected(&[(1, "three: Group")])
    );
}

#[test]
fn adding_the_list_changes_baseline_keys_and_the_growth_gate_reports_them() {
    let dir = project("import { Mesh } from 'three';\n", "");
    let root = dir.path();
    git(root, &["init", "-b", "main"]);
    save_baseline(root);
    assert_eq!(
        baseline_keys(root),
        json!(["policy-violation:src/tick.ts:repro:maths-only:three"])
    );
    commit_all(root, "save baseline");
    git(root, &["checkout", "-b", "feature"]);

    // The old key does not cover the new `matched` value.
    write_pack(root, &allow(MATHS_ONLY));
    let stale = run_policy(root, &["--baseline", &baseline_arg(root)]);
    assert_eq!(
        findings(&parse_json(&stale)),
        expected(&[(1, "three: Mesh")])
    );

    save_baseline(root);
    let output = run_policy(
        root,
        &[
            "--baseline",
            &baseline_arg(root),
            "--fail-on-baseline-growth",
            "--baseline-base",
            "main",
        ],
    );
    assert_eq!(output.code, 1, "stderr: {}", output.stderr);
    assert!(
        output.stderr.contains("maths-only:three%3A Mesh"),
        "stderr: {}",
        output.stderr
    );
}

fn load_error(rule: &str) -> CommandOutput {
    let dir = project("export const a = 1;\n", "");
    std::fs::write(
        dir.path().join("pack.json"),
        format!(r#"{{ "version": 1, "name": "repro", "rules": [{rule}] }}"#),
    )
    .expect("write pack");
    run_policy(dir.path(), &[])
}

#[test]
fn allow_names_on_another_kind_is_a_load_error() {
    for rule in [
        r#"{ "id": "a", "kind": "banned-call", "callees": ["fetch"], "allowNames": ["x"] }"#,
        r#"{ "id": "a", "kind": "banned-effect", "effects": ["network"], "allowNames": ["x"] }"#,
        r#"{ "id": "a", "kind": "banned-export", "exports": ["default"], "allowNames": ["x"] }"#,
        r#"{ "id": "a", "kind": "gdp-proof-producer", "allowedFiles": ["src/**"], "allowNames": ["x"] }"#,
    ] {
        let output = load_error(rule);
        assert_eq!(output.code, 2, "rule: {rule}\nstderr: {}", output.stderr);
        assert!(
            output
                .stdout
                .contains("`allowNames` applies only to banned-import rules"),
            "stdout: {}",
            output.stdout
        );
    }
}

#[test]
fn glob_and_blank_entries_are_load_errors() {
    for (names, message) in [
        (
            r#"["Vector*"]"#,
            "`allowNames` entry `Vector*` contains `*`; entries are exact imported names, not globs",
        ),
        (
            r#"["*"]"#,
            "`allowNames` entry `*` contains `*`; entries are exact imported names, not globs",
        ),
        (r#"[" "]"#, "`allowNames` entries must not be empty"),
    ] {
        let output = load_error(&format!(
            r#"{{ "id": "a", "kind": "banned-import", "specifiers": ["three"], "allowNames": {names} }}"#
        ));
        assert_eq!(output.code, 2, "names: {names}\nstderr: {}", output.stderr);
        assert!(output.stdout.contains(message), "stdout: {}", output.stdout);
    }
}

#[test]
fn guard_states_the_allowed_names() {
    let dir = project("export const a = 1;\n", &allow(MATHS_ONLY));
    let json_output = run_fallow_in_root(
        "guard",
        dir.path(),
        &["src/tick.ts", "--format", "json", "--quiet"],
    );
    assert_eq!(json_output.code, 0, "stderr: {}", json_output.stderr);
    let rule = &parse_json(&json_output)["files"][0]["policy_rules"][0];
    assert_eq!(rule["patterns"], json!(["three"]));
    assert_eq!(rule["allowed_names"], json!(["Vector3", "Quaternion"]));

    let human = run_fallow_in_root("guard", dir.path(), &["src/tick.ts", "--quiet"]);
    assert!(
        human
            .stdout
            .contains("banned-import: three (allowed names: Vector3, Quaternion)"),
        "stdout: {}",
        human.stdout
    );
}

#[test]
fn guard_omits_the_field_without_a_list() {
    let dir = project("export const a = 1;\n", "");
    let output = run_fallow_in_root(
        "guard",
        dir.path(),
        &["src/tick.ts", "--format", "json", "--quiet"],
    );
    let rule = &parse_json(&output)["files"][0]["policy_rules"][0];
    assert_eq!(rule["patterns"], json!(["three"]));
    assert!(rule.get("allowed_names").is_none(), "rule: {rule}");
}

#[test]
fn rule_pack_list_states_the_allowed_names() {
    let dir = project("export const a = 1;\n", &allow(MATHS_ONLY));
    let json_output = run_fallow_in_root("rule-pack", dir.path(), &["list", "--format", "json"]);
    assert_eq!(json_output.code, 0, "stderr: {}", json_output.stderr);
    let rule = &parse_json(&json_output)["packs"][0]["rules"][0];
    assert_eq!(rule["patterns"], json!(["three"]));
    assert_eq!(rule["allowNames"], json!(["Vector3", "Quaternion"]));

    let human = run_fallow_in_root("rule-pack", dir.path(), &["list"]);
    assert!(
        human
            .stdout
            .contains("banned-import: three (allowed names: Vector3, Quaternion)"),
        "stdout: {}",
        human.stdout
    );
}

#[test]
fn rule_pack_list_omits_the_field_without_a_list() {
    let dir = project("export const a = 1;\n", "");
    let output = run_fallow_in_root("rule-pack", dir.path(), &["list", "--format", "json"]);
    let rule = &parse_json(&output)["packs"][0]["rules"][0];
    assert!(rule.get("allowNames").is_none(), "rule: {rule}");
}
