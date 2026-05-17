use super::common::{create_config, fixture_path};

#[test]
fn super_method_calls_credit_parent_class_members() {
    // Base class methods only called via `super.method()` in subclasses must
    // not be reported as unused. See issue #130.
    let root = fixture_path("super-method-calls");
    let mut config = create_config(root);
    config.rules.unused_class_members = fallow_config::Severity::Error;
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused: Vec<String> = results
        .unused_class_members
        .iter()
        .map(|m| format!("{}.{}", m.member.parent_name, m.member.member_name))
        .collect();

    assert!(
        !unused.contains(&"Animal.speak".to_string()),
        "Animal.speak is used via super.speak() in Dog and Cat: {unused:?}"
    );
    assert!(
        !unused.contains(&"Animal.greet".to_string()),
        "Animal.greet is used via dog.greet() in main: {unused:?}"
    );

    // Genuinely unused parent method must still be flagged: guards against
    // an over-eager fix that credits every parent member.
    assert!(
        unused.contains(&"Animal.unusedOnParent".to_string()),
        "Animal.unusedOnParent has no callers and should remain unused: {unused:?}"
    );
}
