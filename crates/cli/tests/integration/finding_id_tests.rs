//! Stable `finding_id` values on dead-code findings.
//!
//! An id must survive edits that do not change what the finding is about
//! (line shift, reformat, reorder) and must change when the subject changes
//! (symbol rename, file rename, other issue type). Filters that hide other
//! findings (workspace scope, baseline) must never change the id of a finding
//! that stays in the report.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use serde_json::Value;

use crate::common::{copy_fixture, parse_json, run_fallow_raw};

const BASIC: &str = "finding-ids-basic";
const WORKSPACES: &str = "finding-ids-workspaces";

/// Each dead-code array and the rule token its ids carry.
const ARRAYS: &[(&str, &str)] = &[
    ("unused_files", "unused-file"),
    ("unused_exports", "unused-export"),
    ("unused_types", "unused-type"),
    ("unused_dependencies", "unused-dependency"),
    ("unused_dev_dependencies", "unused-dev-dependency"),
    ("unused_optional_dependencies", "unused-optional-dependency"),
    ("unused_enum_members", "unused-enum-member"),
    ("unused_class_members", "unused-class-member"),
    ("unused_store_members", "unused-store-member"),
    ("unresolved_imports", "unresolved-import"),
    ("unlisted_dependencies", "unlisted-dependency"),
    ("duplicate_exports", "duplicate-export"),
    ("stale_suppressions", "stale-suppression"),
];

/// The fields that name the subject of a finding. The line is not one of them.
const SUBJECT_FIELDS: &[&str] = &[
    "path",
    "export_name",
    "type_name",
    "parent_name",
    "member_name",
    "package_name",
    "specifier",
    "origin",
];

fn root_arg(root: &Path) -> &str {
    root.to_str().expect("temp path is UTF-8")
}

fn dead_code_json(root: &Path, extra: &[&str]) -> Value {
    let mut args = vec![
        "dead-code",
        "--root",
        root_arg(root),
        "--format",
        "json",
        "--quiet",
        "--no-cache",
    ];
    args.extend_from_slice(extra);
    let output = run_fallow_raw(&args);
    assert!(
        output.code == 0 || output.code == 1,
        "dead-code failed with {}: {}",
        output.code,
        output.stderr
    );
    parse_json(&output)
}

/// A readable subject key for one finding: array plus subject fields.
fn subject_key(array: &str, item: &Value) -> String {
    let mut key = array.to_owned();
    for field in SUBJECT_FIELDS {
        if let Some(value) = item.get(*field) {
            key.push('|');
            key.push_str(&value.to_string());
        }
    }
    key
}

fn finding_id(item: &Value) -> String {
    item.get("finding_id")
        .and_then(Value::as_str)
        .unwrap_or_else(|| panic!("finding without finding_id: {item}"))
        .to_owned()
}

/// Subject key to the sorted ids of every finding with that subject.
fn ids_by_subject(json: &Value) -> BTreeMap<String, Vec<String>> {
    let mut map: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (array, _) in ARRAYS {
        for item in json[*array].as_array().into_iter().flatten() {
            map.entry(subject_key(array, item))
                .or_default()
                .push(finding_id(item));
        }
    }
    for ids in map.values_mut() {
        ids.sort();
    }
    map
}

fn all_ids(json: &Value) -> Vec<String> {
    ids_by_subject(json).into_values().flatten().collect()
}

fn ids_in(json: &Value, array: &str) -> Vec<String> {
    json[array]
        .as_array()
        .into_iter()
        .flatten()
        .map(finding_id)
        .collect()
}

fn id_of(json: &Value, array: &str, field: &str, name: &str) -> String {
    let item = json[array]
        .as_array()
        .into_iter()
        .flatten()
        .find(|item| item.get(field).and_then(Value::as_str) == Some(name))
        .unwrap_or_else(|| panic!("no {array} finding with {field} = {name}"));
    finding_id(item)
}

fn write(root: &Path, relative: &str, contents: &str) {
    std::fs::write(root.join(relative), contents).expect("write fixture file");
}

