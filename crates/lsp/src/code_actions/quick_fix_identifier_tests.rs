//! Tests for the name check of the remove-export quick fix.

use ls_types::Uri;

use super::build_remove_export_actions_for_test;
use super::tests::{
    apply_single_line_edit, make_range, make_unused_export, test_root, unwrap_code_action,
};
use fallow_api::EditorAnalysisResults as AnalysisResults;

#[test]
fn generates_action_for_export_name_with_combining_mark() {
    let root = test_root();
    let file = root.join("combining.ts");
    let uri = Uri::from_file_path(&file).unwrap();

    // U+0301 is a combining mark (category Mn). JavaScript allows it in
    // an identifier after the first character, but it is not alphanumeric.
    let name = "cafe\u{301}";
    let mut results = AnalysisResults::default();
    results
        .unused_exports
        .push(make_unused_export(&file, name, 1, 13));

    let lines = vec!["export const cafe\u{301} = 1;"];
    let actions =
        build_remove_export_actions_for_test(&results, &file, &uri, &make_range(0, 0), &lines);

    assert_eq!(actions.len(), 1);
    let ca = unwrap_code_action(&actions[0]);
    let changes = ca.edit.as_ref().unwrap().changes.as_ref().unwrap();
    let edits = changes.get(&uri).unwrap();
    assert_eq!(
        apply_single_line_edit(lines[0], &edits[0]),
        "const cafe\u{301} = 1;"
    );
}

#[test]
fn no_action_when_export_name_is_prefix_of_combining_mark_identifier() {
    let root = test_root();
    let file = root.join("combining-prefix.ts");
    let uri = Uri::from_file_path(&file).unwrap();

    let mut results = AnalysisResults::default();
    results
        .unused_exports
        .push(make_unused_export(&file, "cafe", 1, 13));

    let lines = vec!["export const cafe\u{301} = 1;"];
    let actions =
        build_remove_export_actions_for_test(&results, &file, &uri, &make_range(0, 0), &lines);
    assert!(actions.is_empty());
}
