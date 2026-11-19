//! Regression tests for issue #4731: with `baseUrl`, a bare specifier whose
//! first segment is a directory under `baseUrl` is a project path. When the
//! file is missing (for example, a code generator did not run yet), the import
//! is an unresolved import, not an unlisted package.

use std::path::Path;

use super::common::create_config;

fn write(root: &Path, path: &str, source: &str) {
    let path = root.join(path);
    std::fs::create_dir_all(path.parent().expect("parent")).expect("directory");
    std::fs::write(path, source).expect("fixture file");
}

struct Findings {
    unresolved: Vec<String>,
    unlisted: Vec<String>,
}

fn analyze(root: &Path) -> Findings {
    let mut config = create_config(root.to_path_buf());
    config.entry_patterns = vec!["src/main.ts".to_string()];
    let results = fallow_core::analyze(&config).expect("analysis");
    Findings {
        unresolved: results
            .unresolved_imports
            .iter()
            .map(|finding| finding.import.specifier.clone())
            .collect(),
        unlisted: results
            .unlisted_dependencies
            .iter()
            .map(|finding| finding.dep.package_name.clone())
            .collect(),
    }
}

fn project(package_json: &str, main: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("project");
    let root = dir.path();
    write(root, "package.json", package_json);
    write(
        root,
        "tsconfig.json",
        r#"{ "compilerOptions": { "baseUrl": "./", "module": "commonjs" } }"#,
    );
    write(root, "src/exists.ts", "export const a = 1\n");
    write(root, "src/main.ts", main);
    dir
}

const PACKAGE_JSON: &str = r#"{ "name": "r", "private": true, "version": "1.0.0" }"#;

#[test]
fn missing_file_under_base_url_directory_is_unresolved() {
    let dir = project(
        PACKAGE_JSON,
        "import { a } from 'src/exists'\nimport { b } from 'src/generated/client'\nconsole.log(a, b)\n",
    );
    let findings = analyze(dir.path());
    assert!(findings.unlisted.is_empty(), "{:?}", findings.unlisted);
    assert_eq!(
        findings.unresolved,
        vec!["src/generated/client".to_string()]
    );
}

#[test]
fn bare_specifier_without_base_url_directory_stays_unlisted() {
    let dir = project(
        PACKAGE_JSON,
        "import { a } from 'src/exists'\nimport b from 'left-pad/index'\nconsole.log(a, b)\n",
    );
    let findings = analyze(dir.path());
    assert!(findings.unresolved.is_empty(), "{:?}", findings.unresolved);
    assert_eq!(findings.unlisted, vec!["left-pad".to_string()]);
}

#[test]
fn declared_package_that_shares_a_base_url_directory_name_stays_credited() {
    let dir = project(
        r#"{ "name": "r", "private": true, "version": "1.0.0", "dependencies": { "src": "1.0.0" } }"#,
        "import { a } from 'src/exists'\nimport { b } from 'src/generated/client'\nconsole.log(a, b)\n",
    );
    let findings = analyze(dir.path());
    assert!(findings.unresolved.is_empty(), "{:?}", findings.unresolved);
    assert!(findings.unlisted.is_empty(), "{:?}", findings.unlisted);
}