fn read(root: &Path, relative: &str) -> String {
    std::fs::read_to_string(root.join(relative)).expect("read fixture file")
}

fn assert_unique(ids: &[String]) {
    let unique: BTreeSet<&String> = ids.iter().collect();
    assert_eq!(unique.len(), ids.len(), "ids are not unique: {ids:?}");
}

/// The fixture must report every kind the other tests rely on, so a detector
/// change cannot turn the invariance tests into empty comparisons.
fn assert_basic_fixture_shape(json: &Value) {
    for array in [
        "unused_files",
        "unused_exports",
        "unused_types",
        "unused_dependencies",
        "unused_enum_members",
        "unused_class_members",
        "stale_suppressions",
    ] {
        assert!(
            !ids_in(json, array).is_empty(),
            "fixture reports no {array}: {json}"
        );
    }
}

#[test]
fn every_dead_code_finding_has_a_well_formed_unique_id() {
    let dir = copy_fixture(BASIC);
    let json = dead_code_json(dir.path(), &[]);
    assert_basic_fixture_shape(&json);

    let ids = all_ids(&json);
    assert_unique(&ids);
    for (array, token) in ARRAYS {
        for id in ids_in(&json, array) {
            let prefix = format!("dc1:{token}:");
            let rest = id
                .strip_prefix(&prefix)
                .unwrap_or_else(|| panic!("{array} id {id} lacks prefix {prefix}"));
            let (hex, suffix) = rest.split_once('~').unwrap_or((rest, ""));
            assert_eq!(hex.len(), 16, "id {id} has no 16-digit hash");
            assert!(hex.chars().all(|c| c.is_ascii_hexdigit()), "id {id}");
            assert!(
                suffix.chars().all(|c| c.is_ascii_digit()),
                "id {id} has a bad tiebreak suffix"
            );
        }
    }
}

#[test]
fn a_line_shift_keeps_every_id() {
    let dir = copy_fixture(BASIC);
    let before = dead_code_json(dir.path(), &[]);

    for file in [
        "src/utils.ts",
        "src/lib.ts",
        "src/flags.ts",
        "src/orphan.ts",
    ] {
        let source = read(dir.path(), file);
        write(dir.path(), file, &format!("\n\n\n// shifted\n\n{source}"));
    }
    let after = dead_code_json(dir.path(), &[]);

    assert_eq!(ids_by_subject(&before), ids_by_subject(&after));
}

#[test]
fn a_reformat_keeps_every_id() {
    let dir = copy_fixture(BASIC);
    let before = dead_code_json(dir.path(), &[]);

    write(
        dir.path(),
        "src/utils.ts",
        "export const used = 1\nexport const helper = (): number =>\n    1\n\
         export function unusedFn(): number { return 2 }\n\
         export type Shape = {\n    width: number\n}\n\
         export const Dual = 1\nexport type Dual = number\n\
         export enum Status { Active = \"active\", Retired = \"retired\" }\n\
         export class Service {\n    start(): number { return 1 }\n    stop(): number { return 0 }\n\
         \x20   static reset(): void {}\n    reset(): void {}\n}\n",
    );
    let after = dead_code_json(dir.path(), &[]);

    assert_eq!(ids_by_subject(&before), ids_by_subject(&after));
}

#[test]
fn a_reorder_keeps_every_id() {
    let dir = copy_fixture(BASIC);
    let before = dead_code_json(dir.path(), &[]);

    write(
        dir.path(),
        "src/utils.ts",
        "export class Service {\n  stop(): number {\n    return 0;\n  }\n\n  start(): number {\n    return 1;\n  }\n\n\
         \x20 static reset(): void {}\n\n  reset(): void {}\n}\n\n\
         export enum Status {\n  Retired = \"retired\",\n  Active = \"active\",\n}\n\n\
         export type Dual = number;\nexport const Dual = 1;\n\n\
         export type Shape = { width: number };\n\n\
         export function unusedFn(): number {\n  return 2;\n}\n\n\
         export const helper = (): number => 1;\n\nexport const used = 1;\n",
    );
    let after = dead_code_json(dir.path(), &[]);

    assert_eq!(ids_by_subject(&before), ids_by_subject(&after));
}

