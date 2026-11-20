use super::common::{create_config, fixture_path};

#[test]
fn flow_style_catalog_group_is_not_empty() {
    let root = fixture_path("flow-style-pnpm-workspace");
    let workspaces = fallow_config::discover_workspaces(&root);
    assert!(
        workspaces.iter().any(|workspace| workspace.name == "app"),
        "the flow-style packages list declares packages/app: {workspaces:?}"
    );

    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let empty: Vec<_> = results
        .empty_catalog_groups
        .iter()
        .map(|group| (group.group.catalog_name.as_str(), group.group.line))
        .collect();
    assert_eq!(empty, vec![("old", 8)]);

    let unresolved: Vec<_> = results
        .unresolved_catalog_references
        .iter()
        .map(|reference| reference.reference.entry_name.as_str())
        .collect();
    assert_eq!(unresolved, vec!["missing-pkg"]);

    let unused: Vec<_> = results
        .unused_catalog_entries
        .iter()
        .map(|entry| entry.entry.entry_name.as_str())
        .collect();
    assert!(unused.is_empty(), "unexpected unused entries: {unused:?}");
}
