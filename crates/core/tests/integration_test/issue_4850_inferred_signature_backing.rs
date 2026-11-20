//! An exported value without a type annotation gets its type from its
//! initializer. When the initializer holds a typed module binding, the inferred
//! type of the export names the types of that binding. A declaration file of
//! another module that holds the value must import these names, so a live
//! export keeps them in use. See issue #4850.

use super::common::{create_config, fixture_path};

#[test]
fn types_of_a_typed_binding_in_an_exported_initializer_are_not_unused() {
    let root = fixture_path("issue-4850-inferred-signature-backing");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let mut unused_types: Vec<&str> = results
        .unused_types
        .iter()
        .map(|finding| finding.export.export_name.as_str())
        .collect();
    unused_types.sort_unstable();

    // `Box` and `Entry` reach `status` through `entryBox`, and `Item` through
    // `nested` and `items`. `Detached` only types a binding that the
    // initializer reads a property of, and `Unrelated` has no use.
    assert_eq!(unused_types, vec!["Detached", "Unrelated"]);
}

#[test]
fn private_type_of_a_held_binding_is_a_private_type_leak() {
    let root = fixture_path("issue-4850-inferred-signature-backing");
    let mut config = create_config(root);
    config.rules.private_type_leaks = fallow_config::Severity::Warn;
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let leaks: Vec<(&str, &str)> = results
        .private_type_leaks
        .iter()
        .map(|leak| (leak.leak.export_name.as_str(), leak.leak.type_name.as_str()))
        .collect();

    // The declaration of `heldLeak` names `Hidden`, which the module does not
    // export, so declaration emit fails with TS4025.
    assert!(leaks.contains(&("heldLeak", "Hidden")), "found: {leaks:?}");
}