#[test]
fn a_symbol_rename_gives_a_new_id_and_keeps_the_others() {
    let dir = copy_fixture(BASIC);
    let before = dead_code_json(dir.path(), &[]);
    let old_id = id_of(&before, "unused_exports", "export_name", "helper");

    let source = read(dir.path(), "src/utils.ts");
    write(
        dir.path(),
        "src/utils.ts",
        &source.replace("export const helper", "export const helperRenamed"),
    );
    let after = dead_code_json(dir.path(), &[]);
    let new_id = id_of(&after, "unused_exports", "export_name", "helperRenamed");

    assert_ne!(old_id, new_id);
    assert!(!all_ids(&after).contains(&old_id));
    let mut expected = ids_by_subject(&before);
    expected.retain(|key, _| !key.contains("\"helper\""));
    let mut actual = ids_by_subject(&after);
    actual.retain(|key, _| !key.contains("\"helperRenamed\""));
    assert_eq!(expected, actual);
}

#[test]
fn a_file_rename_gives_a_new_id_and_keeps_the_others() {
    let dir = copy_fixture(BASIC);
    let before = dead_code_json(dir.path(), &[]);
    let old_id = id_of(&before, "unused_files", "path", "src/orphan.ts");

    std::fs::rename(
        dir.path().join("src/orphan.ts"),
        dir.path().join("src/stray.ts"),
    )
    .expect("rename file");
    let after = dead_code_json(dir.path(), &[]);
    let new_id = id_of(&after, "unused_files", "path", "src/stray.ts");

    assert_ne!(old_id, new_id);
    let mut expected = ids_by_subject(&before);
    expected.retain(|key, _| !key.contains("src/orphan.ts"));
    let mut actual = ids_by_subject(&after);
    actual.retain(|key, _| !key.contains("src/stray.ts"));
    assert_eq!(expected, actual);
}

#[test]
fn the_same_name_in_different_issue_types_gets_different_ids() {
    let dir = copy_fixture(BASIC);
    let json = dead_code_json(dir.path(), &[]);

    let value = id_of(&json, "unused_exports", "export_name", "Dual");
    let type_alias = id_of(&json, "unused_types", "export_name", "Dual");

    assert!(value.starts_with("dc1:unused-export:"), "{value}");
    assert!(type_alias.starts_with("dc1:unused-type:"), "{type_alias}");
    assert_ne!(
        value.rsplit(':').next(),
        type_alias.rsplit(':').next(),
        "the rule token must be part of the hash input"
    );
}

#[test]
fn a_re_export_and_its_source_get_different_ids() {
    let dir = copy_fixture(BASIC);
    let json = dead_code_json(dir.path(), &[]);

    let reexported: Vec<(String, String)> = json["unused_exports"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|item| item["export_name"] == "reexported")
        .map(|item| {
            (
                item["path"].as_str().unwrap_or_default().to_owned(),
                finding_id(item),
            )
        })
        .collect();

    assert!(
        !reexported.is_empty(),
        "fixture reports no unused `reexported`: {json}"
    );
    let ids: Vec<String> = reexported.iter().map(|(_, id)| id.clone()).collect();
    assert_unique(&ids);
}

#[test]
fn duplicate_subjects_get_a_tiebreak_suffix() {
    let dir = copy_fixture(BASIC);
    let json = dead_code_json(dir.path(), &[]);
    let by_subject = ids_by_subject(&json);

    let groups: Vec<&Vec<String>> = by_subject.values().filter(|ids| ids.len() > 1).collect();
    assert!(
        groups.len() >= 2,
        "fixture must have duplicate class members and duplicate suppressions: {by_subject:?}"
    );
    for ids in groups {
        let base = ids
            .iter()
            .find(|id| !id.contains('~'))
            .unwrap_or_else(|| panic!("group without a base id: {ids:?}"));
        for (k, id) in ids.iter().filter(|id| id.contains('~')).enumerate() {
            assert_eq!(id, &format!("{base}~{}", k + 1));
        }
    }
}

