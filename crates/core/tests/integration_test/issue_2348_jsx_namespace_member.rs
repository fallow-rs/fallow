use crate::common::{create_config, fixture_path};

/// Issue #2348: rendering `<SC.UsedStyle />` through a direct namespace import
/// (`import * as SC from './style'`) must credit the `UsedStyle` export in the
/// syntactic scan, while a genuinely-unused export from the same module keeps
/// reporting.
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
}
