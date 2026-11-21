use std::path::Path;

use super::tests::fix_single;

/// Run the apply fix for the export `name` on `line` of a file with
/// `source`, and return the fix records and the new file text.
fn fix_file(source: &str, name: &str, line: u32) -> (Vec<serde_json::Value>, String) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let file = root.join("store.ts");
    std::fs::write(&file, source).unwrap();
    let (had_error, fixes) = fix_single(root, &file, name, line, false);
    assert!(!had_error);
    (fixes, read(&file))
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap()
}

fn assert_unchanged(source: &str, name: &str, line: u32) {
    let (fixes, after) = fix_file(source, name, line);
    assert!(fixes.is_empty(), "{fixes:?}");
    assert_eq!(after, source);
}

#[test]
fn export_fix_skips_an_anonymous_default_function() {
    assert_unchanged("export default function () {}\n", "default", 1);
}

#[test]
fn export_fix_skips_an_anonymous_default_class() {
    assert_unchanged("export default class {}\n", "default", 1);
}

#[test]
fn export_fix_skips_a_default_object() {
    assert_unchanged("export default { a: 1, b: 2 };\n", "default", 1);
}

#[test]
fn export_fix_removes_an_enum_with_a_brace_in_a_comment() {
    let (fixes, after) = fix_file(
        "export enum E {\n  A, // }\n  B,\n}\nexport const kept = 1;\n",
        "E",
        1,
    );
    assert_eq!(fixes.len(), 1);
    assert_eq!(after, "export const kept = 1;\n");
}

#[test]
fn export_fix_removes_an_enum_with_a_brace_in_a_string() {
    let (fixes, after) = fix_file("export enum E {\n  A = \"}\",\n  B,\n}\n", "E", 1);
    assert_eq!(fixes.len(), 1);
    assert_eq!(after, "\n");
}

#[test]
fn export_fix_skips_a_keyword_edit_in_a_file_that_does_not_parse() {
    assert_unchanged("export const a = 1;\nconst = ;\n", "a", 1);
}

#[test]
fn export_fix_still_rewrites_a_list_in_a_file_that_does_not_parse() {
    let (fixes, after) = fix_file(
        "const x = 1;\nconst y = 2;\nexport { x, y };\nconst = ;\n",
        "y",
        3,
    );
    assert_eq!(fixes.len(), 1, "{fixes:?}");
    assert_eq!(
        after,
        "const x = 1;\nconst y = 2;\nexport { x };\nconst = ;\n"
    );
}

#[test]
fn export_fix_skips_a_namespace_re_export() {
    assert_unchanged("export * as ns from './x';\n", "ns", 1);
}
