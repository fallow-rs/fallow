//! Tests for the remove and delete quick fixes that the run withholds: a
//! finding with a reachability caveat, and an export that a test-side file
//! imports in production mode.

use ls_types::Uri;

use super::tests::{make_range, make_unused_export, test_root};
use super::{build_delete_file_actions_for_test, build_remove_export_actions_for_test};
use fallow_api::EditorAnalysisResults as AnalysisResults;
use fallow_api::editor_results::{UnusedExport, UnusedFile, UnusedFileFinding, UnusedTypeFinding};

/// BLOCKER: the quick fix builds its own `WorkspaceEdit` in process, so
/// the CLI-side withholding protected nothing here. The editor is the
/// surface where the user is one keystroke from applying it.
#[test]
fn a_caveated_export_offers_no_remove_quick_fix() {
    use fallow_types::output_dead_code::{CaveatedFinding, ReachabilityCaveat};

    let root = test_root();
    let file = root.join("utils.ts");
    let uri = Uri::from_file_path(&file).unwrap();
    let lines = vec!["export const foo = 1;"];

    let mut clean = AnalysisResults::default();
    clean
        .unused_exports
        .push(make_unused_export(&file, "foo", 1, 13));
    assert_eq!(
        build_remove_export_actions_for_test(&clean, &file, &uri, &make_range(0, 0), &lines).len(),
        1,
        "a run with the evidence still offers the quick fix"
    );

    let mut caveated = AnalysisResults::default();
    let mut finding = make_unused_export(&file, "foo", 1, 13);
    finding.set_reachability_caveats(vec![ReachabilityCaveat::IncompleteImportGraph]);
    caveated.unused_exports.push(finding);

    assert!(
        build_remove_export_actions_for_test(&caveated, &file, &uri, &make_range(0, 0), &lines)
            .is_empty(),
        "a file the run never read may hold the import that credits this export"
    );
}

#[test]
fn a_test_imported_export_offers_no_remove_quick_fix() {
    let root = test_root();
    let file = root.join("utils.ts");
    let uri = Uri::from_file_path(&file).unwrap();
    let lines = vec!["export const foo = 1;"];

    let mut results = AnalysisResults::default();
    let mut finding = make_unused_export(&file, "foo", 1, 13);
    finding.test_importers =
        fallow_types::output_dead_code::TestImporters::from_files(vec![root.join("utils.test.ts")]);
    results.unused_exports.push(finding);

    assert!(
        build_remove_export_actions_for_test(&results, &file, &uri, &make_range(0, 0), &lines)
            .is_empty(),
        "the removal would break the import in the test file"
    );
}

#[test]
fn a_test_imported_type_export_offers_no_remove_quick_fix() {
    let root = test_root();
    let file = root.join("types.ts");
    let uri = Uri::from_file_path(&file).unwrap();
    let lines = vec!["export type Foo = string;"];

    let mut results = AnalysisResults::default();
    let mut finding = UnusedTypeFinding::with_actions(UnusedExport {
        path: file.clone(),
        export_name: "Foo".to_string(),
        is_type_only: true,
        line: 1,
        col: 12,
        span_start: 0,
        is_re_export: false,
        deprecated: false,
        deprecated_reason: None,
    });
    finding.test_importers =
        fallow_types::output_dead_code::TestImporters::from_files(vec![root.join("types.test.ts")]);
    results.unused_types.push(finding);

    assert!(
        build_remove_export_actions_for_test(&results, &file, &uri, &make_range(0, 0), &lines)
            .is_empty(),
        "the removal would break the type import in the test file"
    );
}

/// Type exports go through the same quick-fix builder, so the gate has to
/// reach `unused_types[]` too rather than only the value-export array.
#[test]
fn a_caveated_type_export_offers_no_remove_quick_fix() {
    use fallow_types::output_dead_code::{CaveatedFinding, ReachabilityCaveat};

    let root = test_root();
    let file = root.join("types.ts");
    let uri = Uri::from_file_path(&file).unwrap();
    let lines = vec!["export type Foo = string;"];

    let mut results = AnalysisResults::default();
    let mut finding = UnusedTypeFinding::with_actions(UnusedExport {
        path: file.clone(),
        export_name: "Foo".to_string(),
        is_type_only: true,
        line: 1,
        col: 12,
        span_start: 0,
        is_re_export: false,
        deprecated: false,
        deprecated_reason: None,
    });
    finding.set_reachability_caveats(vec![ReachabilityCaveat::IncompleteImportGraph]);
    results.unused_types.push(finding);

    assert!(
        build_remove_export_actions_for_test(&results, &file, &uri, &make_range(0, 0), &lines)
            .is_empty(),
        "an unused type and an unused export must render with the same confidence"
    );
}

/// The delete action is a `DeleteFile` RESOURCE operation, the most
/// destructive edit the server can hand an editor.
#[test]
fn a_caveated_unused_file_offers_no_delete_quick_fix() {
    use fallow_types::output_dead_code::{CaveatedFinding, ReachabilityCaveat};

    let root = test_root();
    let file = root.join("orphan.ts");
    let uri = Uri::from_file_path(&file).unwrap();

    let mut clean = AnalysisResults::default();
    clean
        .unused_files
        .push(UnusedFileFinding::with_actions(UnusedFile {
            path: file.clone(),
        }));
    assert_eq!(
        build_delete_file_actions_for_test(&clean, &file, &uri, &make_range(0, 0)).len(),
        1,
        "a run with the evidence still offers the delete"
    );

    let mut caveated = AnalysisResults::default();
    let mut finding = UnusedFileFinding::with_actions(UnusedFile { path: file.clone() });
    finding.set_reachability_caveats(vec![ReachabilityCaveat::IncompleteImportGraph]);
    caveated.unused_files.push(finding);

    assert!(
        build_delete_file_actions_for_test(&caveated, &file, &uri, &make_range(0, 0)).is_empty(),
        "an unreachability verdict resting on an unread file must not offer a file deletion"
    );
}
