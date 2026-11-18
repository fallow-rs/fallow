//! Regression tests for issue #4734: TypeScript resolves an import of
//! `./x.ts` to `x.d.ts` when `x.ts` does not exist. A package that ships a
//! hand-written `schema.d.ts` next to `schema.js` is imported this way from
//! its tests.

use std::path::Path;

use super::common::create_config;

fn write(root: &Path, path: &str, source: &str) {
    let path = root.join(path);
    std::fs::create_dir_all(path.parent().expect("parent")).expect("directory");
    std::fs::write(path, source).expect("fixture file");
}

struct Findings {
    unresolved: Vec<String>,
    unused_files: Vec<String>,
}

fn analyze(root: &Path) -> Findings {
    let mut config = create_config(root.to_path_buf());
    config.entry_patterns = vec!["test/a.test.ts".to_string()];
    let results = fallow_core::analyze(&config).expect("analysis");
    Findings {
        unresolved: results
            .unresolved_imports
            .iter()
            .map(|finding| finding.import.specifier.clone())
            .collect(),
        unused_files: results
            .unused_files
            .iter()
            .map(|finding| {
                finding
                    .file
                    .path
                    .strip_prefix(root)
                    .unwrap_or(&finding.file.path)
                    .to_string_lossy()
                    .replace('\\', "/")
            })
            .collect(),
    }
}

fn project(extra: &[(&str, &str)]) -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("project");
    let root = dir.path();
    write(
        root,
        "package.json",
        r#"{"name":"repro-dts","private":true,"version":"1.0.0","main":"test/a.test.ts"}"#,
    );
    write(
        root,
        "schema.d.ts",
        "export interface Options { a: string }\n",
    );
    write(
        root,
        "test/a.test.ts",
        "import type { Options } from '../schema.ts'\n\nexport const t: Options = { a: 'y' }\n",
    );
    for (path, source) in extra {
        write(root, path, source);
    }
    dir
}

#[test]
fn ts_import_resolves_to_declaration_file_without_source() {
    let dir = project(&[]);
    let findings = analyze(dir.path());
    assert!(findings.unresolved.is_empty(), "{:?}", findings.unresolved);
    assert!(
        findings.unused_files.is_empty(),
        "the import must credit schema.d.ts: {:?}",
        findings.unused_files
    );
}

#[test]
fn ts_import_prefers_the_source_file_over_the_declaration_file() {
    let dir = project(&[("schema.ts", "export interface Options { a: string }\n")]);
    let findings = analyze(dir.path());
    assert!(findings.unresolved.is_empty(), "{:?}", findings.unresolved);
    assert!(
        !findings.unused_files.iter().any(|path| path == "schema.ts"),
        "the import must credit schema.ts: {:?}",
        findings.unused_files
    );
}
