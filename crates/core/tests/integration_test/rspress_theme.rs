use super::common::{create_config, fixture_path};

/// rspress projects import the framework's theme layer through the `@theme`
/// build-time virtual module (and `@theme/<component>` subpaths). The rspress
/// plugin contributes `@theme/` / `@theme-original/` as virtual module
/// prefixes, so neither the bare specifier nor a subpath should surface as an
/// `unlisted-dependency` or an `unresolved-import`. See issue #756.
#[test]
fn rspress_theme_virtual_module_is_not_unlisted_or_unresolved() {
    let root = fixture_path("issue-756-rspress-theme");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unlisted: Vec<&str> = results
        .unlisted_dependencies
        .iter()
        .map(|d| d.dep.package_name.as_str())
        .collect();

    assert!(
        !unlisted.contains(&"@theme"),
        "bare `@theme` virtual module should not be reported as unlisted, found: {unlisted:?}"
    );
    assert!(
        !unlisted.contains(&"@theme/Layout"),
        "`@theme/Layout` subpath should not be reported as unlisted, found: {unlisted:?}"
    );

    let unresolved: Vec<&str> = results
        .unresolved_imports
        .iter()
        .map(|i| i.import.specifier.as_str())
        .collect();
    assert!(
        !unresolved.contains(&"@theme"),
        "bare `@theme` should not be reported as unresolved, found: {unresolved:?}"
    );
    assert!(
        !unresolved.contains(&"@theme/Layout"),
        "`@theme/Layout` should not be reported as unresolved, found: {unresolved:?}"
    );

    // Non-vacuous control: a genuinely-missing bare package MUST still report,
    // proving dependency detection actually ran on this fixture.
    assert!(
        unlisted.contains(&"definitely-missing-pkg"),
        "a real missing dependency must still report as unlisted, found: {unlisted:?}"
    );
}
