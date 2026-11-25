use crate::common::{create_config, fixture_path};

/// Issue #5064: a catch parameter that shadows a namespace import is a local,
/// so `String(e)` in the catch block is not a use of the whole namespace.
/// Only the dotted read `e.usedError()` uses the import, so `unusedError`
/// stays unused.
#[test]
fn shadowing_catch_parameter_does_not_credit_every_export() {
    let root = fixture_path("issue-5064-shadowed-namespace-import");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused: Vec<&str> = results
        .unused_exports
        .iter()
        .map(|e| e.export.export_name.as_str())
        .collect();

    assert_eq!(
        unused,
        ["unusedError"],
        "a use of the catch parameter must not credit the namespace import"
    );
}
