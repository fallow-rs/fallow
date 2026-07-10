use super::common::{create_config, fixture_path};

/// A member reached through a LOCAL (non-exported) subclass must credit the class
/// that declares it.
///
/// `class Sub extends Base {}` never becomes an export, so the analyze layer's
/// import/export map cannot resolve `Sub.someStatic`, and the heritage
/// `parent -> children` map is built from exports alone. Every member reached only
/// through the subclass was therefore reported unused. Exporting the subclass makes
/// the identical code resolve, which is what pinned the diagnosis.
///
/// The fixture is deliberately cross-file: the subclass lives one module away from
/// the base, which is what real code looks like and what a same-file fixture cannot
/// exercise. `UrlSyncManager.trulyDead` and `instanceTrulyDead` are the non-vacuous
/// controls proving the detector still fires.
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

#[test]
fn local_subclass_credits_members_on_the_declaring_class() {
    let unused = unused_members("local-subclass-static-class-members");

    for member in [
        // Static method, static passed as a value, and a static arrow property.
        // There is no method/arrow divergence: propagation is on member names.
        "calledViaSub",
        "passedViaSub",
        "arrowViaSub",
        // A two-level local chain.
        "viaGrandchild",
        // Reached through the base directly; never affected by this bug.
        "passedViaBase",
        // `const instance = new MapUrlSyncManager(); instance.instanceViaSub()`.
        // This access only exists once `resolve_bound_member_accesses` rewrites the
        // bound local to its class, so the subclass pass must run after it.
        "instanceViaSub",
        // A chain of 18 local classes. Cycle detection, not a depth cap, terminates
        // the walk; a fixed cap would abstain here and report this falsely.
        "viaDeepChain",
        // The subclass declares its own `shadowedOnSub`, so this credit on the base
        // is an over-credit. It is the accepted direction: a false negative, never a
        // false positive.
        "shadowedOnSub",
    ] {
        assert!(
            !unused.contains(&format!("UrlSyncManager.{member}")),
            "UrlSyncManager.{member} is reached through a local subclass and must be \
             credited, found: {unused:?}"
        );
    }

    for dead in ["trulyDead", "instanceTrulyDead"] {
        assert!(
            unused.contains(&format!("UrlSyncManager.{dead}")),
            "UrlSyncManager.{dead} is never referenced and must still be reported, \
             otherwise this fixture passes vacuously: {unused:?}"
        );
    }
}

/// `class Sub extends mixin(Base) {}` records no superclass, because a mixin call is
/// not a bare identifier. The walk abstains and `viaMixin` stays reported.
///
/// This is a known, pre-existing false positive, not one this change introduces:
/// crediting through a mixin would be a guess, since the mixin may redefine what the
/// subclass exposes. Pinned here so a future change to superclass extraction has to
/// decide this case deliberately rather than by accident.
#[test]
fn mixin_superclass_abstains_rather_than_guessing() {
    let unused = unused_members("local-subclass-static-class-members");

    assert!(
        unused.contains(&"UrlSyncManager.viaMixin".to_string()),
        "a mixin superclass carries no resolvable name, so the walk must abstain; \
         if this now passes, superclass extraction changed and the abstention should \
         be revisited: {unused:?}"
    );
}

/// `class Sub extends ns.UrlSyncManager {}` walks to the dotted name `ns.UrlSyncManager`,
/// which the analyze layer cannot resolve: its import/export map keys bare local names,
/// not namespace-qualified ones. `viaNamespaceBase` therefore stays reported.
///
/// This is a pre-existing gap, not one this change introduces, and it is wider than
/// subclassing: a direct `ns.UrlSyncManager.viaNamespaceBase()` is equally uncredited on
/// `main`. Closing it means resolving namespace aliases in the analyze layer.
///
/// Pinned so that a future change to namespace resolution has to decide this case
/// deliberately rather than flip it by accident.
#[test]
fn namespace_qualified_base_is_a_known_uncredited_gap() {
    let unused = unused_members("local-subclass-static-class-members");

    assert!(
        unused.contains(&"UrlSyncManager.viaNamespaceBase".to_string()),
        "a namespace-qualified base resolves to no bare local name, so this member is \
         expected to stay reported; if it is now credited, namespace resolution changed \
         and this gap should be closed deliberately: {unused:?}"
    );
}
