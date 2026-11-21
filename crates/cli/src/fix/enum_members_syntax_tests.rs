use super::tests::fix_single_member;

/// The fix records and the new text of a file `e.ts` with `source` after
/// the removal of the member `name` of the enum `E` on `line`.
fn fix(source: &str, name: &str, line: u32) -> (Vec<serde_json::Value>, String) {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("e.ts");
    std::fs::write(&file, source).unwrap();
    let fixes = fix_single_member(dir.path(), &file, "E", name, line, false);
    (fixes, std::fs::read_to_string(&file).unwrap())
}

#[test]
fn enum_fix_keeps_a_packed_sibling() {
    let (fixes, after) = fix("export enum E {\n  A, B,\n  C\n}\n", "A", 2);
    assert_eq!(fixes.len(), 1);
    assert_eq!(after, "export enum E {\n  B = 1,\n  C\n}\n");
}

#[test]
fn enum_fix_keeps_a_comma_in_a_string_value() {
    let (fixes, after) = fix("export enum E { A = \"x,y\", B }\n", "B", 1);
    assert_eq!(fixes.len(), 1);
    assert_eq!(after, "export enum E { A = \"x,y\" }\n");
}

#[test]
fn enum_fix_removes_an_initializer_over_several_lines() {
    let (fixes, after) = fix("export enum E {\n  A = 1 +\n    2,\n  B = 4,\n}\n", "A", 2);
    assert_eq!(fixes.len(), 1);
    assert_eq!(after, "export enum E {\n  B = 4,\n}\n");
}

#[test]
fn enum_fix_keeps_an_enum_whose_kept_value_is_unknown() {
    let source = "export enum E {\n  A = 1 +\n    2,\n  B,\n}\n";
    let (fixes, after) = fix(source, "A", 2);
    assert!(fixes.is_empty(), "{fixes:?}");
    assert_eq!(after, source, "B counts on from a computed value");
}

#[test]
fn enum_fix_keeps_a_comment_line_inside_the_enum() {
    let (fixes, after) = fix(
        "export enum E {\n  // the first member\n  A,\n  B,\n}\n",
        "B",
        4,
    );
    assert_eq!(fixes.len(), 1);
    assert_eq!(after, "export enum E {\n  // the first member\n  A,\n}\n");
}

#[test]
fn enum_fix_skips_a_file_that_does_not_parse() {
    let source = "export enum E {\n  A,\n  B,\n}\nconst = ;\n";
    let (fixes, after) = fix(source, "B", 3);
    assert!(fixes.is_empty(), "{fixes:?}");
    assert_eq!(after, source);
}

#[test]
fn enum_fix_reports_only_the_members_it_removes_from_two_enums_with_one_name() {
    use super::tests::{capture_hashes, make_enum_member};
    use super::{EnumMemberFixInput, apply_enum_member_fixes};
    use crate::fix::plan::FixPlan;
    use fallow_config::OutputFormat;
    use fallow_types::results::UnusedMember;
    use rustc_hash::FxHashMap;
    use std::path::PathBuf;

    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("e.ts");
    let source = "namespace N {\n  enum E {\n    A,\n    B = 2,\n  }\n}\nnamespace M {\n  enum E {\n    A = 1,\n    C,\n  }\n}\n";
    std::fs::write(&file, source).unwrap();
    let in_n = make_enum_member(&file, "E", "A", 3);
    let in_m = make_enum_member(&file, "E", "C", 10);
    let mut map: FxHashMap<PathBuf, Vec<&UnusedMember>> = FxHashMap::default();
    map.insert(file.clone(), vec![&in_n, &in_m]);
    let mut fixes = Vec::new();
    let mut plan = FixPlan::for_root(dir.path()).unwrap();
    apply_enum_member_fixes(EnumMemberFixInput {
        root: dir.path(),
        members_by_file: &map,
        hashes: &capture_hashes(&[&file]),
        plan: &mut plan,
        output: OutputFormat::Json,
        dry_run: false,
        fixes: &mut fixes,
    });
    assert!(plan.commit().failed.is_empty());
    let mut lines: Vec<u64> = fixes
        .iter()
        .map(|fix| fix["line"].as_u64().unwrap())
        .collect();
    lines.sort_unstable();
    assert_eq!(lines, vec![3, 10]);
    assert_eq!(
        std::fs::read_to_string(&file).unwrap(),
        "namespace N {\n  enum E {\n    B = 2,\n  }\n}\nnamespace M {\n  enum E {\n    A = 1,\n  }\n}\n"
    );
}

#[test]
fn enum_fix_does_not_fold_an_exported_enum_whose_name_has_another_enum() {
    use super::tests::{capture_hashes, make_enum_member};
    use super::{EnumMemberFixInput, apply_enum_member_fixes};
    use crate::fix::plan::FixPlan;
    use fallow_config::OutputFormat;
    use fallow_types::results::UnusedMember;
    use rustc_hash::FxHashMap;
    use std::path::PathBuf;

    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("e.ts");
    let source = "export enum E {\n  A = 1,\n  B = 2,\n}\nnamespace M {\n  enum E {\n    A = 1,\n    B = 2,\n  }\n}\n";
    std::fs::write(&file, source).unwrap();
    let members = [
        make_enum_member(&file, "E", "A", 2),
        make_enum_member(&file, "E", "A", 7),
        make_enum_member(&file, "E", "B", 8),
    ];
    let mut map: FxHashMap<PathBuf, Vec<&UnusedMember>> = FxHashMap::default();
    map.insert(file.clone(), members.iter().collect());
    let mut fixes = Vec::new();
    let mut plan = FixPlan::for_root(dir.path()).unwrap();
    apply_enum_member_fixes(EnumMemberFixInput {
        root: dir.path(),
        members_by_file: &map,
        hashes: &capture_hashes(&[&file]),
        plan: &mut plan,
        output: OutputFormat::Json,
        dry_run: false,
        fixes: &mut fixes,
    });
    assert!(plan.commit().failed.is_empty());
    assert!(
        fixes.iter().all(|fix| fix["type"] == "remove_enum_member"),
        "no fold: {fixes:?}"
    );
    let after = std::fs::read_to_string(&file).unwrap();
    assert!(
        after.starts_with("export enum E {\n  B = 2,\n}\n"),
        "{after}"
    );
}
