//! Module resolution and project discovery in an audit base snapshot stop at
//! the snapshot root: the files in the directory above it do not count.

#![cfg(unix)]

use std::fs;
use std::path::Path;

use crate::common::{parse_json, run_fallow_in_root};

fn write(path: &Path, contents: &str) {
    fs::create_dir_all(path.parent().expect("parent")).expect("parent directory");
    fs::write(path, contents).expect("write file");
}

/// The snapshot root has no `package.json`. The directory above it has one.
/// A lookup that reads it would resolve the `#lib` import through its
/// `imports` map.
#[test]
fn snapshot_lookups_do_not_read_the_directory_above_the_snapshot_root() {
    let temp = tempfile::tempdir().expect("temp dir");
    let snapshots = dunce::canonicalize(temp.path())
        .expect("canonical temp dir")
        .join(fallow_types::audit_snapshot::snapshot_dir_name());
    let snapshot_name = "fallow-audit-base-cache-0000000000000000-root-0000000000000000";
    let root = snapshots.join(snapshot_name);
    fs::create_dir_all(root.join(".git")).expect("git directory");
    write(
        &root.join("src/index.ts"),
        "import { lib } from '#lib';\nexport const value = lib;\n",
    );
    write(&root.join("src/lib.ts"), "export const lib = 'x';\n");
    write(
        &snapshots.join("package.json"),
        &format!(
            "{{\"name\":\"above\",\"main\":\"{snapshot_name}/src/index.ts\",\
             \"imports\":{{\"#lib\":\"./{snapshot_name}/src/lib.ts\"}}}}\n"
        ),
    );

    let output = run_fallow_in_root(
        "dead-code",
        &root,
        &["--format", "json", "--quiet", "--no-cache"],
    );
    let json = parse_json(&output);
    let unresolved: Vec<&str> = json["unresolved_imports"]
        .as_array()
        .expect("unresolved imports")
        .iter()
        .filter_map(|item| item["specifier"].as_str())
        .collect();
    assert_eq!(unresolved, vec!["#lib"], "{}", output.stdout);
}
