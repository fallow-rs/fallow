//! An exported type that backs the signature of another export stays hidden
//! only while at least one backing export is live. When every backing export is
//! itself unused, the type is unused too and must be reported.

use super::common::{create_config, fixture_path};

fn findings(entries: &[(String, String)]) -> Vec<String> {
    let mut names: Vec<String> = entries
        .iter()
        .map(|(file, name)| format!("{file}:{name}"))
        .collect();
    names.sort();
    names
}

#[test]
fn signature_type_is_reported_when_every_backer_is_unused() {
    let root = fixture_path("signature-backing-unused-backer");
    let config = create_config(root.clone());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    let relative = |path: &std::path::Path| {
        path.strip_prefix(&root)
            .unwrap_or(path)
            .to_string_lossy()
            .replace('\\', "/")
    };

    let unused_exports = findings(
        &results
            .unused_exports
            .iter()
            .map(|f| (relative(&f.export.path), f.export.export_name.clone()))
            .collect::<Vec<_>>(),
    );
    let unused_types = findings(
        &results
            .unused_types
            .iter()
            .map(|f| (relative(&f.export.path), f.export.export_name.clone()))
            .collect::<Vec<_>>(),
    );

    assert_eq!(
        unused_exports,
        vec![
            "src/mixed.ts:unusedReader".to_owned(),
            "src/report.ts:summarize".to_owned(),
        ]
    );
    // `Summary` backs only `summarize`, which is unused. `TreeNode` backs only
    // itself. `Settings`, `Entry`, `Outer` and `Inner` each have a live backer,
    // directly or through a type that stays hidden.
    assert_eq!(
        unused_types,
        vec![
            "src/report.ts:Summary".to_owned(),
            "src/report.ts:TreeNode".to_owned(),
        ]
    );
}
