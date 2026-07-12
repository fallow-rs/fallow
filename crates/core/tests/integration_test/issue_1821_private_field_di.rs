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
fn private_field_di_credits_member_cross_module() {
    // Issue #1821 (Fix A): a member reached through a `#`-private DI field
    // (`this.#dep.m()`) on a class in ANOTHER module must be credited, the same
    // way a public `this.dep.m()` receiver already is. Consumers live in
    // separate files from the dep classes on purpose: a same-file fixture would
    // pass for the wrong reason via the file-level self-access map. Three
    // receiver shapes are covered:
    //   - inline-new `readonly #dep = new Dep()`
    //   - bare `#dep` + ctor `this.#dep = new Dep()`
    //   - interface-typed `#svc: IDep` crediting the implementer
    // Every dep class also carries a genuinely-dead control member that must
    // STAY flagged, proving the fix credits precisely and never over-credits.
    let unused = unused_member_names(fixture_path("issue-1821-private-field-di"));

    for credited in [
        "DepInlineNew.inlineM",
        "DepCtorNew.ctorNewM",
        "DepIface.ifaceM",
    ] {
        assert!(
            !unused.contains(&credited.to_string()),
            "{credited} is reached through a `#`-private DI field and must be credited \
             (issue #1821), found: {unused:?}"
        );
    }
    for control in [
        "DepInlineNew.deadOnInlineNew",
        "DepCtorNew.deadOnCtorNew",
        "DepIface.deadOnDepIface",
    ] {
        assert!(
            unused.contains(&control.to_string()),
            "{control} has no call site and must stay flagged (no blanket over-credit), \
             found: {unused:?}"
        );
    }
}
