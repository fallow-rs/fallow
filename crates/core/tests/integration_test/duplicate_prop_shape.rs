//! `duplicate-prop-shape`: three or more React/Preact components across two or
//! more files whose statically-harvested prop NAME set is byte-identical after
//! stripping ubiquitous DOM / passthrough names, with four or more significant
//! names remaining. A missing shared `Props` type. The rule defaults to `off`
//! (dormant), so every positive test enables it. Asserts the genuine group is
//! flagged (one finding per member, with the right shape + sibling roster) and
//! that each noise case (superset, DOM-only-shared wrappers, a two-member pair)
//! is excluded (anti-noise gates, zero-FP doctrine).

use super::common::{create_config, fixture_path};

/// The genuine `{ error, helpText, label, value }` group (FieldText +
/// FieldNumber in fields.tsx, FieldTextarea in form.tsx) is flagged as three
/// findings, one per member, each carrying the shared shape and the other two
/// members in `sharing_components`. The superset (FieldSelectWithOptions), the
/// DOM-only-shared wrappers (Box/Stack), and the two-member card pair
/// (CardA/CardB) are all excluded.
#[test]
fn detects_genuine_group_and_excludes_noise() {
    let root = fixture_path("duplicate-prop-shape");
    let mut config = create_config(root);
    config.rules.duplicate_prop_shape = fallow_config::Severity::Warn;
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let shapes = &results.duplicate_prop_shapes;
    let names: Vec<&str> = shapes.iter().map(|d| d.shape.component.as_str()).collect();

    // Exactly the three genuine members, nothing else.
    assert_eq!(
        shapes.len(),
        3,
        "exactly three duplicate-prop-shape members expected, got: {names:?}"
    );
    for member in ["FieldText", "FieldNumber", "FieldTextarea"] {
        assert!(
            names.contains(&member),
            "{member} should be a flagged member, got: {names:?}"
        );
    }
    // Noise cases must NOT appear.
    for excluded in [
        "FieldSelectWithOptions", // superset: not byte-identical
        "Box",                    // empty significant set after denylist
        "Stack",                  // empty significant set after denylist
        "CardA",                  // two-member pair: below the group floor
        "CardB",                  // two-member pair: below the group floor
    ] {
        assert!(
            !names.contains(&excluded),
            "{excluded} must NOT be flagged, got: {names:?}"
        );
    }

    // Every member carries the same shared significant shape, sorted.
    let expected_shape = vec![
        "error".to_string(),
        "helpText".to_string(),
        "label".to_string(),
        "value".to_string(),
    ];
    for d in shapes {
        assert_eq!(
            d.shape.shape, expected_shape,
            "shared shape mismatch for {}",
            d.shape.component
        );
        assert_eq!(
            d.shape.group_size, 3,
            "group size is 3 for {}",
            d.shape.component
        );
        assert_eq!(
            d.shape.sharing_components.len(),
            2,
            "each member lists the other two siblings for {}",
            d.shape.component
        );
        assert!(d.shape.line >= 1, "1-based line for {}", d.shape.component);
    }

    // The group spans two distinct files (fields.tsx + form.tsx).
    let stems: std::collections::BTreeSet<String> = shapes
        .iter()
        .filter_map(|d| {
            d.shape
                .file
                .file_stem()
                .and_then(|s| s.to_str())
                .map(str::to_string)
        })
        .collect();
    assert!(
        stems.contains("fields") && stems.contains("form"),
        "the group must span fields.tsx and form.tsx, got: {stems:?}"
    );
}

/// Dormant by default: with the rule at its `off` default, the fixture emits NO
/// findings even though a genuine group exists.
#[test]
fn dormant_when_rule_off() {
    let root = fixture_path("duplicate-prop-shape");
    let config = create_config(root); // rule defaults to off
    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    assert!(
        results.duplicate_prop_shapes.is_empty(),
        "the duplicate-prop-shape rule is off by default: {}",
        results.duplicate_prop_shapes.len()
    );
}

/// Dep gate: a non-React project never emits duplicate-prop-shape findings even
/// with the rule enabled.
#[test]
fn dep_gated_to_react() {
    // The Vue fixture declares only `vue` (no react/react-dom/next/preact).
    let root = fixture_path("unused-component-prop");
    let mut config = create_config(root);
    config.rules.duplicate_prop_shape = fallow_config::Severity::Warn;
    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    assert!(
        results.duplicate_prop_shapes.is_empty(),
        "duplicate-prop-shape must not fire on a non-React project"
    );
}
