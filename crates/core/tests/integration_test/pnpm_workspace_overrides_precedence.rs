//! pnpm 10 and earlier ignore the `overrides` section of `pnpm-workspace.yaml`
//! when the root `package.json` declares overrides.
//!
//! pnpm 10 merges `resolutions` and `pnpm.overrides` of the root
//! `package.json` into one map. When that map has at least one key, it
//! replaces the `overrides` of `pnpm-workspace.yaml` as a whole. An empty map
//! does not replace it. pnpm 11 and later do not read the `package.json`
//! sources. Without a known pnpm version, both sources stay active.

use std::fs;
use std::path::{Path, PathBuf};

use fallow_config::{FallowConfig, OutputFormat};
use fallow_types::results::{DependencyOverrideMisconfigReason, DependencyOverrideSource};
use rustc_hash::FxHashSet;

const IGNORED_DIAGNOSTIC: &str = "pnpm-workspace-overrides-ignored";
const PNPM_WORKSPACE_FILE: &str = "pnpm-workspace.yaml";

/// The `package.json` override fields of one test project.
struct Manifest<'a> {
    package_manager: Option<&'a str>,
    pnpm_overrides: Option<&'a str>,
    resolutions: Option<&'a str>,
}

const JSON_OVERRIDES: &str = r#"{ "stale-json-pkg": "^1.0.0", "legacy-pkg": "catalog:legacy" }"#;

/// The findings and diagnostics of one analysis run.
#[derive(Debug, Default)]
struct Outcome {
    unused_overrides: FxHashSet<(String, DependencyOverrideSource)>,
    misconfigured_overrides: FxHashSet<(String, DependencyOverrideMisconfigReason)>,
    unused_catalog_entries: FxHashSet<(String, String)>,
    unresolved_catalog_references: FxHashSet<(String, String, String)>,
    ignored_diagnostic_paths: Vec<PathBuf>,
}

