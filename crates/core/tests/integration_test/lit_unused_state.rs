//! Lit `@state` reactive-property health: a `@state()` on a `LitElement`
//! subclass read NOWHERE via `this.<name>` (template or methods) surfaces as a
//! dead `unused-class-member`. `@property` (the public attribute API) and a
//! template-read `@state` are never flagged. Reuses the existing
//! `find_unused_members` machinery (gated on a Lit dependency).

use super::common::{create_config, fixture_path};

#[test]
fn flags_dead_lit_state_but_credits_used_state_and_property() {
    let root = fixture_path("lit-unused-state");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let flagged: Vec<&str> = results
        .unused_class_members
        .iter()
        .map(|m| m.member.member_name.as_str())
        .collect();

    // A `@state` read nowhere is dead internal reactive state: flagged.
    assert!(
        flagged.contains(&"deadState"),
        "a never-read @state should be flagged: {flagged:?}"
    );
    // A `@state` read in the `html`` template (`${this.usedCount}`) is credited.
    assert!(
        !flagged.contains(&"usedCount"),
        "a template-read @state must not be flagged: {flagged:?}"
    );
    // `@property` is the public attribute API (settable via HTML attribute /
    // parent binding / setAttribute / CSS): never flagged.
    assert!(
        !flagged.contains(&"publicAttr"),
        "a @property (public attribute API) must not be flagged: {flagged:?}"
    );
}
