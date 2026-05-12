//! Integration tests for unused / misconfigured pnpm dependency-override
//! detection (issue #336).
//!
//! Fixture under `tests/fixtures/issue-336-unused-overrides/` declares
//! overrides in BOTH sources:
//!
//! - `pnpm-workspace.yaml` `overrides:`: `axios` (declared, USED),
//!   `@types/react@<18` (declared, USED), `react>react-dom` (parent declared,
//!   USED via parent-chain rule), `lodash` (NOT declared, UNUSED).
//! - root `package.json` `pnpm.overrides`: `@scope/legacy-pkg` (NOT declared,
//!   UNUSED), `react>react-dom` (parent declared, USED), empty key
//!   (MISCONFIGURED: unparsable), `react@<18: ""` (MISCONFIGURED: empty
//!   value).
//!
//! Workspace member dep sets: `app` declares `react` + `axios`;
//! `lib` declares `@types/react`.

use std::path::PathBuf;

use fallow_config::{
    FallowConfig, IgnoreDependencyOverrideRule, OutputFormat, RulesConfig, Severity,
};
use fallow_types::results::{DependencyOverrideMisconfigReason, DependencyOverrideSource};
use rustc_hash::FxHashSet;

use super::common::fixture_path;

fn config_for_fixture(
    root: PathBuf,
    ignore: Vec<IgnoreDependencyOverrideRule>,
) -> fallow_config::ResolvedConfig {
    FallowConfig {
        ignore_dependency_overrides: ignore,
        ..Default::default()
    }
    .resolve(root, OutputFormat::Human, 4, true, true)
}

#[test]
fn detects_unused_overrides_across_both_sources() {
    let root = fixture_path("issue-336-unused-overrides");
    let config = config_for_fixture(root, vec![]);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let actual: FxHashSet<(&str, DependencyOverrideSource)> = results
        .unused_dependency_overrides
        .iter()
        .map(|f| (f.target_package.as_str(), f.source))
        .collect();

    let mut expected: FxHashSet<(&str, DependencyOverrideSource)> = FxHashSet::default();
    expected.insert(("lodash", DependencyOverrideSource::PnpmWorkspaceYaml));
    expected.insert((
        "@scope/legacy-pkg",
        DependencyOverrideSource::PnpmPackageJson,
    ));

    assert_eq!(
        actual, expected,
        "expected only lodash + @scope/legacy-pkg flagged as unused; got {actual:?}"
    );
}

#[test]
fn parent_chain_with_declared_parent_is_used() {
    let root = fixture_path("issue-336-unused-overrides");
    let config = config_for_fixture(root, vec![]);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let any_react_dom = results
        .unused_dependency_overrides
        .iter()
        .any(|f| f.target_package == "react-dom");
    assert!(
        !any_react_dom,
        "react>react-dom should be USED (parent `react` is declared); flagged: {:?}",
        results.unused_dependency_overrides
    );
}

#[test]
fn target_with_version_selector_is_resolved() {
    let root = fixture_path("issue-336-unused-overrides");
    let config = config_for_fixture(root, vec![]);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let any_types_react = results
        .unused_dependency_overrides
        .iter()
        .any(|f| f.target_package == "@types/react");
    assert!(
        !any_types_react,
        "@types/react@<18 should resolve target=@types/react which IS declared; flagged: {:?}",
        results.unused_dependency_overrides
    );
}

#[test]
fn detects_misconfigured_overrides() {
    let root = fixture_path("issue-336-unused-overrides");
    let config = config_for_fixture(root, vec![]);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let actual: FxHashSet<(String, DependencyOverrideMisconfigReason)> = results
        .misconfigured_dependency_overrides
        .iter()
        .map(|f| (f.raw_key.clone(), f.reason))
        .collect();

    let mut expected: FxHashSet<(String, DependencyOverrideMisconfigReason)> = FxHashSet::default();
    expected.insert((
        String::new(),
        DependencyOverrideMisconfigReason::UnparsableKey,
    ));
    expected.insert((
        "react@<18".to_string(),
        DependencyOverrideMisconfigReason::EmptyValue,
    ));

    assert_eq!(
        actual, expected,
        "expected unparsable empty-key + empty-value entries; got {actual:?}"
    );
}

#[test]
fn ignore_rule_suppresses_unused_override() {
    let root = fixture_path("issue-336-unused-overrides");
    let ignore = vec![IgnoreDependencyOverrideRule {
        package: "lodash".to_string(),
        source: None,
    }];
    let config = config_for_fixture(root, ignore);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let any_lodash = results
        .unused_dependency_overrides
        .iter()
        .any(|f| f.target_package == "lodash");
    assert!(
        !any_lodash,
        "lodash should be suppressed by the ignoreDependencyOverrides rule; flagged: {:?}",
        results.unused_dependency_overrides
    );
}

#[test]
fn ignore_rule_scoped_by_source_only_affects_matching_source() {
    let root = fixture_path("issue-336-unused-overrides");
    // Ignore lodash only when declared in package.json (it lives in YAML, so
    // the suppression should NOT apply).
    let ignore = vec![IgnoreDependencyOverrideRule {
        package: "lodash".to_string(),
        source: Some("package.json".to_string()),
    }];
    let config = config_for_fixture(root, ignore);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let any_lodash = results
        .unused_dependency_overrides
        .iter()
        .any(|f| f.target_package == "lodash");
    assert!(
        any_lodash,
        "lodash override is in YAML; suppression scoped to package.json must not match; got {:?}",
        results.unused_dependency_overrides
    );
}

#[test]
fn severity_off_short_circuits() {
    let root = fixture_path("issue-336-unused-overrides");
    let rules = RulesConfig {
        unused_dependency_overrides: Severity::Off,
        misconfigured_dependency_overrides: Severity::Off,
        ..RulesConfig::default()
    };
    let config = FallowConfig {
        rules,
        ..Default::default()
    }
    .resolve(root, OutputFormat::Human, 4, true, true);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    assert!(
        results.unused_dependency_overrides.is_empty(),
        "Severity::Off must suppress unused overrides; got {:?}",
        results.unused_dependency_overrides
    );
    assert!(
        results.misconfigured_dependency_overrides.is_empty(),
        "Severity::Off must suppress misconfigured overrides; got {:?}",
        results.misconfigured_dependency_overrides
    );
}

#[test]
fn unused_overrides_carry_transitive_hint_on_every_shape() {
    // Both bare-target AND parent-chain unused findings must carry the
    // transitive-CVE hint so agents can de-prioritize. Synthesize a tempdir
    // with one of each shape and confirm the hint fires on both.
    use std::fs;
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path();
    fs::write(
        root.join("package.json"),
        r#"{"name": "tmp", "private": true, "version": "0.0.0"}"#,
    )
    .expect("write root pkg");
    fs::write(
        root.join("pnpm-workspace.yaml"),
        "packages:\n  - 'packages/*'\n\noverrides:\n  bare-orphan: \"^1.0.0\"\n  \"unrelated-parent>orphaned-target\": \"^1.0.0\"\n",
    )
    .expect("write yaml");

    let config =
        FallowConfig::default().resolve(root.to_path_buf(), OutputFormat::Human, 4, true, true);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    assert_eq!(results.unused_dependency_overrides.len(), 2);
    for finding in &results.unused_dependency_overrides {
        assert!(
            finding.hint.is_some(),
            "every unused override (bare-target or parent-chain) should carry the transitive hint; missing on {:?}",
            finding.raw_key
        );
    }
}
