//! The pnpm version selects the root `package.json` override source.
//!
//! pnpm 10 and earlier read `pnpm.overrides` in the root `package.json`.
//! pnpm 11 and later do not read the `pnpm` field and print a warning, so a
//! stale entry there has no effect on the install. The `overrides` section of
//! `pnpm-workspace.yaml` applies for pnpm 11 and later. pnpm 10 ignores it
//! when `pnpm.overrides` is not empty; see
//! `pnpm_workspace_overrides_precedence.rs`.

use std::fs;
use std::path::Path;

use fallow_config::{FallowConfig, OutputFormat};
use fallow_types::results::{DependencyOverrideMisconfigReason, DependencyOverrideSource};
use rustc_hash::FxHashSet;

type UnusedPairs = FxHashSet<(String, DependencyOverrideSource)>;
type MisconfiguredPairs = FxHashSet<(String, DependencyOverrideMisconfigReason)>;

/// Write a pnpm project with one stale entry in each override source and one
/// empty-value entry in `pnpm.overrides`.
fn write_project(root: &Path, package_manager: Option<&str>) {
    let package_manager_line = package_manager
        .map(|value| format!(r#"  "packageManager": "{value}","#))
        .unwrap_or_default();
    fs::write(
        root.join("package.json"),
        format!(
            r#"{{
  "name": "pnpm-overrides-version",
  "private": true,
{package_manager_line}
  "dependencies": {{ "react": "^18.2.0" }},
  "pnpm": {{
    "overrides": {{
      "stale-json-pkg": "^1.0.0",
      "react@<18": ""
    }}
  }}
}}
"#
        ),
    )
    .expect("write package.json");
    fs::write(
        root.join("pnpm-workspace.yaml"),
        "overrides:\n  stale-yaml-pkg: ^1.0.0\n",
    )
    .expect("write pnpm-workspace.yaml");
    fs::write(root.join("index.js"), "import 'react';\n").expect("write index.js");
}

fn analyze(package_manager: Option<&str>) -> (UnusedPairs, MisconfiguredPairs) {
    let tmp = tempfile::tempdir().expect("tempdir");
    write_project(tmp.path(), package_manager);
    let config = FallowConfig::default().resolve(
        tmp.path().to_path_buf(),
        OutputFormat::Human,
        4,
        true,
        true,
        None,
    );
    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    let unused = results
        .unused_dependency_overrides
        .iter()
        .map(|f| (f.entry.target_package.clone(), f.entry.source))
        .collect();
    let misconfigured = results
        .misconfigured_dependency_overrides
        .iter()
        .map(|f| (f.entry.raw_key.clone(), f.entry.reason))
        .collect();
    (unused, misconfigured)
}

fn yaml_finding_only() -> UnusedPairs {
    std::iter::once((
        "stale-yaml-pkg".to_string(),
        DependencyOverrideSource::PnpmWorkspaceYaml,
    ))
    .collect()
}

fn both_sources() -> (UnusedPairs, MisconfiguredPairs) {
    let mut unused = yaml_finding_only();
    unused.insert((
        "stale-json-pkg".to_string(),
        DependencyOverrideSource::PnpmPackageJson,
    ));
    let misconfigured = std::iter::once((
        "react@<18".to_string(),
        DependencyOverrideMisconfigReason::EmptyValue,
    ))
    .collect();
    (unused, misconfigured)
}

#[test]
fn pnpm_11_ignores_package_json_pnpm_overrides() {
    let (unused, misconfigured) = analyze(Some("pnpm@11.28.3"));
    assert_eq!(unused, yaml_finding_only());
    assert!(
        misconfigured.is_empty(),
        "pnpm 11 does not read `pnpm.overrides`; got {misconfigured:?}"
    );
}

#[test]
fn pnpm_12_with_integrity_suffix_ignores_package_json_pnpm_overrides() {
    let (unused, misconfigured) = analyze(Some("pnpm@12.8.1+sha512.0123abcd"));
    assert_eq!(unused, yaml_finding_only());
    assert!(misconfigured.is_empty(), "got {misconfigured:?}");
}

/// pnpm 10 reads `pnpm.overrides`. Because that map is not empty, pnpm 10
/// ignores the `pnpm-workspace.yaml` overrides, so the stale yaml entry gives
/// no finding.
#[test]
fn pnpm_10_reads_package_json_pnpm_overrides() {
    let (mut unused, misconfigured) = both_sources();
    unused.retain(|(_, source)| *source == DependencyOverrideSource::PnpmPackageJson);
    assert_eq!(analyze(Some("pnpm@10.34.5")), (unused, misconfigured));
}

#[test]
fn unknown_pnpm_version_keeps_reading_package_json_pnpm_overrides() {
    assert_eq!(analyze(None), both_sources());
}

#[test]
fn other_package_manager_keeps_reading_package_json_pnpm_overrides() {
    assert_eq!(analyze(Some("yarn@4.5.0")), both_sources());
}
