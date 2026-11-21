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

/// The remove-export actions for `findings` (name, line, col) in a file
/// with `lines`, with the cursor on each line of the file.
fn actions_for(
    file_name: &str,
    lines: &[&str],
    findings: &[(&str, u32, u32)],
) -> (Uri, Vec<ls_types::CodeActionOrCommand>) {
    let root = test_root();
    let file = root.join(file_name);
    let uri = Uri::from_file_path(&file).unwrap();
    let mut results = AnalysisResults::default();
    for (name, line, col) in findings {
        results
            .unused_exports
            .push(make_unused_export(&file, name, *line, *col));
    }
    let last = u32::try_from(lines.len().saturating_sub(1)).unwrap();
    let actions =
        build_remove_export_actions_for_test(&results, &file, &uri, &make_range(0, last), lines);
    (uri, actions)
}

fn edit_ranges(uri: &Uri, actions: &[ls_types::CodeActionOrCommand]) -> Vec<(u32, u32, u32)> {
    actions
        .iter()
        .map(|action| {
            let edits = unwrap_code_action(action)
                .edit
                .as_ref()
                .unwrap()
                .changes
                .as_ref()
                .unwrap()
                .get(uri)
                .unwrap();
            let range = edits[0].range;
            assert_eq!(range.start.line, range.end.line);
            (range.start.line, range.start.character, range.end.character)
        })
        .collect()
}

#[test]
fn no_action_for_an_anonymous_default_export() {
    for line in [
        "export default function () {}",
        "export default { a: 1, b: 2 };",
        "export default class {}",
    ] {
        let (_, actions) = actions_for("default.ts", &[line], &[("default", 1, 0)]);
        assert!(actions.is_empty(), "{line}");
    }
}

#[test]
fn no_action_when_a_sibling_declarator_is_in_use() {
    let (_, actions) = actions_for(
        "shared.ts",
        &["export const dead = 1, live = 2;"],
        &[("dead", 1, 13)],
    );
    assert!(actions.is_empty());
}

#[test]
fn one_action_for_each_name_when_every_declarator_is_unused() {
    let (uri, actions) = actions_for(
        "shared.ts",
        &["export const dead = 1, live = 2;"],
        &[("dead", 1, 13), ("live", 1, 23)],
    );
    assert_eq!(edit_ranges(&uri, &actions), vec![(0, 0, 7), (0, 0, 7)]);
}

#[test]
fn edit_targets_the_second_export_on_one_line() {
    let (uri, actions) = actions_for(
        "two.ts",
        &["export function live() {} export function dead() {}"],
        &[("dead", 1, 42)],
    );
    assert_eq!(edit_ranges(&uri, &actions), vec![(0, 26, 33)]);
}

#[test]
fn no_action_in_a_document_that_does_not_parse() {
    let (_, actions) = actions_for("broken.ts", &["export const a = ;"], &[("a", 1, 13)]);
    assert!(actions.is_empty());
}
