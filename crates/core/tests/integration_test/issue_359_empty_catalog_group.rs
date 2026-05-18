use super::common::{create_config, fixture_path};

#[test]
fn detects_empty_named_catalog_groups_only() {
    let root = fixture_path("issue-359-empty-catalog-group");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let actual: Vec<_> = results
        .empty_catalog_groups
        .iter()
        .map(|group| {
            (
                group.group.catalog_name.as_str(),
                group.group.path.as_path(),
                group.group.line,
            )
        })
        .collect();

    assert_eq!(
        actual,
        vec![
            ("legacy", std::path::Path::new("pnpm-workspace.yaml"), 8),
            ("react17", std::path::Path::new("pnpm-workspace.yaml"), 7,),
        ],
        "unexpected empty catalog group findings: {actual:?}",
    );
    assert!(
        results
            .empty_catalog_groups
            .iter()
            .all(|group| group.group.catalog_name != "default"),
        "top-level catalog: must not be flagged even when empty",
    );
}
