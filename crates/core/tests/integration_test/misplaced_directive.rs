use fallow_config::{FallowConfig, OutputFormat, RulesConfig, Severity};

use crate::common::fixture_path;

/// Resolve a fixture with the `misplaced-directive` rule at `warn` (its
/// default). The detector is gated on the project declaring `next`, which the
/// `misplaced-directive` fixture's `package.json` does.
fn fixture_config(name: &str) -> fallow_config::ResolvedConfig {
    FallowConfig {
        rules: RulesConfig {
            misplaced_directive: Severity::Warn,
            ..RulesConfig::default()
        },
        ..Default::default()
    }
    .resolve(fixture_path(name), OutputFormat::Human, 4, true, true, None)
}

#[test]
fn post_import_directive_is_flagged_once() {
    let config = fixture_config("misplaced-directive");
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let findings: Vec<(String, String)> = results
        .misplaced_directives
        .iter()
        .map(|f| {
            (
                f.directive_site.directive.clone(),
                f.directive_site.path.to_string_lossy().replace('\\', "/"),
            )
        })
        .collect();

    assert_eq!(
        findings.len(),
        1,
        "exactly one misplaced directive expected: {findings:?}"
    );
    assert_eq!(findings[0].0, "use client");
    assert!(
        findings[0].1.ends_with("app/page.tsx"),
        "finding should anchor at app/page.tsx, got {}",
        findings[0].1
    );
}

#[test]
fn leading_directive_is_not_flagged() {
    let config = fixture_config("misplaced-directive");
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    // app/widget.tsx carries a correctly-positioned leading "use client", which
    // oxc places in `program.directives`; it must never be flagged.
    assert!(
        !results.misplaced_directives.iter().any(|f| f
            .directive_site
            .path
            .to_string_lossy()
            .replace('\\', "/")
            .ends_with("app/widget.tsx")),
        "leading-directive file must not produce a finding"
    );
}

#[test]
fn no_findings_when_next_is_absent() {
    let config = fixture_config("misplaced-directive-no-next");
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    assert!(
        results.misplaced_directives.is_empty(),
        "without `next` declared, the rule must not fire: {:?}",
        results.misplaced_directives
    );
}

/// W4.3: `misplaced-directive` is universal RSC semantics, so it fires for a
/// framework-agnostic RSC bundler (Waku), not just Next.
#[test]
fn waku_project_flags_misplaced_directive() {
    let config = fixture_config("misplaced-directive-waku");
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    assert!(
        results.misplaced_directives.iter().any(|f| f
            .directive_site
            .path
            .to_string_lossy()
            .replace('\\', "/")
            .ends_with("app/page.tsx")
            && f.directive_site.directive == "use client"),
        "a body-position `use client` in a Waku project must be flagged: {:?}",
        results.misplaced_directives
    );
}

/// W4.3: the two Next-specific RSC rules stay Next-gated. Under a Waku project,
/// `invalid-client-export` (illegal names are Next route-segment config) and
/// `unused-server-action` (Server Action registration is Next-specific) must not
/// fire even though the same fixture carries a `metadata` client export and a
/// dead `"use server"` action.
#[test]
fn waku_project_does_not_flag_next_specific_rules() {
    let config = fixture_config("misplaced-directive-waku");
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    assert!(
        results.invalid_client_exports.is_empty(),
        "invalid-client-export is Next-only and must not fire under Waku: {:?}",
        results.invalid_client_exports
    );
    assert!(
        results.unused_server_actions.is_empty(),
        "unused-server-action is Next-only and must not fire under Waku: {:?}",
        results.unused_server_actions
    );
}
