//! A workspace package that looks published, but that `publicPackages` does
//! not list, gets a `public-package-candidate` diagnostic when its entry
//! publishes an export or a member that has a finding (issue #5335). The
//! diagnostic changes no finding.

use std::fs;
use std::path::Path;

use fallow_config::ResolvedConfig;

use super::common::{create_config, create_config_with_overrides};

const KIND: &str = "public-package-candidate";

fn write_file(root: &Path, relative: &str, contents: &str) {
    let path = root.join(relative);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("create parent dir");
    }
    fs::write(path, contents).expect("write fixture file");
}

/// One library package with an entry that publishes an enum with an unused
/// member. `manifest_extra` is spliced into its
/// `package.json`.
fn write_library(root: &Path, dir: &str, name: &str, manifest_extra: &str) {
    write_file(
        root,
        &format!("packages/{dir}/package.json"),
        &format!(r#"{{ "name": "{name}"{manifest_extra} }}"#),
    );
    write_file(
        root,
        &format!("packages/{dir}/src/index.ts"),
        "export * from './enums';\n",
    );
    write_file(
        root,
        &format!("packages/{dir}/src/enums.ts"),
        "export enum Mode { On = 'on', Off = 'off' }\n",
    );
}

fn write_monorepo(root: &Path) {
    write_file(
        root,
        "package.json",
        r#"{ "name": "mono", "private": true, "workspaces": ["packages/*", "apps/*"] }"#,
    );
    let exports = r#", "exports": { ".": "./src/index.ts" }"#;
    write_library(root, "sdk", "@x/sdk", exports);
    write_library(
        root,
        "priv",
        "@x/priv",
        &format!(r#", "private": true{exports}"#),
    );
    write_library(root, "listed", "@x/listed", exports);
    write_library(root, "bare", "@x/bare", "");

    // The entry of `@x/hidden` publishes only `api`, which the app uses. The
    // finding is on an export that the entry does not publish.
    write_file(
        root,
        "packages/hidden/package.json",
        r#"{ "name": "@x/hidden", "main": "./src/index.ts" }"#,
    );
    write_file(
        root,
        "packages/hidden/src/index.ts",
        "import { helper } from './internal';\nexport const api = helper;\n",
    );
    write_file(
        root,
        "packages/hidden/src/internal.ts",
        "export const helper = 1;\nexport const internalDead = 2;\n",
    );

    write_file(
        root,
        "apps/web/package.json",
        r#"{ "name": "web", "private": true, "main": "./src/main.ts", "dependencies": {
            "@x/sdk": "*", "@x/priv": "*", "@x/listed": "*", "@x/bare": "*", "@x/hidden": "*"
        } }"#,
    );
    write_file(
        root,
        "apps/web/src/main.ts",
        "import { Mode as A } from '@x/sdk';\n\
         import { Mode as B } from '@x/priv';\n\
         import { Mode as C } from '@x/listed';\n\
         import { Mode as D } from '@x/bare';\n\
         import { api } from '@x/hidden';\n\
         console.log(A.On, B.On, C.On, D.On, api);\n",
    );
}

fn analyze(
    root: &Path,
    public_packages: &[&str],
) -> (ResolvedConfig, fallow_core::results::AnalysisResults) {
    let mut config = create_config(root.to_path_buf());
    config.public_packages = public_packages.iter().map(ToString::to_string).collect();
    let results = fallow_core::analyze(&config).expect("analysis succeeds");
    (config, results)
}

/// The serialized entries of this kind that the registry holds for the root.
fn reported(config: &ResolvedConfig) -> Vec<serde_json::Value> {
    fallow_config::workspace_diagnostics_for(&config.root)
        .into_iter()
        .filter(|diagnostic| diagnostic.kind.id() == KIND)
        .map(|diagnostic| {
            let relative = diagnostic.into_root_relative(&config.root);
            serde_json::to_value(&relative).expect("diagnostic serializes")
        })
        .collect()
}

fn reported_packages(config: &ResolvedConfig) -> Vec<String> {
    reported(config)
        .iter()
        .map(|entry| entry["package"].as_str().unwrap_or_default().to_owned())
        .collect()
}

fn off_members(results: &fallow_core::results::AnalysisResults) -> usize {
    results
        .unused_enum_members
        .iter()
        .filter(|m| m.member.parent_name == "Mode" && m.member.member_name == "Off")
        .count()
}

