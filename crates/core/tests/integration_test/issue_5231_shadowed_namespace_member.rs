use crate::common::{create_config, fixture_path};

/// Issue #5231: a catch parameter that shadows a namespace import is a local,
/// so `e.message` in the catch block is not a read of the `message` export.
/// Only the dotted read `e.usedError()` uses the import, so `message` stays
/// unused.
#[test]
fn shadowing_catch_parameter_member_read_does_not_credit_the_export() {
    let root = fixture_path("issue-5231-shadowed-namespace-member");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused: Vec<&str> = results
        .unused_exports
        .iter()
        .map(|e| e.export.export_name.as_str())
        .collect();

    assert_eq!(
        unused,
        ["message"],
        "a member read of the catch parameter must not credit the namespace export"
    );
}
