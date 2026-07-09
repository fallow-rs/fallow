//! Runtime backstop for fallow's "static analysis never executes the analyzed
//! project's code" invariant. The compile-time guarantee is the
//! `#![cfg_attr(not(test), deny(clippy::disallowed_methods))]` ban on raw
//! `Command::new` in fallow-core/extract/graph; this test proves the behavioral
//! consequence end-to-end: a
//! `package.json` lifecycle script that would write a sentinel file never runs,
//! because fallow reads `package.json` as data and never invokes a package
//! manager.

use super::common::create_config;
use std::fs;

#[test]
fn analysis_never_runs_package_json_lifecycle_scripts() {
    let dir = tempfile::tempdir().expect("create tempdir");
    let root = dir.path();
    let sentinel = root.join("LIFECYCLE_SCRIPT_RAN");

    fs::write(
        root.join("package.json"),
        r#"{
  "name": "sentinel-fixture",
  "version": "1.0.0",
  "main": "index.ts",
  "dependencies": { "left-pad": "^1.0.0" },
  "scripts": {
    "preinstall": "node -e \"require('fs').writeFileSync('LIFECYCLE_SCRIPT_RAN','')\"",
    "postinstall": "node -e \"require('fs').writeFileSync('LIFECYCLE_SCRIPT_RAN','')\"",
    "prepare": "node -e \"require('fs').writeFileSync('LIFECYCLE_SCRIPT_RAN','')\""
  }
}"#,
    )
    .expect("write package.json");

    fs::write(
        root.join("index.ts"),
        "import { used } from './used';\nconsole.log(used);\n",
    )
    .expect("write index.ts");
    fs::write(root.join("used.ts"), "export const used = 1;\n").expect("write used.ts");
    fs::write(root.join("orphan.ts"), "export const orphan = 2;\n").expect("write orphan.ts");

    let config = create_config(root.to_path_buf());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    assert!(
        !sentinel.exists(),
        "fallow executed a package.json lifecycle script during analysis: the sentinel \
         file was created. Static analysis must never run the analyzed project's code.",
    );

    let unused_files: Vec<String> = results
        .unused_files
        .iter()
        .map(|f| {
            f.file
                .path
                .file_name()
                .unwrap()
                .to_string_lossy()
                .to_string()
        })
        .collect();
    assert!(
        unused_files.iter().any(|name| name == "orphan.ts"),
        "expected analysis to flag orphan.ts as unused, found: {unused_files:?}",
    );
    assert!(
        !unused_files.iter().any(|name| name == "used.ts"),
        "expected used.ts to be reachable from the index.ts entry point (proving the \
         import graph was traversed), but it was flagged unused: {unused_files:?}",
    );
}
