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

/// A stale leftover `bun.lockb` next to a parseable pnpm lockfile (bun repo
/// migrated to pnpm, or a one-off `bun install` in a pnpm repo) must not
/// disable the analysis: pnpm-lock.yaml is complete resolution ground truth.
#[test]
fn bun_lockb_alongside_pnpm_lockfile_keeps_analysis_running() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path();
    fs::write(
        root.join("package.json"),
        r#"{
  "name": "issue-2341-bun-lockb-leftover",
  "private": true,
  "version": "0.0.0",
  "devDependencies": { "happy-dom": "^20.10.6" },
  "overrides": { "ws": "^8.21.0", "left-pad": "^1.3.0" }
}"#,
    )
    .expect("write root package.json");
    fs::write(
        root.join("pnpm-lock.yaml"),
        r"lockfileVersion: '9.0'

packages:
  happy-dom@20.11.6:
    resolution: {integrity: sha512-Hl}
  ws@8.21.3:
    resolution: {integrity: sha512-20}

snapshots:
  happy-dom@20.11.6:
    dependencies:
      ws: 8.21.3
  ws@8.21.3: {}
",
    )
    .expect("write pnpm lockfile");
    fs::write(root.join("bun.lockb"), b"\x00binary lockfile\x01\x02").expect("write bun.lockb");

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
        "pnpm-lock.yaml resolves ws; the stale bun.lockb must not suppress the whole check"
    );
}

/// npm renames `package-lock.json` to `npm-shrinkwrap.json` for publishable
/// packages; the format is identical, so shrinkwrap repos must get the same
/// transitive-resolution crediting instead of declaration-only degradation.
#[test]
fn transitive_only_targets_in_npm_shrinkwrap_are_used() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path();
    fs::write(
        root.join("package.json"),
        r#"{
  "name": "shrinkwrap-overrides",
  "private": true,
  "version": "0.0.0",
  "overrides": { "postcss": ">=8.5.10", "left-pad": "^1.3.0" }
}"#,
    )
    .expect("write root package.json");
    fs::write(
        root.join("npm-shrinkwrap.json"),
        r#"{
  "name": "shrinkwrap-overrides",
  "version": "0.0.0",
  "lockfileVersion": 3,
  "packages": {
    "": { "name": "shrinkwrap-overrides", "version": "0.0.0" },
    "node_modules/postcss": { "version": "8.5.10" }
  }
}"#,
    )
    .expect("write npm shrinkwrap");

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
        "postcss resolves in npm-shrinkwrap.json; only left-pad is unused"
    );
    let hint = results.unused_dependency_overrides[0]
        .entry
        .hint
        .as_deref()
        .expect("unused override carries a hint");
    assert!(
        hint.contains("npm ci"),
        "hint should name npm in a shrinkwrap repo; got {hint:?}"
    );
}

/// A malformed text `bun.lock` deliberately degrades to declaration-only
/// analysis (matching the pnpm/npm malformed-lockfile philosophy), so the
/// transitive-only override is flagged again, with the bun-flavored hint.
#[test]
fn malformed_bun_lock_degrades_to_declaration_only_with_bun_hint() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path();
    write_bun_repro_package_json(root, r#"{ "ws": "^8.21.0" }"#);
    fs::write(root.join("bun.lock"), "not json {{{").expect("write bun.lock");

    let config = config_for_fixture(root.to_path_buf(), vec![]);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let flagged: Vec<&str> = results
        .unused_dependency_overrides
        .iter()
        .map(|f| f.entry.target_package.as_str())
        .collect();
    assert_eq!(
        flagged,
        vec!["ws"],
        "a malformed bun.lock degrades to declaration-only analysis"
    );
    let hint = results.unused_dependency_overrides[0]
        .entry
        .hint
        .as_deref()
        .expect("unused override carries a hint");
    assert!(
        hint.contains("bun install"),
        "hint should name bun for a bun.lock repo; got {hint:?}"
    );
}

/// The `packageManager` field alone (lockfile not committed yet) must pick
/// the bun hint instead of falling back to the pnpm wording from the issue's
/// closing paragraph.
#[test]
fn package_manager_field_selects_bun_hint_without_lockfile() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path();
    write_bun_repro_package_json(root, r#"{ "left-pad": "^1.3.0" }"#);

    let config = config_for_fixture(root.to_path_buf(), vec![]);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let hint = results
        .unused_dependency_overrides
        .first()
        .and_then(|f| f.entry.hint.as_deref())
        .expect("declaration-only fallback still flags left-pad with a hint");
    assert!(
        hint.contains("bun install"),
        "packageManager: bun must select the bun hint even without a lockfile; got {hint:?}"
    );
}

/// yarn (classic and berry) resolves version pins through `resolutions` and
/// never applies the npm `overrides` object, so a yarn repo gets a hint that
/// explains the entry is inert instead of citing a pnpm command.
#[test]
fn yarn_repo_gets_resolutions_hint() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path();
    fs::write(
        root.join("package.json"),
        r#"{
  "name": "yarn-overrides",
  "private": true,
  "version": "0.0.0",
  "overrides": { "ws": "^8.21.0" }
}"#,
    )
    .expect("write root package.json");
    fs::write(root.join("yarn.lock"), "# yarn lockfile v1\n").expect("write yarn.lock");

    let config = config_for_fixture(root.to_path_buf(), vec![]);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let hint = results
        .unused_dependency_overrides
        .first()
        .and_then(|f| f.entry.hint.as_deref())
        .expect("inert overrides entry in a yarn repo is flagged with a hint");
    assert!(
        hint.contains("resolutions"),
        "yarn repos should be pointed at `resolutions`; got {hint:?}"
    );
}

/// bun workspace shape: the override target resolves only through a workspace
/// member's dependency, keyed in the shared top-level `packages` map.
#[test]
fn bun_workspace_member_transitive_targets_are_used() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path();
    fs::write(
        root.join("package.json"),
        r#"{
  "name": "bun-workspace-root",
  "private": true,
  "version": "0.0.0",
  "packageManager": "bun@1.3.2",
  "workspaces": ["packages/*"],
  "overrides": { "ws": "^8.21.0" }
}"#,
    )
    .expect("write root package.json");
    fs::create_dir_all(root.join("packages/app")).expect("member dir");
    fs::write(
        root.join("packages/app/package.json"),
        r#"{ "name": "@repro/app", "version": "0.0.0", "devDependencies": { "happy-dom": "^20.10.6" } }"#,
    )
    .expect("write member package.json");
    fs::write(
        root.join("bun.lock"),
        r#"{
  "lockfileVersion": 1,
  "workspaces": {
    "": {
      "name": "bun-workspace-root",
    },
    "packages/app": {
      "name": "@repro/app",
      "devDependencies": {
        "happy-dom": "^20.10.6",
      },
    },
  },
  "overrides": {
    "ws": "^8.21.0",
  },
  "packages": {
    "@repro/app": ["@repro/app@workspace:packages/app"],
    "happy-dom": ["happy-dom@20.11.6", "", { "dependencies": { "ws": "^8.21.0" } }, "sha512-Hl"],
    "ws": ["ws@8.21.3", "", {}, "sha512-20"],
  }
}
"#,
    )
    .expect("write bun.lock");

    let config = config_for_fixture(root.to_path_buf(), vec![]);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    assert!(
        results.unused_dependency_overrides.is_empty(),
        "ws resolves in bun.lock via the workspace member's dependency; flagged: {:?}",
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
