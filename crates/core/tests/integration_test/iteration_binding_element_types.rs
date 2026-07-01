use super::common::{create_config, fixture_path};

fn unused_members(fixture: &str) -> Vec<String> {
    let root = fixture_path(fixture);
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    results
        .unused_class_members
        .iter()
        .map(|member| {
            format!(
                "{}.{}",
                member.member.parent_name, member.member.member_name
            )
        })
        .collect()
}

/// Family 2 (#1707 follow-up): a JS iteration variable typed to the element class
/// of a typed array credits member accesses. `utils.map(u => u.getter)`,
/// `utils.forEach(u => u.hello())`, and `for (const u of utils) u.property`
/// credit `getter` / `hello` / `property`; `deadMethod` (never accessed) stays
/// flagged, proving the detector still fires.
#[test]
fn js_iteration_bindings_credit_class_member_accesses() {
    let unused = unused_members("iteration-binding-js");

    for member in ["getter", "hello", "property"] {
        assert!(
            !unused.contains(&format!("Util.{member}")),
            "Util.{member} is accessed via a .map/.forEach/for-of iteration variable and must be credited, found: {unused:?}"
        );
    }
    assert!(
        unused.contains(&"Util.deadMethod".to_string()),
        "Util.deadMethod is never accessed and must still report, found: {unused:?}"
    );
}

/// Family 1 (#1707 follow-up): a Svelte `{#each utils as util}` item typed to the
/// element class credits member accesses on the item.
#[test]
fn svelte_each_item_credits_class_member_accesses() {
    let unused = unused_members("iteration-binding-svelte");

    for member in ["getter", "hello", "property"] {
        assert!(
            !unused.contains(&format!("Util.{member}")),
            "Util.{member} is accessed via the Svelte {{#each}} item and must be credited, found: {unused:?}"
        );
    }
    assert!(
        unused.contains(&"Util.deadMethod".to_string()),
        "Util.deadMethod is never accessed and must still report, found: {unused:?}"
    );
}
