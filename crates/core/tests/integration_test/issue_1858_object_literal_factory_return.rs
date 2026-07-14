use super::common::{create_config, fixture_path};

fn unused_member_names(root: std::path::PathBuf) -> Vec<String> {
    let mut config = create_config(root);
    config.rules.unused_class_members = fallow_config::Severity::Error;
    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    results
        .unused_class_members
        .iter()
        .map(|m| format!("{}.{}", m.member.parent_name, m.member.member_name))
        .collect()
}

#[test]
fn object_literal_factory_return_credits_members_cross_module() {
    // Issue #1858: a factory function returning an OBJECT LITERAL whose property
    // values are class instances, consumed cross-module as
    // `const ui = createUi(); ui.prop.member()`, must credit the property class's
    // member. Covers every value shape (field member-read, getter member-read,
    // direct `new`, local-const alias, nested literal, assigned-then-returned) and
    // a genuinely-dead member that stays flagged.
    //
    // Regression-strength guards (panel review): the credited method names appear
    // ONLY in the consumer, never in the factory module (a direct in-body
    // `factory.orders.place()` would credit via the shipped compound path, making
    // the test vacuous); and the package is `private` so the exportless-src-index
    // public-API abstain does not mask both the false positive and the fix.
    let unused = unused_member_names(fixture_path("issue-1858-object-literal-factory-return"));

    for credited in [
        "OrdersPage.place",       // field member-read (issue #1858 headline)
        "DashboardPage.open",     // getter member-read
        "CheckoutPage.submit",    // direct `new Class()` in the literal
        "ProfilePage.load",       // local `const` alias to `new Class()`
        "OrdersPage.placeNested", // nested object literal (ui.invoke.orders.member)
        "SettingsPage.save",      // assigned-then-returned (const ui = {...}; return ui)
    ] {
        assert!(
            !unused.contains(&credited.to_string()),
            "{credited} is reached through an object-literal factory return and must be \
             credited (issue #1858), found: {unused:?}"
        );
    }

    // Non-vacuous control: a method reached through no `ui.orders.*` access stays
    // flagged, proving the credit is member-scoped (not a whole-class suppression).
    assert!(
        unused.contains(&"OrdersPage.dead".to_string()),
        "OrdersPage.dead has no call site and must stay flagged (no blanket over-credit), \
         found: {unused:?}"
    );
}

#[test]
fn object_literal_factory_return_credits_member_same_file() {
    // Issue #1858 same-file arm: when the factory and its consumer live in one
    // module (`function createUi() { return { page: new SameFilePage() } }` plus
    // `const ui = createUi(); ui.page.go()`), the member is credited through the
    // loose (same-file) object-shape map, not the cross-module fact join. A dead
    // sibling on the same class stays flagged.
    let unused = unused_member_names(fixture_path("issue-1858-object-literal-same-file"));

    assert!(
        !unused.contains(&"SameFilePage.go".to_string()),
        "SameFilePage.go is reached through a same-file object-literal factory return and \
         must be credited (issue #1858), found: {unused:?}"
    );
    assert!(
        unused.contains(&"SameFilePage.deadSameFile".to_string()),
        "SameFilePage.deadSameFile has no call site and must stay flagged, found: {unused:?}"
    );
}
