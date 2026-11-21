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
    assert_eq!(after, "export enum E {\n  B,\n  C\n}\n");
}

#[test]
fn enum_fix_keeps_a_comma_in_a_string_value() {
    let (fixes, after) = fix("export enum E { A = \"x,y\", B }\n", "B", 1);
    assert_eq!(fixes.len(), 1);
    assert_eq!(after, "export enum E { A = \"x,y\" }\n");
}

#[test]
fn enum_fix_removes_an_initializer_over_several_lines() {
    let (fixes, after) = fix("export enum E {\n  A = 1 +\n    2,\n  B,\n}\n", "A", 2);
    assert_eq!(fixes.len(), 1);
    assert_eq!(after, "export enum E {\n  B,\n}\n");
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
