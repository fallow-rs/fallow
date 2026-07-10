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

/// Members read off a cross-module factory result must be credited whatever shape
/// the consumer uses to read them.
///
/// Cross-file factory-return propagation already worked for `const s = f(); s.m`.
/// It never fired for a chained call (`f().m`) or a destructure
/// (`const { m } = f()`), because the extractor only recorded a factory-return
/// candidate for a plain `BindingIdentifier` declarator, and a chained call is not a
/// declarator at all. Those two shapes are what real composables and singletons use,
/// so every member of the returned class was reported unused.
///
/// The fixture is cross-file on purpose. A same-file factory is skipped wholesale by
/// the whole-object gate (the returned instance escapes), so a same-file fixture
/// reports nothing and passes for the wrong reason.
///
/// `RESTApi.trulyDead` is the non-vacuous control proving the detector still fires.
#[test]
fn cross_file_factory_credits_every_consumer_access_shape() {
    let unused = unused_members("cross-file-factory-class-members");

    for member in [
        // Already worked; guards against a regression.
        "viaSimpleBinding",
        // `useApi().viaChainedCall`
        "viaChainedCall",
        // `useApi().viaChainedCallThenCall()`
        "viaChainedCallThenCall",
        // `useApi().viaChainedThenDeep.deep` credits the first level only.
        "viaChainedThenDeep",
        // `const { viaDestructure } = useApi()`
        "viaDestructure",
        // `const { viaRenamedKey: renamed } = useApi()`
        "viaRenamedKey",
        // `const { viaDefaultedKey = 0 } = useApi()`
        "viaDefaultedKey",
        // `const { viaNestedKey: { inner } } = useApi()` credits `viaNestedKey`, not
        // `inner`, which belongs to whatever type that property has.
        "viaNestedKey",
        // `useApi()?.viaOptionalChain`
        "viaOptionalChain",
    ] {
        assert!(
            !unused.contains(&format!("RESTApi.{member}")),
            "RESTApi.{member} is read off the factory result and must be credited, \
             found: {unused:?}"
        );
    }

    assert!(
        unused.contains(&"RESTApi.trulyDead".to_string()),
        "RESTApi.trulyDead is never read and must still be reported, otherwise this \
         fixture passes vacuously: {unused:?}"
    );

    // `deep` and `inner` belong to the property's type, not to RESTApi. Crediting
    // them would credit a same-named member of an unrelated class.
    for leaked in ["deep", "inner"] {
        assert!(
            !unused
                .iter()
                .any(|entry| entry == &format!("RESTApi.{leaked}")),
            "second-level key {leaked} must not be recorded as a RESTApi member"
        );
    }
}

/// A callee that is not a proven factory (`helper().anything`) is read the same way a
/// factory result is. It must resolve to no exported factory return, credit nothing,
/// and suppress nothing: the extractor records every `identifier().member`, and only
/// the analyze layer's strict gate decides which of them mean anything.
#[test]
fn a_non_factory_callee_credits_nothing() {
    let unused = unused_members("cross-file-factory-class-members");

    // `helper` returns an object literal, not a class, so nothing named `anything` may
    // be credited against RESTApi, and RESTApi must not be suppressed wholesale.
    assert!(
        unused.contains(&"RESTApi.trulyDead".to_string()),
        "a non-factory callee must not suppress the factory's class: {unused:?}"
    );
    assert!(
        !unused.iter().any(|entry| entry.ends_with(".anything")),
        "`helper().anything` names no class member: {unused:?}"
    );
}

/// `const { named, ...rest } = useApi()` can read ANY property through `rest`, so no
/// set of visible keys describes what is used.
///
/// Crediting only `named` would leave every other live member reported as dead --
/// the exact false positive this change removes. The returned class is marked wholly
/// used instead, so nothing is reported for it. That is a deliberate false negative:
/// under-reporting is safe, over-reporting is not.
#[test]
fn opaque_destructure_of_a_factory_result_suppresses_the_class() {
    let unused = unused_members("cross-file-factory-opaque-destructure");

    let rest_api: Vec<&String> = unused
        .iter()
        .filter(|entry| entry.starts_with("RESTApi."))
        .collect();

    assert!(
        rest_api.is_empty(),
        "a rest destructure can expose any property, so no RESTApi member may be \
         reported: {rest_api:?}"
    );
}