#[test]
fn ids_do_not_depend_on_the_thread_count() {
    let dir = copy_fixture(BASIC);
    let single = dead_code_json(dir.path(), &["--threads", "1"]);
    let many = dead_code_json(dir.path(), &["--threads", "8"]);

    assert_eq!(ids_by_subject(&single), ids_by_subject(&many));
}

#[test]
fn workspaces_with_the_same_names_get_different_ids() {
    let dir = copy_fixture(WORKSPACES);
    let json = dead_code_json(dir.path(), &[]);

    let deps = ids_in(&json, "unused_dependencies");
    let exports = ids_in(&json, "unused_exports");
    assert_eq!(deps.len(), 2, "{json}");
    assert_eq!(exports.len(), 2, "{json}");
    assert_unique(&all_ids(&json));
}

#[test]
fn a_workspace_scope_keeps_the_id_of_every_kept_finding() {
    let dir = copy_fixture(WORKSPACES);
    let full = ids_by_subject(&dead_code_json(dir.path(), &[]));
    let scoped = ids_by_subject(&dead_code_json(dir.path(), &["--workspace", "@ids/a"]));

    assert!(!scoped.is_empty());
    assert!(scoped.len() < full.len(), "scope removed nothing");
    for (subject, ids) in &scoped {
        assert_eq!(full.get(subject), Some(ids), "{subject}");
    }
}

#[test]
fn a_baseline_keeps_the_id_of_every_kept_finding() {
    let dir = copy_fixture(BASIC);
    let baseline = dir.path().join("fallow-baseline.json");
    let baseline_arg = baseline.to_str().expect("temp path is UTF-8");
    let save = run_fallow_raw(&[
        "dead-code",
        "--root",
        root_arg(dir.path()),
        "--quiet",
        "--no-cache",
        "--save-baseline",
        baseline_arg,
    ]);
    assert!(baseline.exists(), "no baseline written: {}", save.stderr);

    let source = read(dir.path(), "src/utils.ts");
    write(
        dir.path(),
        "src/utils.ts",
        &format!("{source}\nexport const fresh = 3;\n"),
    );
    let full = dead_code_json(dir.path(), &[]);
    let filtered = dead_code_json(dir.path(), &["--baseline", baseline_arg]);

    assert_eq!(
        ids_in(&filtered, "unused_exports"),
        vec![id_of(&full, "unused_exports", "export_name", "fresh")]
    );
}

#[test]
fn ids_do_not_depend_on_the_checkout_location() {
    let first = copy_fixture(BASIC);
    let second = copy_fixture(BASIC);

    assert_eq!(
        ids_by_subject(&dead_code_json(first.path(), &[])),
        ids_by_subject(&dead_code_json(second.path(), &[]))
    );
}

/// Every finding in `value` that carries an id, as a sorted readable line:
/// `<array> <path> <name> <line> <id>`. The walk follows nested envelopes
/// (`groups[]`, `check`, `dead_code`), so one helper reads every surface.
fn id_rows(value: &Value) -> Vec<String> {
    fn walk(value: &Value, array: &str, rows: &mut Vec<String>) {
        match value {
            Value::Object(map) => {
                if let Some(id) = map.get("finding_id").and_then(Value::as_str) {
                    let name = ["export_name", "member_name", "package_name", "specifier"]
                        .iter()
                        .find_map(|field| map.get(*field).and_then(Value::as_str))
                        .unwrap_or("-");
                    let path = map.get("path").and_then(Value::as_str).unwrap_or("-");
                    let line = map
                        .get("line")
                        .map_or_else(|| "-".to_owned(), Value::to_string);
                    rows.push(format!("{array} {path} {name} {line} {id}"));
                }
                for (key, child) in map {
                    walk(child, key, rows);
                }
            }
            Value::Array(items) => {
                for item in items {
                    walk(item, array, rows);
                }
            }
            _ => {}
        }
    }
    let mut rows = Vec::new();
    walk(value, "-", &mut rows);
    rows.sort();
    rows
}

