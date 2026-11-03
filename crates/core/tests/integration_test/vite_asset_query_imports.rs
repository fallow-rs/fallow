use super::common::create_config;
use std::path::Path;

const PACKAGE_JSON: &str = r#"{"name":"vite-asset-queries","private":true,"dependencies":{"plain-pkg":"1.0.0","@scope/mapped-pkg":"1.0.0"},"devDependencies":{"vite":"*"}}"#;

fn write(root: &Path, path: &str, source: &str) {
    let path = root.join(path);
    std::fs::create_dir_all(path.parent().expect("parent")).expect("directory");
    std::fs::write(path, source).expect("fixture file");
}

fn project(entry: &str, installed: bool) -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("project");
    let root = dir.path();
    write(root, "package.json", PACKAGE_JSON);
    write(root, "src/index.ts", entry);
    if installed {
        write(
            root,
            "node_modules/plain-pkg/package.json",
            r#"{"name":"plain-pkg","version":"1.0.0","main":"index.js"}"#,
        );
        write(
            root,
            "node_modules/plain-pkg/index.js",
            "module.exports = {};",
        );
        write(
            root,
            "node_modules/plain-pkg/build/worker.min.mjs",
            "self.onmessage = () => {};",
        );
        write(
            root,
            "node_modules/@scope/mapped-pkg/package.json",
            r#"{"name":"@scope/mapped-pkg","version":"1.0.0","exports":{".":"./dist/index.js","./dist/*":"./dist/*"}}"#,
        );
        write(
            root,
            "node_modules/@scope/mapped-pkg/dist/index.js",
            "export {};",
        );
        write(root, "node_modules/@scope/mapped-pkg/dist/engine.wasm", "");
        write(
            root,
            "node_modules/@scope/mapped-pkg/dist/worker.js",
            "export {};",
        );
    }
    dir
}

struct Findings {
    unresolved: Vec<String>,
    unused_dependencies: Vec<String>,
}

fn analyze(root: &Path) -> Findings {
    let mut config = create_config(root.to_path_buf());
    config.entry_patterns = vec!["src/index.ts".to_string()];
    let results = fallow_core::analyze(&config).expect("analysis");
    Findings {
        unresolved: results
            .unresolved_imports
            .iter()
            .map(|finding| finding.import.specifier.clone())
            .collect(),
        unused_dependencies: results
            .unused_dependencies
            .iter()
            .map(|finding| finding.dep.package_name.clone())
            .collect(),
    }
}

const QUERY_IMPORTS: &str = "\
import a from 'plain-pkg/build/worker.min.mjs?url';
import b from 'plain-pkg/build/worker.min.mjs?raw';
import c from 'plain-pkg/build/worker.min.mjs?worker&inline';
import d from '@scope/mapped-pkg/dist/engine.wasm?url';
import e from '@scope/mapped-pkg/dist/engine.wasm?init';
import f from '@scope/mapped-pkg/dist/worker.js?sharedworker';
import g from '@scope/mapped-pkg/dist/engine.wasm?url&no-inline';
console.log(a, b, c, d, e, f, g);
";

#[test]
fn query_imports_of_an_installed_package_resolve_and_credit_it() {
    let dir = project(QUERY_IMPORTS, true);
    let findings = analyze(dir.path());
    assert!(findings.unresolved.is_empty(), "{:?}", findings.unresolved);
    assert!(
        findings.unused_dependencies.is_empty(),
        "{:?}",
        findings.unused_dependencies
    );
}

#[test]
fn query_imports_of_an_uninstalled_package_credit_it() {
    let dir = project(QUERY_IMPORTS, false);
    let findings = analyze(dir.path());
    assert!(
        findings.unresolved.is_empty(),
        "an uninstalled package cannot prove the file is missing: {:?}",
        findings.unresolved
    );
    assert!(
        findings.unused_dependencies.is_empty(),
        "both packages must count as used: {:?}",
        findings.unused_dependencies
    );
}

#[test]
fn missing_file_of_an_installed_package_with_query_stays_unresolved() {
    let dir = project(
        "import a from 'plain-pkg/build/missing.mjs?url';\nimport b from '@scope/mapped-pkg/dist/missing.wasm?raw';\nconsole.log(a, b);\n",
        true,
    );
    assert_eq!(
        analyze(dir.path()).unresolved,
        vec![
            "plain-pkg/build/missing.mjs?url".to_string(),
            "@scope/mapped-pkg/dist/missing.wasm?raw".to_string(),
        ]
    );
}

#[test]
fn unknown_query_on_an_uninstalled_package_stays_unresolved() {
    let dir = project(
        "import a from 'plain-pkg/build/worker.min.mjs?custom';\nconsole.log(a);\n",
        false,
    );
    assert_eq!(
        analyze(dir.path()).unresolved,
        vec!["plain-pkg/build/worker.min.mjs?custom".to_string()]
    );
}
