#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests and benches use unwrap and expect to keep fixture setup concise"
)]

//! `fallow fix` plans the edits of each source fixer against the text that
//! the analysis read. A fixer that runs after another fixer of the same file
//! must not look up the line numbers of the analysis in the text that the
//! first fixer changed.

use std::path::Path;

use crate::common::{parse_json, run_fallow_in_root};

const LIB_SOURCE: &str = "const a = 1;\nconst b = 2;\nexport {\n  a,\n  b,\n};\nexport enum Status {\n  Active,\n  Unused,\n}\n";

fn write_project(root: &Path) {
    std::fs::write(
        root.join("package.json"),
        r#"{"name":"snapshot","main":"src/index.ts"}"#,
    )
    .unwrap();
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(
        root.join("src/index.ts"),
        "import { Status } from './lib';\nconsole.log(Status.Active);\n",
    )
    .unwrap();
    std::fs::write(root.join("src/lib.ts"), LIB_SOURCE).unwrap();
}

/// The `(type, line, name)` of each fix record, sorted, and whether each
/// record has `applied: true`.
fn fix_records(root: &Path, args: &[&str]) -> (Vec<(String, u64, String)>, bool) {
    let output = run_fallow_in_root("fix", root, args);
    assert_eq!(output.code, 0, "{}", output.stderr);
    let envelope = parse_json(&output);
    let fixes = envelope["fixes"].as_array().unwrap();
    let all_applied = fixes.iter().all(|fix| fix["applied"] == true);
    let mut records: Vec<(String, u64, String)> = fixes
        .iter()
        .map(|fix| {
            (
                fix["type"].as_str().unwrap().to_owned(),
                fix["line"].as_u64().unwrap(),
                fix["name"].as_str().unwrap().to_owned(),
            )
        })
        .collect();
    records.sort();
    (records, all_applied)
}

#[test]
fn fix_applies_the_enum_member_edit_below_a_removed_export_list() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write_project(root);
    let expected = vec![
        ("remove_enum_member".to_owned(), 9, "Unused".to_owned()),
        ("remove_export".to_owned(), 4, "a".to_owned()),
        ("remove_export".to_owned(), 5, "b".to_owned()),
    ];

    let (preview, _) = fix_records(root, &["--dry-run", "--format", "json", "--quiet"]);
    assert_eq!(preview, expected);
    assert_eq!(
        std::fs::read_to_string(root.join("src/lib.ts")).unwrap(),
        LIB_SOURCE
    );

    let (applied, all_applied) = fix_records(root, &["--yes", "--format", "json", "--quiet"]);
    assert_eq!(applied, expected);
    assert!(
        all_applied,
        "each record of the apply run has applied: true"
    );
    assert_eq!(
        std::fs::read_to_string(root.join("src/lib.ts")).unwrap(),
        "const a = 1;\nconst b = 2;\nexport enum Status {\n  Active,\n}\n"
    );
}
