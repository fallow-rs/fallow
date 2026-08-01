//! Integration tests for unused / misconfigured npm dependency-override
//! detection (issue #2069).
//!
//! Fixture under `tests/fixtures/issue-2069-npm-overrides/` declares a
//! top-level npm `overrides` object in the root `package.json`:
//!
//! - `sanitize-html` (NOT declared, UNUSED), `axios` (declared, USED),
//!   `@types/react@<18` (declared, USED), nested `react: { react-dom }`
//!   (parent declared, USED via parent crediting), nested
//!   `@scope/legacy-parent: { @scope/legacy-child }` (nothing declared,
//!   UNUSED), `typescript: "$typescript"` (`$package` reference, credited),
//!   empty key (MISCONFIGURED: unparsable), `react@<18: ""` (MISCONFIGURED:
//!   empty value).
//!
//! Workspace member dep sets: `app` declares `react` + `axios`;
//! `lib` declares `@types/react`.

use std::{fs, path::PathBuf};

use fallow_config::{FallowConfig, IgnoreDependencyOverrideRule, OutputFormat};
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
    .resolve(root, OutputFormat::Human, 4, true, true, None)
}

#[test]
fn detects_unused_npm_overrides() {
    let root = fixture_path("issue-2069-npm-overrides");
    let config = config_for_fixture(root, vec![]);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let actual: FxHashSet<(&str, DependencyOverrideSource)> = results
        .unused_dependency_overrides
        .iter()
        .map(|f| (f.entry.target_package.as_str(), f.entry.source))
        .collect();

    let mut expected: FxHashSet<(&str, DependencyOverrideSource)> = FxHashSet::default();
    expected.insert(("sanitize-html", DependencyOverrideSource::PnpmPackageJson));
    expected.insert((
        "@scope/legacy-child",
        DependencyOverrideSource::PnpmPackageJson,
    ));

    assert_eq!(
        actual, expected,
        "expected only sanitize-html + @scope/legacy-child flagged as unused; got {actual:?}"
    );
}

#[test]
fn nested_override_with_declared_parent_is_used() {
    let root = fixture_path("issue-2069-npm-overrides");
    let config = config_for_fixture(root, vec![]);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let any_react_dom = results
        .unused_dependency_overrides
        .iter()
        .any(|f| f.entry.target_package == "react-dom");
    assert!(
        !any_react_dom,
        "nested react: {{ react-dom }} should be USED (parent `react` is declared); flagged: {:?}",
        results.unused_dependency_overrides
    );
}

#[test]
fn nested_unused_override_reports_parent_chain() {
    let root = fixture_path("issue-2069-npm-overrides");
    let config = config_for_fixture(root, vec![]);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let finding = results
        .unused_dependency_overrides
        .iter()
        .find(|f| f.entry.target_package == "@scope/legacy-child")
        .expect("nested unused override should be reported");
    assert_eq!(
        finding.entry.raw_key,
        "@scope/legacy-parent>@scope/legacy-child"
    );
    assert_eq!(
        finding.entry.parent_package.as_deref(),
        Some("@scope/legacy-parent")
    );
}

#[test]
fn dollar_reference_value_is_credited_not_reported() {
    let root = fixture_path("issue-2069-npm-overrides");
    let config = config_for_fixture(root, vec![]);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let any_typescript = results
        .unused_dependency_overrides
        .iter()
        .any(|f| f.entry.target_package == "typescript");
    assert!(
        !any_typescript,
        "`$typescript` reference values resolve indirectly and must not be reported; flagged: {:?}",
        results.unused_dependency_overrides
    );
}

#[test]
fn detects_misconfigured_npm_overrides() {
    let root = fixture_path("issue-2069-npm-overrides");
    let config = config_for_fixture(root, vec![]);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let actual: FxHashSet<(String, DependencyOverrideMisconfigReason)> = results
        .misconfigured_dependency_overrides
        .iter()
        .map(|f| (f.entry.raw_key.clone(), f.entry.reason))
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
fn ignore_rule_suppresses_unused_npm_override() {
    let root = fixture_path("issue-2069-npm-overrides");
    let ignore = vec![IgnoreDependencyOverrideRule {
        package: "sanitize-html".to_string(),
        source: Some("package.json".to_string()),
    }];
    let config = config_for_fixture(root, ignore);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let any_sanitize_html = results
        .unused_dependency_overrides
        .iter()
        .any(|f| f.entry.target_package == "sanitize-html");
    assert!(
        !any_sanitize_html,
        "sanitize-html should be suppressed by the ignoreDependencyOverrides rule; flagged: {:?}",
        results.unused_dependency_overrides
    );
}

#[test]
fn transitive_only_targets_in_npm_lockfile_are_used() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path();
    fs::write(
        root.join("package.json"),
        r#"{
  "name": "issue-2069-transitive-overrides",
  "private": true,
  "version": "0.0.0",
  "overrides": {
    "postcss": ">=8.5.10",
    "lodash": ">=4.18.0",
    "@babel/runtime": ">=7.26.10"
  }
}"#,
    )
    .expect("write root package.json");
    fs::write(
        root.join("package-lock.json"),
        r#"{
  "name": "issue-2069-transitive-overrides",
  "version": "0.0.0",
  "lockfileVersion": 3,
  "packages": {
    "": {
      "name": "issue-2069-transitive-overrides",
      "version": "0.0.0"
    },
    "node_modules/postcss": {
      "version": "8.5.10"
    },
    "node_modules/some-lib/node_modules/lodash": {
      "version": "4.18.0"
    }
  }
}"#,
    )
    .expect("write npm lockfile");

    let config = FallowConfig::default().resolve(
        root.to_path_buf(),
        OutputFormat::Human,
        4,
        true,
        true,
        None,
    );
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let actual: FxHashSet<&str> = results
        .unused_dependency_overrides
        .iter()
        .map(|finding| finding.entry.target_package.as_str())
        .collect();

    assert_eq!(
        actual,
        FxHashSet::from_iter(["@babel/runtime"]),
        "lockfile-resolved transitive packages should not be reported as unused; got {:?}",
        results.unused_dependency_overrides
    );
}
