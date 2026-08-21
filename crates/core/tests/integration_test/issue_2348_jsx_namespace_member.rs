use crate::common::{create_config, fixture_path};

/// Issue #2348: rendering `<SC.UsedStyle />` through a direct namespace import
/// (`import * as SC from './style'`) must credit the `UsedStyle` export in the
/// syntactic scan, while a genuinely-unused export from the same module keeps
/// reporting.
///
/// The fixture pins three consumer shapes:
/// - the entry point itself renders `<SC.UsedStyle />` (the reported repro,
///   previously the `is_entry_with_no_access` false positive),
/// - a non-entry component renders `<S.Wrapper />` (previously conservatively
///   credited every export via mark-all; now narrows, so `UnusedSibling`
///   starts reporting, a deliberate semantics widening),
/// - a non-entry component renders `<UI.Button />` through a barrel that only
///   star-re-exports it, exercising the synthetic star re-export path.
#[test]
fn jsx_namespace_member_render_credits_export() {
    let root = fixture_path("issue-2348-jsx-namespace-member");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_export_names: Vec<&str> = results
        .unused_exports
        .iter()
        .map(|e| e.export.export_name.as_str())
        .collect();

    assert!(
        !unused_export_names.contains(&"UsedStyle"),
        "a namespace export rendered as <SC.UsedStyle /> must be credited: {unused_export_names:?}"
    );
    assert!(
        unused_export_names.contains(&"ActuallyUnusedStyle"),
        "a genuinely-unused export on the same module must still report: {unused_export_names:?}"
    );
    assert!(
        !unused_export_names.contains(&"Wrapper"),
        "a namespace export rendered as <S.Wrapper /> from a non-entry consumer must be credited: {unused_export_names:?}"
    );
    assert!(
        unused_export_names.contains(&"UnusedSibling"),
        "a non-entry JSX consumer must narrow instead of conservatively crediting every export: {unused_export_names:?}"
    );
    assert!(
        !unused_export_names.contains(&"Button"),
        "a namespace member rendered through a star-re-exporting barrel must be credited: {unused_export_names:?}"
    );
    assert!(
        unused_export_names.contains(&"UnusedButtonSibling"),
        "a genuinely-unused export behind the barrel must still report: {unused_export_names:?}"
    );
}
