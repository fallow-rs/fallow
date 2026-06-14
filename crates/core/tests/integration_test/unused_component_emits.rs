//! `unused-component-emit`: a Vue `defineEmits` event emitted nowhere in its SFC.
//! Covers the FP-safety regressions: an emit fired through a renamed binding is
//! NOT flagged, an emit fired only in the template via `$emit` is NOT flagged,
//! and a file with a dynamic `emit(<nonLiteral>)` abstains entirely.

use super::common::{create_config, fixture_path};

#[test]
fn flags_unused_emits_but_credits_renamed_template_and_abstains_on_dynamic() {
    let root = fixture_path("unused-component-emit");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    let flagged: Vec<&str> = results
        .unused_component_emits
        .iter()
        .map(|e| e.emit.emit_name.as_str())
        .collect();

    // A declared emit fired nowhere is flagged.
    assert!(
        flagged.contains(&"dead"),
        "an unused emit should be flagged: {flagged:?}"
    );
    // The sibling emit fired in script is NOT flagged.
    assert!(
        !flagged.contains(&"saved"),
        "an emit fired in script must not be flagged: {flagged:?}"
    );
    // An emit fired through a renamed binding is NOT flagged (FP regression).
    assert!(
        !flagged.contains(&"change"),
        "an emit fired through a renamed binding must not be flagged: {flagged:?}"
    );
    // An emit fired only in the template via `$emit` is NOT flagged (FP regression).
    assert!(
        !flagged.contains(&"go"),
        "an emit fired in the template must not be flagged: {flagged:?}"
    );
    // A dynamic `emit(<nonLiteral>)` abstains the whole file (FP regression),
    // so neither sibling is flagged.
    assert!(
        !flagged.contains(&"a") && !flagged.contains(&"b"),
        "a dynamic-emit file must abstain entirely: {flagged:?}"
    );
}
