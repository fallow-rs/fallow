//! Regression tests for issue #2794: a package listed only in the Module
//! Federation `shared` option was reported as an unused dependency.
//!
//! The Federation runtime loads a shared package on behalf of the remote
//! containers, so the host needs the package in `package.json` even when no
//! source file imports it.

use std::path::Path;

use super::common::create_config;

fn write(path: &Path, contents: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("create parent dir");
    }
    std::fs::write(path, contents).expect("write file");
}

/// Build a webpack project whose `package.json` declares `dependencies`, with
/// `files` (path, contents) added, and return the unused and unlisted package
/// names.
fn dependency_findings(dependencies: &str, files: &[(&str, &str)]) -> (Vec<String>, Vec<String>) {
    let dir = tempfile::tempdir().expect("temp dir");
    let root = dir.path();
    write(
        &root.join("package.json"),
        &format!(
            r#"{{
                "name": "mf-shared",
                "private": true,
                "main": "src/index.ts",
                "dependencies": {{ {dependencies} }},
                "devDependencies": {{ "webpack": "^5.98.0", "@module-federation/enhanced": "^0.9.0" }}
            }}"#
        ),
    );
    write(&root.join("src/index.ts"), "export const app = 1;\n");
    for (path, contents) in files {
        write(&root.join(path), contents);
    }
    let config = create_config(root.to_path_buf());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    let unused = results
        .unused_dependencies
        .iter()
        .map(|finding| finding.dep.package_name.clone())
        .collect();
    let unlisted = results
        .unlisted_dependencies
        .iter()
        .map(|finding| finding.dep.package_name.clone())
        .collect();
    (unused, unlisted)
}

const DEPENDENCIES: &str =
    r#""react": "^18.0.0", "react-dom": "^18.0.0", "@scope/ui": "^1.0.0", "lodash": "^4.0.0""#;

#[test]
fn object_form_shared_entries_credit_the_named_packages() {
    let (unused, _) = dependency_findings(
        DEPENDENCIES,
        &[(
            "webpack.config.js",
            r"
            const { ModuleFederationPlugin } = require('@module-federation/enhanced/webpack');
            module.exports = {
                plugins: [
                    new ModuleFederationPlugin({
                        name: 'app',
                        shared: {
                            react: { singleton: true },
                            'react-dom': '^18.0.0',
                            '@scope/ui/': { singleton: true },
                        },
                    }),
                ],
            };
            ",
        )],
    );
    for credited in ["react", "react-dom", "@scope/ui"] {
        assert!(
            !unused.iter().any(|name| name == credited),
            "{credited} is shared, got {unused:?}"
        );
    }
    assert!(
        unused.iter().any(|name| name == "lodash"),
        "a package that `shared` does not name still reports, got {unused:?}"
    );
}

#[test]
fn array_form_shared_entries_credit_the_named_packages() {
    let (unused, _) = dependency_findings(
        DEPENDENCIES,
        &[(
            "module-federation.config.ts",
            r"
            export default {
                name: 'app',
                shared: ['react', { 'react-dom': { singleton: true } }],
            };
            ",
        )],
    );
    for credited in ["react", "react-dom"] {
        assert!(
            !unused.iter().any(|name| name == credited),
            "{credited} is shared, got {unused:?}"
        );
    }
    assert!(
        unused.iter().any(|name| name == "lodash"),
        "a package that `shared` does not name still reports, got {unused:?}"
    );
}

#[test]
fn a_shared_entry_whose_import_names_another_package_credits_it() {
    let (unused, _) = dependency_findings(
        DEPENDENCIES,
        &[(
            "module-federation.config.ts",
            r"
            export default {
                name: 'app',
                shared: { 'my-react': { import: 'react', shareKey: 'react' } },
            };
            ",
        )],
    );
    assert!(
        !unused.iter().any(|name| name == "react"),
        "the `import` request of a shared entry is credited, got {unused:?}"
    );
}

#[test]
fn a_shared_entry_for_an_undeclared_package_creates_no_finding() {
    let (unused, unlisted) = dependency_findings(
        r#""react": "^18.0.0""#,
        &[(
            "module-federation.config.ts",
            r"
            export default {
                name: 'app',
                shared: { react: { singleton: true }, vue: { singleton: true } },
            };
            ",
        )],
    );
    assert!(unused.is_empty(), "got unused {unused:?}");
    assert!(unlisted.is_empty(), "got unlisted {unlisted:?}");
}

/// A `shared` entry credits the package that owns the config. A sibling
/// workspace that declares the same package and never uses it still reports.
#[test]
fn a_shared_entry_credits_only_the_package_that_owns_the_config() {
    let dir = tempfile::tempdir().expect("temp dir");
    let root = dir.path();
    write(
        &root.join("package.json"),
        r#"{ "name": "mono", "private": true, "workspaces": ["packages/*"] }"#,
    );
    for name in ["host", "other"] {
        write(
            &root.join(format!("packages/{name}/package.json")),
            &format!(
                r#"{{
                    "name": "{name}",
                    "main": "src/index.ts",
                    "dependencies": {{ "react": "^18.0.0" }},
                    "devDependencies": {{ "webpack": "^5.98.0" }}
                }}"#
            ),
        );
        write(
            &root.join(format!("packages/{name}/src/index.ts")),
            "export const app = 1;\n",
        );
    }
    write(
        &root.join("packages/host/webpack.config.js"),
        r"
        module.exports = {
            plugins: [new ModuleFederationPlugin({ name: 'host', shared: ['react'] })],
        };
        ",
    );
    let config = create_config(root.to_path_buf());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    let unused: Vec<String> = results
        .unused_dependencies
        .iter()
        .map(|finding| {
            format!(
                "{}:{}",
                finding.dep.path.to_string_lossy().replace('\\', "/"),
                finding.dep.package_name
            )
        })
        .collect();
    assert!(
        !unused
            .iter()
            .any(|finding| finding.ends_with("packages/host/package.json:react")),
        "the host shares react, got {unused:?}"
    );
    assert!(
        unused
            .iter()
            .any(|finding| finding.ends_with("packages/other/package.json:react")),
        "a sibling that does not share react still reports it, got {unused:?}"
    );
}
