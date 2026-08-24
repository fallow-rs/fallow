use crate::common::{create_config, fixture_path};

#[test]
fn dead_code_verdicts_cover_merge_and_unreachable_reference_shapes() {
    let root = fixture_path("issue-2390-trace-consistency");
    let results = fallow_core::analyze(&create_config(root)).expect("analysis should succeed");
    let unused: Vec<_> = results
        .unused_exports
        .iter()
        .map(|finding| {
            (
                finding.export.path.to_string_lossy().replace('\\', "/"),
                finding.export.export_name.as_str(),
            )
        })
        .collect();

    assert!(
        !unused
            .iter()
            .any(|(path, name)| path.ends_with("source.ts") && *name == "Foo"),
        "the type import credits both halves of the declaration merge: {unused:?}"
    );
    assert!(
        unused
            .iter()
            .any(|(path, name)| path.ends_with("lonely.ts") && *name == "helper"),
        "a reference from an unreachable file cannot credit the export: {unused:?}"
    );
}