#[test]
fn a_published_looking_package_with_entry_findings_gets_the_diagnostic() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let root = tmp.path().join("project");
    write_monorepo(&root);

    let (config, _) = analyze(&root, &["@x/listed"]);

    let entries = reported(&config);
    assert_eq!(
        reported_packages(&config),
        vec!["@x/sdk".to_owned()],
        "only the published-looking package with entry findings: {entries:?}"
    );
    let entry = &entries[0];
    assert_eq!(entry["path"], "packages/sdk");
    assert_eq!(entry["findings"], 1, "Mode.Off: {entry}");
    let message = entry["message"].as_str().expect("message is a string");
    assert!(
        message.contains("@x/sdk") && message.contains("publicPackages"),
        "the message names the package and the setting: {message}"
    );
    assert!(
        entry.get("degrades_analysis").is_none(),
        "a hint, not a degraded run: {entry}"
    );
}

#[test]
fn the_diagnostic_changes_no_finding() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let root = tmp.path().join("project");
    write_monorepo(&root);

    let (_, results) = analyze(&root, &["@x/listed"]);

    // `Mode.Off` stays reported in `@x/sdk`, `@x/priv` and `@x/bare`.
    assert_eq!(off_members(&results), 3);
    // `@x/hidden` has a finding, but not on the exports of its entry.
    assert!(
        results
            .unused_exports
            .iter()
            .any(|e| e.export.export_name == "internalDead"),
        "the finding off the entry surface is reported"
    );
}

#[test]
fn listing_the_package_in_public_packages_removes_the_diagnostic() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let root = tmp.path().join("project");
    write_monorepo(&root);

    let (config, _) = analyze(&root, &["@x/sdk", "@x/listed"]);

    assert!(reported(&config).is_empty());
}

#[test]
fn an_unlisted_package_that_was_listed_before_gets_the_diagnostic() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let root = tmp.path().join("project");
    write_monorepo(&root);

    let (config, _) = analyze(&root, &[]);

    assert_eq!(
        reported_packages(&config),
        vec!["@x/listed".to_owned(), "@x/sdk".to_owned()]
    );
}

/// A monorepo with one library package `@x/one` and an app that uses
/// `Mode.On`. `manifest_extra` is spliced into the library manifest.
fn write_one_library(root: &Path, manifest_extra: &str) {
    write_file(
        root,
        "package.json",
        r#"{ "name": "mono", "private": true, "workspaces": ["packages/*", "apps/*"] }"#,
    );
    write_library(root, "one", "@x/one", manifest_extra);
    write_file(
        root,
        "apps/web/package.json",
        r#"{ "name": "web", "private": true, "main": "./src/main.ts", "dependencies": { "@x/one": "*" } }"#,
    );
    write_file(
        root,
        "apps/web/src/main.ts",
        "import { Mode } from '@x/one';\nconsole.log(Mode.On);\n",
    );
}

#[test]
fn a_module_field_alone_makes_a_package_look_published() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let root = tmp.path().join("project");
    write_one_library(&root, r#", "module": "./src/index.ts""#);

    let (config, _) = analyze(&root, &[]);

    assert_eq!(reported_packages(&config), vec!["@x/one".to_owned()]);
}

#[test]
fn an_entry_file_that_does_not_exist_gives_no_diagnostic() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let root = tmp.path().join("project");
    // With `exports`, the `src/index.ts` fallback is not an entry.
    write_one_library(&root, r#", "exports": { ".": "./missing/index.js" }"#);

    let (config, _) = analyze(&root, &[]);

    assert!(reported(&config).is_empty());
}

#[test]
fn a_finding_that_an_override_turns_off_does_not_count() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let root = tmp.path().join("project");
    write_one_library(&root, r#", "exports": { ".": "./src/index.ts" }"#);

    let config = create_config_with_overrides(
        root,
        vec![(
            "packages/one/**",
            fallow_config::PartialRulesConfig {
                unused_enum_members: Some(fallow_config::Severity::Off),
                ..Default::default()
            },
        )],
    );
    let _ = fallow_core::analyze(&config).expect("analysis succeeds");

    assert!(reported(&config).is_empty());
}

#[test]
fn a_rerun_with_the_package_listed_drops_the_diagnostic() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let root = tmp.path().join("project");
    write_one_library(&root, r#", "exports": { ".": "./src/index.ts" }"#);

    let (config, _) = analyze(&root, &[]);
    assert_eq!(reported_packages(&config), vec!["@x/one".to_owned()]);

    let (config, _) = analyze(&root, &["@x/one"]);
    assert!(reported(&config).is_empty());
}
