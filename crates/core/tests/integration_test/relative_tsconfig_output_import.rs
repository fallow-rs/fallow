use std::path::Path;

use super::common::create_config;

/// A package whose TypeScript config emits `src/` into `lib/types/`. The
/// output directory is not on disk, and a script imports the emitted files
/// through relative paths.
fn write_package(root: &Path) {
    for dir in ["src", "tools"] {
        std::fs::create_dir_all(root.join(dir)).expect("package directory");
    }
    std::fs::write(
        root.join("package.json"),
        r#"{
            "name": "relative-output-package",
            "private": true,
            "type": "module",
            "main": "./tools/check.mjs"
        }"#,
    )
    .expect("package manifest");
    std::fs::write(
        root.join("tsconfig.host.json"),
        r#"{
            "compilerOptions": {
                "rootDir": "src",
                "outDir": "lib/types"
            }
        }"#,
    )
    .expect("TypeScript config");
    std::fs::write(root.join("src/helper.ts"), "export const helper = 1;\n")
        .expect("source helper");
    std::fs::write(root.join("src/module.mts"), "export const module = 1;\n")
        .expect("source module");
    std::fs::write(root.join("src/internal.ts"), "export const internal = 1;\n")
        .expect("unrelated source file");
    std::fs::write(
        root.join("tools/check.mjs"),
        "import { helper } from '../lib/types/helper.js';\n\
         import { module } from '../lib/types/module.mjs';\n\
         import { missing } from '../lib/types/missing.js';\n\
         console.log(helper, module, missing);\n",
    )
    .expect("script entry");
}

fn relative(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

#[test]
fn relative_imports_into_tsconfig_out_dir_resolve_to_source_files() {
    let directory = tempfile::tempdir().expect("temporary project directory");
    let root = directory.path();
    write_package(root);

    let results =
        fallow_core::analyze(&create_config(root.to_path_buf())).expect("analysis should succeed");

    let unresolved: Vec<String> = results
        .unresolved_imports
        .iter()
        .map(|finding| finding.import.specifier.clone())
        .collect();
    assert_eq!(
        unresolved,
        vec!["../lib/types/missing.js".to_string()],
        "only the output path without a source file stays unresolved"
    );

    let unused: Vec<String> = results
        .unused_files
        .iter()
        .map(|unused| relative(root, &unused.file.path))
        .collect();
    assert_eq!(
        unused,
        vec!["src/internal.ts".to_string()],
        "the mapped source files are reachable, the unrelated source file is not"
    );
}
