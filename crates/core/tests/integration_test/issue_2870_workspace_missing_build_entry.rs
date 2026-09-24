//! Issue #2870: a workspace package sets `main` and `types` to a build folder
//! that is not in the checkout, and the file behind the import specifier is a
//! build output too. A local file re-exports a name through that package, and
//! an entry file imports the name from the local file.
//!
//! Fallow cannot see the declaration, so the import stays an unresolved import.
//! The local re-export is still used by the entry file, so it must not also be
//! an unused export. A re-exported name that no file imports stays unused.

use std::path::Path;

use super::common::create_config;

fn write(root: &Path, relative: &str, contents: &str) {
    let path = root.join(relative);
    std::fs::create_dir_all(path.parent().expect("parent")).expect("create dir");
    std::fs::write(path, contents).expect("write file");
}

fn workspace(root: &Path) {
    write(
        root,
        "package.json",
        r#"{ "name": "root", "private": true, "workspaces": ["packages/*", "apps/*"] }"#,
    );
    write(
        root,
        "packages/icons/package.json",
        r#"{ "name": "@scope/icons", "private": true, "types": "dist/index.d.ts", "main": "dist/index.js" }"#,
    );
    write(
        root,
        "packages/icons/scripts/build.ts",
        "export const build = (): string => 'export const iconsList = {}';\nbuild();\n",
    );
    write(
        root,
        "apps/docs/package.json",
        r#"{ "name": "docs", "private": true, "main": "main.ts", "dependencies": { "@scope/icons": "workspace:*" } }"#,
    );
    write(
        root,
        "apps/docs/utils/icons-list.ts",
        "export { iconsList, unusedIcons } from '@scope/icons/index'\n",
    );
    write(
        root,
        "apps/docs/main.ts",
        "import { iconsList } from './utils/icons-list'\nconsole.log(iconsList)\n",
    );
}

fn build_output(root: &Path) {
    write(
        root,
        "packages/icons/index.ts",
        "export const iconsList = {};\nexport const unusedIcons = {};\n",
    );
    write(
        root,
        "packages/icons/dist/index.js",
        "exports.iconsList = {};\nexports.unusedIcons = {};\n",
    );
    write(
        root,
        "packages/icons/dist/index.d.ts",
        "export declare const iconsList: {};\nexport declare const unusedIcons: {};\n",
    );
}

struct Findings {
    unused_exports: Vec<String>,
    unresolved_imports: Vec<String>,
}

fn analyze(root: &Path) -> Findings {
    let config = create_config(root.to_path_buf());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    let relative = |path: &Path| {
        path.strip_prefix(&config.root)
            .unwrap_or(path)
            .to_string_lossy()
            .replace('\\', "/")
    };
    let mut unused_exports: Vec<String> = results
        .unused_exports
        .iter()
        .map(|finding| {
            format!(
                "{}:{}",
                relative(&finding.export.path),
                finding.export.export_name
            )
        })
        .collect();
    unused_exports.sort();
    let mut unresolved_imports: Vec<String> = results
        .unresolved_imports
        .iter()
        .map(|finding| {
            format!(
                "{}:{}",
                relative(&finding.import.path),
                finding.import.specifier
            )
        })
        .collect();
    unresolved_imports.sort();
    Findings {
        unused_exports,
        unresolved_imports,
    }
}

#[test]
fn a_re_export_through_a_missing_build_entry_is_credited_to_its_consumer() {
    let dir = tempfile::tempdir().expect("temp dir");
    workspace(dir.path());

    let findings = analyze(dir.path());

    assert_eq!(
        findings.unresolved_imports,
        vec!["apps/docs/utils/icons-list.ts:@scope/icons/index".to_string()],
        "the unresolved hop must stay visible"
    );
    assert!(
        !findings
            .unused_exports
            .contains(&"apps/docs/utils/icons-list.ts:iconsList".to_string()),
        "an imported re-export must not be an unused export, got: {:?}",
        findings.unused_exports
    );
    assert!(
        findings
            .unused_exports
            .contains(&"apps/docs/utils/icons-list.ts:unusedIcons".to_string()),
        "a re-export that no file imports must stay unused, got: {:?}",
        findings.unused_exports
    );
}

#[test]
fn a_checkout_with_the_build_output_resolves_through_the_package() {
    let dir = tempfile::tempdir().expect("temp dir");
    workspace(dir.path());
    build_output(dir.path());

    let findings = analyze(dir.path());

    assert!(
        findings.unresolved_imports.is_empty(),
        "the built package must resolve, got: {:?}",
        findings.unresolved_imports
    );
    assert!(
        !findings
            .unused_exports
            .iter()
            .any(|export| export.ends_with(":iconsList")),
        "iconsList is used through the package, got: {:?}",
        findings.unused_exports
    );
}