fn write_project(root: &Path, manifest: &Manifest<'_>) {
    let mut fields = Vec::new();
    if let Some(value) = manifest.package_manager {
        fields.push(format!(r#""packageManager": "{value}""#));
    }
    if let Some(value) = manifest.pnpm_overrides {
        fields.push(format!(r#""pnpm": {{ "overrides": {value} }}"#));
    }
    if let Some(value) = manifest.resolutions {
        fields.push(format!(r#""resolutions": {value}"#));
    }
    // The catalog-valued override targets are declared, so only the
    // `stale-*` overrides can be unused.
    fields.push(
        r#""dependencies": { "react": "^18.2.0", "legacy-pkg": "^1.0.0", "yaml-only-pkg": "^1.0.0", "ghost-pkg": "^1.0.0" }"#
            .to_string(),
    );
    fs::write(
        root.join("package.json"),
        format!(
            "{{\n  \"name\": \"pnpm-overrides-precedence\",\n  \"private\": true,\n  {}\n}}\n",
            fields.join(",\n  ")
        ),
    )
    .expect("write package.json");
    fs::write(
        root.join(PNPM_WORKSPACE_FILE),
        "packages:\n  - .\n\ncatalog:\n  yaml-only-pkg: ^1.0.0\n\ncatalogs:\n  legacy:\n    legacy-pkg: ^1.0.0\n\noverrides:\n  stale-yaml-pkg: ^1.0.0\n  \"react@<18\": \"\"\n  yaml-only-pkg: \"catalog:\"\n  ghost-pkg: \"catalog:legacy\"\n",
    )
    .expect("write pnpm-workspace.yaml");
    fs::write(root.join("index.js"), "import 'react';\n").expect("write index.js");
}

fn analyze(manifest: &Manifest<'_>) -> Outcome {
    let tmp = tempfile::tempdir().expect("tempdir");
    write_project(tmp.path(), manifest);
    let config = FallowConfig::default().resolve(
        tmp.path().to_path_buf(),
        OutputFormat::Human,
        4,
        true,
        true,
        None,
    );
    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    let relative = |path: &Path| -> String {
        path.strip_prefix(&config.root)
            .unwrap_or(path)
            .to_string_lossy()
            .replace('\\', "/")
    };
    Outcome {
        unused_overrides: results
            .unused_dependency_overrides
            .iter()
            .map(|f| (f.entry.target_package.clone(), f.entry.source))
            .collect(),
        misconfigured_overrides: results
            .misconfigured_dependency_overrides
            .iter()
            .map(|f| (f.entry.raw_key.clone(), f.entry.reason))
            .collect(),
        unused_catalog_entries: results
            .unused_catalog_entries
            .iter()
            .map(|e| (e.entry.catalog_name.clone(), e.entry.entry_name.clone()))
            .collect(),
        unresolved_catalog_references: results
            .unresolved_catalog_references
            .iter()
            .map(|r| {
                (
                    r.reference.catalog_name.clone(),
                    r.reference.entry_name.clone(),
                    relative(&r.reference.path),
                )
            })
            .collect(),
        ignored_diagnostic_paths: fallow_config::workspace_diagnostics_for(&config.root)
            .into_iter()
            .filter(|diagnostic| diagnostic.kind.id() == IGNORED_DIAGNOSTIC)
            .map(|diagnostic| diagnostic.path)
            .collect(),
    }
}

fn unused(
    items: &[(&str, DependencyOverrideSource)],
) -> FxHashSet<(String, DependencyOverrideSource)> {
    items
        .iter()
        .map(|(name, source)| ((*name).to_string(), *source))
        .collect()
}

fn pairs(items: &[(&str, &str)]) -> FxHashSet<(String, String)> {
    items
        .iter()
        .map(|(a, b)| ((*a).to_string(), (*b).to_string()))
        .collect()
}

fn yaml_misconfigured() -> FxHashSet<(String, DependencyOverrideMisconfigReason)> {
    std::iter::once((
        "react@<18".to_string(),
        DependencyOverrideMisconfigReason::EmptyValue,
    ))
    .collect()
}

fn yaml_unresolved() -> FxHashSet<(String, String, String)> {
    std::iter::once((
        "legacy".to_string(),
        "ghost-pkg".to_string(),
        PNPM_WORKSPACE_FILE.to_string(),
    ))
    .collect()
}

#[test]
fn pnpm_10_ignores_workspace_overrides_when_package_json_has_overrides() {
    let outcome = analyze(&Manifest {
        package_manager: Some("pnpm@10.34.5"),
        pnpm_overrides: Some(JSON_OVERRIDES),
        resolutions: None,
    });

    assert_eq!(
        outcome.unused_overrides,
        unused(&[("stale-json-pkg", DependencyOverrideSource::PnpmPackageJson)]),
        "only the package.json overrides apply on pnpm 10",
    );
    assert!(
        outcome.misconfigured_overrides.is_empty(),
        "pnpm 10 ignores the yaml entry with an empty value; got {:?}",
        outcome.misconfigured_overrides,
    );
    assert_eq!(
        outcome.unused_catalog_entries,
        pairs(&[("default", "yaml-only-pkg")]),
        "an ignored yaml override is not a catalog consumer",
    );
    assert!(
        outcome.unresolved_catalog_references.is_empty(),
        "an ignored yaml override cannot fail the install; got {:?}",
        outcome.unresolved_catalog_references,
    );
    assert_eq!(
        outcome.ignored_diagnostic_paths.len(),
        1,
        "expected one diagnostic, got {:?}",
        outcome.ignored_diagnostic_paths,
    );
    assert!(
        outcome.ignored_diagnostic_paths[0].ends_with(PNPM_WORKSPACE_FILE),
        "the diagnostic points at pnpm-workspace.yaml; got {:?}",
        outcome.ignored_diagnostic_paths,
    );
}

#[test]
fn pnpm_10_ignores_workspace_overrides_when_package_json_has_resolutions() {
    let outcome = analyze(&Manifest {
        package_manager: Some("pnpm@10.34.5"),
        pnpm_overrides: None,
        resolutions: Some(r#"{ "legacy-pkg": "catalog:legacy" }"#),
    });

    assert!(
        outcome.unused_overrides.is_empty() && outcome.misconfigured_overrides.is_empty(),
        "pnpm 10 ignores every yaml override; got {:?} and {:?}",
        outcome.unused_overrides,
        outcome.misconfigured_overrides,
    );
    assert_eq!(
        outcome.unused_catalog_entries,
        pairs(&[("default", "yaml-only-pkg")])
    );
    assert!(outcome.unresolved_catalog_references.is_empty());
    assert_eq!(outcome.ignored_diagnostic_paths.len(), 1);
}

#[test]
fn pnpm_10_reads_workspace_overrides_when_package_json_overrides_are_empty() {
    let outcome = analyze(&Manifest {
        package_manager: Some("pnpm@10.34.5"),
        pnpm_overrides: Some("{}"),
        resolutions: Some("{}"),
    });

    assert_eq!(
        outcome.unused_overrides,
        unused(&[(
            "stale-yaml-pkg",
            DependencyOverrideSource::PnpmWorkspaceYaml
        )]),
    );
    assert_eq!(outcome.misconfigured_overrides, yaml_misconfigured());
    assert_eq!(
        outcome.unused_catalog_entries,
        pairs(&[("legacy", "legacy-pkg")])
    );
    assert_eq!(outcome.unresolved_catalog_references, yaml_unresolved());
    assert!(outcome.ignored_diagnostic_paths.is_empty());
}

#[test]
fn unknown_pnpm_version_keeps_both_override_sources() {
    let outcome = analyze(&Manifest {
        package_manager: None,
        pnpm_overrides: Some(JSON_OVERRIDES),
        resolutions: None,
    });

    assert_eq!(
        outcome.unused_overrides,
        unused(&[
            (
                "stale-yaml-pkg",
                DependencyOverrideSource::PnpmWorkspaceYaml
            ),
            ("stale-json-pkg", DependencyOverrideSource::PnpmPackageJson),
        ]),
    );
    assert_eq!(outcome.misconfigured_overrides, yaml_misconfigured());
    assert!(outcome.unused_catalog_entries.is_empty());
    assert_eq!(outcome.unresolved_catalog_references, yaml_unresolved());
    assert!(outcome.ignored_diagnostic_paths.is_empty());
}

#[test]
fn pnpm_11_keeps_workspace_overrides() {
    let outcome = analyze(&Manifest {
        package_manager: Some("pnpm@11.25.0"),
        pnpm_overrides: Some(JSON_OVERRIDES),
        resolutions: None,
    });

    assert_eq!(
        outcome.unused_overrides,
        unused(&[(
            "stale-yaml-pkg",
            DependencyOverrideSource::PnpmWorkspaceYaml
        )]),
    );
    assert_eq!(outcome.misconfigured_overrides, yaml_misconfigured());
    assert_eq!(
        outcome.unused_catalog_entries,
        pairs(&[("legacy", "legacy-pkg")])
    );
    assert_eq!(outcome.unresolved_catalog_references, yaml_unresolved());
    assert!(outcome.ignored_diagnostic_paths.is_empty());
}
