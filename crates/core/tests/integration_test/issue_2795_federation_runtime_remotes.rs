//! Regression tests for issue #2795: a remote that the Module Federation
//! runtime API registers or loads was invisible to analysis.
//!
//! `registerRemotes([{ name: 'checkout', entry }])` and
//! `loadRemote('checkout/Button')` make `checkout` a remote container at
//! runtime, the same as a `remotes` config entry. Without reading the calls, a
//! static `import('checkout/Widget')` in the same package surfaced as an
//! unlisted dependency named `checkout`. Only a call with a static literal
//! argument is read, and only in a file that imports a Federation runtime
//! package. A call with any other argument records a
//! `plugin-config-unreadable` diagnostic on the file.

use std::path::Path;

use fallow_config::WorkspaceDiagnosticKind;

use super::common::create_config;

fn write(path: &Path, contents: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("create parent dir");
    }
    std::fs::write(path, contents).expect("write file");
}

struct Outcome {
    unlisted: Vec<String>,
    /// `(file name, key, reason)` of each `plugin-config-unreadable` entry.
    unreadable: Vec<(String, String, String)>,
}

/// Build a project from `files` (path, contents) and return the unlisted
/// package names and the `plugin-config-unreadable` entries.
fn analyze(files: &[(&str, &str)]) -> Outcome {
    let dir = tempfile::tempdir().expect("temp dir");
    let root = dir.path();
    for (path, contents) in files {
        write(&root.join(path), contents);
    }
    let config = create_config(root.to_path_buf());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    let unlisted = results
        .unlisted_dependencies
        .iter()
        .map(|finding| {
            let site = finding
                .dep
                .imported_from
                .first()
                .map(|site| site.path.to_string_lossy().replace('\\', "/"))
                .unwrap_or_default();
            let package = site
                .rsplit_once("/src/")
                .map_or("", |(before, _)| before)
                .rsplit('/')
                .next()
                .unwrap_or_default()
                .to_string();
            format!("{package}:{}", finding.dep.package_name)
        })
        .collect();
    let unreadable = fallow_config::workspace_diagnostics_for(&config.root)
        .into_iter()
        .filter_map(|diagnostic| match diagnostic.kind {
            WorkspaceDiagnosticKind::PluginConfigUnreadable { key, reason, .. } => Some((
                diagnostic
                    .path
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_default(),
                key,
                reason,
            )),
            _ => None,
        })
        .collect();
    Outcome {
        unlisted,
        unreadable,
    }
}

const ROOT_PACKAGE: &str = r#"{ "name": "mono", "private": true, "workspaces": ["packages/*"] }"#;

fn package(name: &str) -> String {
    format!(
        r#"{{ "name": "{name}", "main": "src/index.ts", "dependencies": {{ "@module-federation/enhanced": "^0.9.0", "@module-federation/runtime": "^0.9.0" }} }}"#
    )
}

#[test]
fn a_literal_register_remotes_call_provides_the_remote_in_its_package() {
    let host = package("host");
    let other = package("other");
    let outcome = analyze(&[
        ("package.json", ROOT_PACKAGE),
        ("packages/host/package.json", &host),
        (
            "packages/host/src/index.ts",
            r"
            import { registerRemotes } from '@module-federation/enhanced/runtime';
            registerRemotes([{ name: 'checkout', entry: 'https://example.test/mf.js' }]);
            export const widget = () => import('checkout/Widget');
            ",
        ),
        ("packages/other/package.json", &other),
        (
            "packages/other/src/index.ts",
            "export const widget = () => import('checkout/Widget');\n",
        ),
    ]);
    assert!(
        !outcome.unlisted.contains(&"host:checkout".to_string()),
        "the host registers `checkout`, got {:?}",
        outcome.unlisted
    );
    assert!(
        outcome.unlisted.contains(&"other:checkout".to_string()),
        "a sibling package does not register `checkout`, got {:?}",
        outcome.unlisted
    );
    assert!(
        outcome.unreadable.is_empty(),
        "got {:?}",
        outcome.unreadable
    );
}

#[test]
fn a_literal_load_remote_call_provides_the_remote_alias() {
    let host = package("host");
    let outcome = analyze(&[
        ("package.json", ROOT_PACKAGE),
        ("packages/host/package.json", &host),
        (
            "packages/host/src/index.ts",
            r"
            import { loadRemote as load } from '@module-federation/runtime';
            export const button = () => load('checkout/Button');
            export const widget = () => import('checkout/Widget');
            ",
        ),
    ]);
    assert!(outcome.unlisted.is_empty(), "got {:?}", outcome.unlisted);
    assert!(
        outcome.unreadable.is_empty(),
        "got {:?}",
        outcome.unreadable
    );
}

#[test]
fn a_call_without_a_runtime_import_is_not_read() {
    let host = package("host");
    let outcome = analyze(&[
        ("package.json", ROOT_PACKAGE),
        ("packages/host/package.json", &host),
        (
            "packages/host/src/index.ts",
            r"
            import { registerRemotes } from './local';
            registerRemotes([{ name: 'checkout', entry: 'https://example.test/mf.js' }]);
            registerRemotes(remotes);
            export const widget = () => import('checkout/Widget');
            ",
        ),
        (
            "packages/host/src/local.ts",
            "export const registerRemotes = (value: unknown): unknown => value;\n",
        ),
    ]);
    assert!(
        outcome.unlisted.contains(&"host:checkout".to_string()),
        "a local `registerRemotes` registers nothing, got {:?}",
        outcome.unlisted
    );
    assert!(
        outcome.unreadable.is_empty(),
        "got {:?}",
        outcome.unreadable
    );
}

#[test]
fn a_call_with_a_dynamic_argument_is_recorded() {
    let host = package("host");
    let outcome = analyze(&[
        ("package.json", ROOT_PACKAGE),
        ("packages/host/package.json", &host),
        (
            "packages/host/src/index.ts",
            r"
            import * as runtime from '@module-federation/runtime';
            const remotes = [{ name: 'checkout', entry: 'https://example.test/mf.js' }];
            runtime.registerRemotes(remotes);
            export const load = (scope: string) => runtime.loadRemote(`${scope}/Button`);
            export const widget = () => import('checkout/Widget');
            ",
        ),
    ]);
    assert!(
        outcome.unlisted.contains(&"host:checkout".to_string()),
        "a dynamic argument registers nothing, got {:?}",
        outcome.unlisted
    );
    let mut unreadable = outcome.unreadable;
    unreadable.sort();
    assert_eq!(
        unreadable,
        vec![
            (
                "index.ts".to_string(),
                "loadRemote".to_string(),
                "dynamic-argument".to_string()
            ),
            (
                "index.ts".to_string(),
                "registerRemotes".to_string(),
                "dynamic-argument".to_string()
            ),
        ]
    );
}