fn fallow_json(root: &Path, args: &[&str]) -> Value {
    let mut full = args.to_vec();
    full.extend_from_slice(&[
        "--root",
        root_arg(root),
        "--format",
        "json",
        "--quiet",
        "--no-cache",
    ]);
    let output = run_fallow_raw(&full);
    assert!(
        output.code == 0 || output.code == 1,
        "fallow {args:?} failed with {}: {}",
        output.code,
        output.stderr
    );
    parse_json(&output)
}

/// Pins real ids end to end: the fixture runs through the binary, so a move
/// of the stamping call site, a change of the hash input or a lost field on
/// one envelope fails here. Only ids and their subjects are compared, so no
/// volatile field (elapsed time, version) takes part. `--format compact`
/// carries no ids, so it is not in the list.
#[test]
fn every_envelope_carries_the_pinned_ids() {
    let dir = copy_fixture(BASIC);
    let dead_code = id_rows(&fallow_json(dir.path(), &["dead-code"]));
    insta::assert_snapshot!("finding_ids_basic_dead_code", dead_code.join("\n"));

    for (label, args) in [
        ("check", vec!["check"]),
        ("grouped", vec!["dead-code", "--group-by", "directory"]),
        ("combined", vec![]),
    ] {
        assert_eq!(
            id_rows(&fallow_json(dir.path(), &args)),
            dead_code,
            "{label} envelope carries other ids than dead-code"
        );
    }
}

/// `fallow audit` reports the dead-code findings of changed files. Each one
/// must carry the id that a full `dead-code` run gives the same finding.
#[test]
fn audit_carries_the_dead_code_ids() {
    let dir = copy_fixture(BASIC);
    crate::common::git(dir.path(), &["init", "-q", "-b", "main"]);
    crate::common::commit_all(dir.path(), "base");
    let source = read(dir.path(), "src/utils.ts");
    write(
        dir.path(),
        "src/utils.ts",
        &format!("{source}\nexport const fresh = 3;\n"),
    );

    let full = id_rows(&fallow_json(dir.path(), &["dead-code"]));
    let audit = fallow_json(dir.path(), &["audit", "--base", "main"]);
    let rows = id_rows(&audit["dead_code"]);

    assert!(
        rows.iter().any(|row| row.contains(" fresh ")),
        "audit misses the new export: {rows:?}"
    );
    for row in &rows {
        assert!(
            full.contains(row),
            "audit row {row} is not in the dead-code run"
        );
    }
}

/// Type-aware refinement runs after the scope filters and must keep the id
/// of every finding that the syntactic run also reports.
#[test]
fn type_aware_analysis_keeps_the_ids() {
    let dir = copy_fixture(BASIC);
    write(
        dir.path(),
        "tsconfig.json",
        r#"{"compilerOptions":{"strict":true,"module":"esnext","moduleResolution":"bundler","target":"es2022"},"include":["src"]}"#,
    );
    let syntactic = id_rows(&fallow_json(dir.path(), &["dead-code"]));
    assert!(
        syntactic.len() >= 10,
        "the syntactic run must report the fixture findings with ids: {syntactic:?}"
    );

    let output = crate::common::run_fallow_raw_with_type_aware_sidecar(&[
        "dead-code",
        "--type-aware",
        "--root",
        root_arg(dir.path()),
        "--format",
        "json",
        "--quiet",
        "--no-cache",
    ]);
    assert!(
        output.code == 0 || output.code == 1,
        "type-aware run failed with {}: {}",
        output.code,
        output.stderr
    );
    let json = parse_json(&output);
    assert_eq!(
        json["_meta"]["type_aware"]["identity"]["mode"], "type-aware",
        "the type-aware path did not run: {}",
        json["_meta"]
    );
    let type_aware = id_rows(&json);

    let ids: Vec<String> = type_aware
        .iter()
        .map(|row| row.rsplit(' ').next().unwrap_or_default().to_owned())
        .collect();
    assert_unique(&ids);
    for row in &syntactic {
        assert!(
            type_aware.contains(row),
            "type-aware run lost or changed {row}: {type_aware:?}"
        );
    }
}
