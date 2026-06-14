use fallow_config::{FallowConfig, OutputFormat, RulesConfig, Severity};

use crate::common::fixture_path;

/// Resolve the fixture with the default rule set: `unused-server-action` at
/// `warn` and `unused-export` at `error` (both its defaults). The detector is
/// gated on the project declaring `next`, which the fixture's package.json does.
fn fixture_config(name: &str) -> fallow_config::ResolvedConfig {
    FallowConfig::default().resolve(fixture_path(name), OutputFormat::Human, 4, true, true, None)
}

/// Same fixture, but with the `unused-server-action` rule turned off so the
/// reclassification does not run.
fn fixture_config_rule_off(name: &str) -> fallow_config::ResolvedConfig {
    FallowConfig {
        rules: RulesConfig {
            unused_server_actions: Severity::Off,
            ..RulesConfig::default()
        },
        ..Default::default()
    }
    .resolve(fixture_path(name), OutputFormat::Human, 4, true, true, None)
}

fn action_names(results: &fallow_core::results::AnalysisResults) -> Vec<String> {
    results
        .unused_server_actions
        .iter()
        .map(|f| f.action.action_name.clone())
        .collect()
}

fn export_names(results: &fallow_core::results::AnalysisResults) -> Vec<String> {
    results
        .unused_exports
        .iter()
        .map(|f| f.export.export_name.clone())
        .collect()
}

#[test]
fn dead_action_is_flagged_and_anchored() {
    let config = fixture_config("unused-server-action");
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let actions = action_names(&results);
    assert!(
        actions.contains(&"deadAction".to_string()),
        "deadAction should be flagged as unused-server-action: {actions:?}"
    );

    let dead = results
        .unused_server_actions
        .iter()
        .find(|f| f.action.action_name == "deadAction")
        .expect("deadAction finding");
    assert!(
        dead.action
            .path
            .to_string_lossy()
            .replace('\\', "/")
            .ends_with("app/actions.ts"),
        "finding should anchor at app/actions.ts, got {}",
        dead.action.path.display()
    );
}

#[test]
fn referenced_actions_are_not_flagged() {
    let config = fixture_config("unused-server-action");
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let actions = action_names(&results);
    // formAction (<form action={...}>), callAction (import-and-call), and
    // propAction (component prop) are all referenced, so none is flagged.
    for referenced in ["formAction", "callAction", "propAction"] {
        assert!(
            !actions.contains(&referenced.to_string()),
            "{referenced} is referenced and must not be flagged: {actions:?}"
        );
    }
}

#[test]
fn reclassification_removes_dead_action_from_unused_exports() {
    let config = fixture_config("unused-server-action");
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    // With the rule active, deadAction is reclassified out of unused_exports.
    let exports = export_names(&results);
    assert!(
        !exports.contains(&"deadAction".to_string()),
        "deadAction should be reclassified out of unused_exports: {exports:?}"
    );
}

#[test]
fn non_use_server_dead_export_stays_unused_export() {
    let config = fixture_config("unused-server-action");
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    // deadUtil lives in a plain module (no "use server"); it must stay an
    // ordinary unused-export and never be reclassified.
    let exports = export_names(&results);
    assert!(
        exports.contains(&"deadUtil".to_string()),
        "deadUtil should remain an unused-export: {exports:?}"
    );
    let actions = action_names(&results);
    assert!(
        !actions.contains(&"deadUtil".to_string()),
        "deadUtil must NOT be reclassified as a server action: {actions:?}"
    );
}

#[test]
fn suppressed_action_is_in_neither_bucket() {
    let config = fixture_config("unused-server-action");
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let actions = action_names(&results);
    let exports = export_names(&results);
    assert!(
        !actions.contains(&"suppressedDeadAction".to_string()),
        "suppressed action must not appear as unused-server-action: {actions:?}"
    );
    assert!(
        !exports.contains(&"suppressedDeadAction".to_string()),
        "suppressed action must not leak back into unused_exports: {exports:?}"
    );
    // The suppression is consumed, not stale.
    assert!(
        !results.stale_suppressions.iter().any(|s| s
            .path
            .to_string_lossy()
            .replace('\\', "/")
            .ends_with("app/actions.ts")),
        "the unused-server-action suppression should be consumed, not stale: {:?}",
        results.stale_suppressions
    );
}

#[test]
fn rule_off_keeps_dead_action_as_unused_export() {
    let config = fixture_config_rule_off("unused-server-action");
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    // Neuter check: with the rule off, no reclassification happens.
    assert!(
        results.unused_server_actions.is_empty(),
        "rule off must produce no unused-server-action findings: {:?}",
        action_names(&results)
    );
    let exports = export_names(&results);
    assert!(
        exports.contains(&"deadAction".to_string()),
        "with the rule off, deadAction should stay an unused-export: {exports:?}"
    );
}

#[test]
fn no_findings_when_next_is_absent() {
    let config = fixture_config("unused-server-action-no-next");
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    assert!(
        results.unused_server_actions.is_empty(),
        "without `next` declared, the rule must not fire: {:?}",
        action_names(&results)
    );
    // The dead action still surfaces as a plain unused-export.
    assert!(
        export_names(&results).contains(&"deadAction".to_string()),
        "deadAction should remain an unused-export without next"
    );
}
