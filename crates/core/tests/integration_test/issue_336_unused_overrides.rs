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

use std::{fs, path::PathBuf};

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
    .resolve(root, OutputFormat::Human, 4, true, true, None)
}

#[test]
fn detects_unused_overrides_across_both_sources() {
    let root = fixture_path("issue-336-unused-overrides");
    let config = config_for_fixture(root, vec![]);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let actual: FxHashSet<(&str, DependencyOverrideSource)> = results
        .unused_dependency_overrides
        .iter()
        .map(|f| (f.entry.target_package.as_str(), f.entry.source))
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
        .any(|f| f.entry.target_package == "react-dom");
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
        .any(|f| f.entry.target_package == "@types/react");
    assert!(
        !any_types_react,
        "@types/react@<18 should resolve target=@types/react which IS declared; flagged: {:?}",
        results.unused_dependency_overrides
    );
}

#[test]
fn transitive_only_targets_in_pnpm_lockfile_are_used() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path();
    fs::write(
        root.join("package.json"),
        r#"{
  "name": "issue-371-transitive-overrides",
  "private": true,
  "version": "0.0.0",
  "pnpm": {
    "overrides": {
      "postcss": ">=8.5.10",
      "lodash": ">=4.18.0",
      "@babel/runtime": ">=7.26.10"
    }
  }
}"#,
    )
    .expect("write root package.json");
    fs::write(
        root.join("pnpm-lock.yaml"),
        r"lockfileVersion: '9.0'

packages:
  postcss@8.5.10:
    resolution: {integrity: sha512-postcss}
  lodash@4.17.21:
    resolution: {integrity: sha512-lodash}

snapshots:
  postcss@8.5.10: {}
  lodash@4.17.21: {}
",
    )
    .expect("write pnpm lockfile");

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

// Trimmed from a real `bun install` run (bun 1.3.x); keeps bun's trailing
// commas so the JSONC dialect is exercised end to end. `ws` is a transitive
// dependency of `happy-dom`, declared in no dependency section.
const BUN_LOCK_TRANSITIVE_WS: &str = r#"{
  "lockfileVersion": 1,
  "configVersion": 1,
  "workspaces": {
    "": {
      "name": "issue-2341-bun-overrides",
      "devDependencies": {
        "happy-dom": "^20.10.6",
      },
    },
  },
  "overrides": {
    "ws": "^8.21.0",
  },
  "packages": {
    "@types/whatwg-mimetype": ["@types/whatwg-mimetype@3.0.2", "", {}, "sha512-c2"],
    "happy-dom": ["happy-dom@20.11.6", "", { "dependencies": { "@types/whatwg-mimetype": "^3.0.2", "whatwg-mimetype": "^3.0.0", "ws": "^8.21.0" } }, "sha512-Hl"],
    "whatwg-mimetype": ["whatwg-mimetype@3.0.0", "", {}, "sha512-nt"],
    "ws": ["ws@8.21.3", "", { "peerDependencies": { "bufferutil": "^4.0.1", "utf-8-validate": ">=5.0.2" }, "optionalPeers": ["bufferutil", "utf-8-validate"] }, "sha512-20"],
  }
}
"#;

fn write_bun_repro_package_json(root: &std::path::Path, overrides: &str) {
    fs::write(
        root.join("package.json"),
        format!(
            r#"{{
  "name": "issue-2341-bun-overrides",
  "private": true,
  "version": "0.0.0",
  "packageManager": "bun@1.3.2",
  "devDependencies": {{ "happy-dom": "^20.10.6" }},
  "overrides": {overrides}
}}"#
        ),
    )
    .expect("write root package.json");
}

/// Issue #2341 repro: bun declares overrides through the npm-style top-level
/// `overrides` key, and transitive resolution lives in `bun.lock`. The
/// override target must be credited from the bun lockfile instead of being
/// flagged as unused.
#[test]
fn transitive_only_targets_in_bun_lockfile_are_used() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path();
    write_bun_repro_package_json(root, r#"{ "ws": "^8.21.0" }"#);
    fs::write(root.join("bun.lock"), BUN_LOCK_TRANSITIVE_WS).expect("write bun.lock");

    let config = config_for_fixture(root.to_path_buf(), vec![]);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    assert!(
        results.unused_dependency_overrides.is_empty(),
        "ws resolves in bun.lock as a transitive dependency of happy-dom; flagged: {:?}",
        results.unused_dependency_overrides
    );
}

