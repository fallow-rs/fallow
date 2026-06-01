use crate::common::{create_config, fixture_path};

/// End-to-end regression for issue #843: the `nestjs` plugin must credit
/// framework-dispatched lifecycle and handler methods so they do not surface
/// as `unused-class-member`, while genuinely unused methods (and same-named
/// methods on classes that do NOT implement a Nest interface) still report.
#[test]
fn nestjs_lifecycle_and_handler_methods_credited_but_genuinely_unused_reported() {
    let root = fixture_path("issue-843-nestjs-lifecycle");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused: Vec<String> = results
        .unused_class_members
        .iter()
        .map(|m| format!("{}.{}", m.member.parent_name, m.member.member_name))
        .collect();

    // NestModule + lifecycle hooks on a class implementing the interfaces:
    // none of these may be flagged.
    for credited in [
        "AppModule.configure",
        "AppModule.onModuleInit",
        "AppModule.onModuleDestroy",
        // Sibling lifecycle hook NOT in the implements clause: still credited
        // because Nest dispatches by duck-typed method presence.
        "AppModule.onApplicationBootstrap",
        // Guard dispatch method.
        "AuthGuard.canActivate",
    ] {
        assert!(
            !unused.contains(&credited.to_string()),
            "`{credited}` is framework-dispatched and must be credited, found unused: {unused:?}"
        );
    }

    // Genuinely unused helpers on Nest classes must still report.
    assert!(
        unused.contains(&"AppModule.unusedModuleHelper".to_string()),
        "unused non-lifecycle method on a Nest module should still report, found: {unused:?}"
    );
    assert!(
        unused.contains(&"AuthGuard.unusedGuardHelper".to_string()),
        "unused non-lifecycle method on a Nest guard should still report, found: {unused:?}"
    );

    // Heritage scoping: a plain class that implements NONE of the Nest
    // interfaces must STILL report its lifecycle-named method, proving the
    // rules are `implements`-scoped and not global.
    assert!(
        unused.contains(&"PlainService.onModuleInit".to_string()),
        "`onModuleInit` on a class implementing no lifecycle interface must still report, found: {unused:?}"
    );
}
