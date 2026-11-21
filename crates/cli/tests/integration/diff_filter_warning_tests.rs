//! The warning for a `--diff-file` with no added lines.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests and benches use unwrap and expect to keep fixture setup concise"
)]

use std::fs;

use crate::common::{parse_json, run_fallow_in_root};

/// A pure rename adds no line, but it touches the renamed file. A finding that
/// is anchored to the whole file (here an unused file) still passes the
/// filter, and a dependency finding bypasses it. The warning must say so.
#[test]
fn a_pure_rename_warns_that_file_and_dependency_findings_still_pass() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let root = tmp.path();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(
        root.join("package.json"),
        r#"{ "name": "rename-fixture", "main": "src/index.ts", "dependencies": { "left-pad": "1.0.0" } }"#,
    )
    .unwrap();
    fs::write(root.join("src/index.ts"), "export const used = 1;\n").unwrap();
    fs::write(root.join("src/orphan.ts"), "export const orphan = 1;\n").unwrap();
    fs::write(
        root.join("rename.diff"),
        "diff --git a/src/old.ts b/src/orphan.ts\n\
         similarity index 100%\n\
         rename from src/old.ts\n\
         rename to src/orphan.ts\n",
    )
    .unwrap();
    let diff = root.join("rename.diff");

    let output = run_fallow_in_root(
        "dead-code",
        root,
        &[
            "--no-cache",
            "--diff-file",
            diff.to_str().expect("utf-8 path"),
            "--format",
            "json",
        ],
    );

    let json = parse_json(&output);
    let unused: Vec<&str> = json["unused_files"]
        .as_array()
        .expect("unused_files array")
        .iter()
        .filter_map(|finding| finding["path"].as_str())
        .collect();
    assert_eq!(unused, vec!["src/orphan.ts"], "{json}");
    assert_eq!(
        json["unused_dependencies"].as_array().map_or(0, Vec::len),
        1,
        "{json}"
    );
    assert!(
        output.stderr.contains("parsed 0 added lines")
            && output.stderr.contains("Dependency findings")
            && output.stderr.contains("findings on a whole touched file"),
        "stderr: {}",
        output.stderr
    );
    assert!(
        !output.stderr.contains("no findings will pass"),
        "stderr: {}",
        output.stderr
    );
}