#[test]
fn unresolved_override_in_bun_repo_is_flagged_with_bun_hint() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path();
    write_bun_repro_package_json(root, r#"{ "left-pad": "^1.3.0" }"#);
    fs::write(root.join("bun.lock"), BUN_LOCK_TRANSITIVE_WS).expect("write bun.lock");

    let config = config_for_fixture(root.to_path_buf(), vec![]);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let flagged: Vec<&str> = results
        .unused_dependency_overrides
        .iter()
        .map(|f| f.entry.target_package.as_str())
        .collect();
    assert_eq!(
        flagged,
        vec!["left-pad"],
        "left-pad appears in no dep section and not in bun.lock"
    );
    let hint = results.unused_dependency_overrides[0]
        .entry
        .hint
        .as_deref()
        .expect("unused override carries a hint");
    assert!(
        hint.contains("bun install"),
        "hint should name bun, not pnpm, in a bun repo; got {hint:?}"
    );
}

/// bun's legacy binary lockfile cannot be parsed, so resolution ground truth
/// is unavailable; the analysis must stay silent instead of flagging every
/// transitive-only override.
#[test]
fn bun_lockb_without_text_lockfile_suppresses_unused_overrides() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path();
    write_bun_repro_package_json(root, r#"{ "ws": "^8.21.0" }"#);
    fs::write(root.join("bun.lockb"), b"\x00binary lockfile\x01\x02").expect("write bun.lockb");

    let config = config_for_fixture(root.to_path_buf(), vec![]);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    assert!(
        results.unused_dependency_overrides.is_empty(),
        "bun.lockb is unreadable; unused-override analysis must be skipped; flagged: {:?}",
        results.unused_dependency_overrides
    );
}

/// Misconfigured-override detection is static and does not depend on lockfile
/// resolution, so it keeps reporting even when only `bun.lockb` exists.
#[test]
fn bun_lockb_does_not_suppress_misconfigured_overrides() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path();
    write_bun_repro_package_json(root, r#"{ "ws": "" }"#);
    fs::write(root.join("bun.lockb"), b"\x00binary lockfile\x01\x02").expect("write bun.lockb");

    let config = config_for_fixture(root.to_path_buf(), vec![]);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let any_empty_value = results.misconfigured_dependency_overrides.iter().any(|f| {
        f.entry.raw_key == "ws" && f.entry.reason == DependencyOverrideMisconfigReason::EmptyValue
    });
    assert!(
        any_empty_value,
        "empty-value override must stay reported with bun.lockb present; got {:?}",
        results.misconfigured_dependency_overrides
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
        .any(|f| f.entry.target_package == "lodash");
    assert!(
        !any_lodash,
        "lodash should be suppressed by the ignoreDependencyOverrides rule; flagged: {:?}",
        results.unused_dependency_overrides
    );
}

#[test]
fn ignore_rule_scoped_by_source_only_affects_matching_source() {
    let root = fixture_path("issue-336-unused-overrides");
    let ignore = vec![IgnoreDependencyOverrideRule {
        package: "lodash".to_string(),
        source: Some("package.json".to_string()),
    }];
    let config = config_for_fixture(root, ignore);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let any_lodash = results
        .unused_dependency_overrides
        .iter()
        .any(|f| f.entry.target_package == "lodash");
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
    .resolve(root, OutputFormat::Human, 4, true, true, None);
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

    let config = FallowConfig::default().resolve(
        root.to_path_buf(),
        OutputFormat::Human,
        4,
        true,
        true,
        None,
    );
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    assert_eq!(results.unused_dependency_overrides.len(), 2);
    for finding in &results.unused_dependency_overrides {
        assert!(
            finding.entry.hint.is_some(),
            "every unused override (bare-target or parent-chain) should carry the transitive hint; missing on {:?}",
            finding.entry.raw_key
        );
    }
}
