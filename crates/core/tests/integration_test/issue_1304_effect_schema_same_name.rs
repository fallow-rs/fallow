use super::common::{create_config, fixture_path};

#[test]
fn same_name_effect_schema_type_alias_keeps_nested_schema_value_used() {
    let root = fixture_path("issue-1304-effect-schema-same-name");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_export_names: Vec<&str> = results
        .unused_exports
        .iter()
        .map(|e| e.export.export_name.as_str())
        .collect();

    assert!(
        !unused_export_names.contains(&"ServiceCategoryResponse"),
        "ServiceCategoryResponse should be credited through Schema.Array(ServiceCategoryResponse), found: {unused_export_names:?}"
    );
    assert!(
        !unused_export_names.contains(&"AssistantPromptResponse"),
        "AssistantPromptResponse should be credited by the route schema import, found: {unused_export_names:?}"
    );
    assert!(
        unused_export_names.contains(&"UnusedSiblingSchema"),
        "unrelated schema exports must remain reportable, found: {unused_export_names:?}"
    );
    assert!(
        unused_export_names.contains(&"OrphanChildSchema"),
        "a schema used only by an unused same-file parent must remain reportable, found: {unused_export_names:?}"
    );
    assert!(
        unused_export_names.contains(&"UnusedParentSchema"),
        "unused parent schemas must remain reportable, found: {unused_export_names:?}"
    );
    assert!(
        unused_export_names.contains(&"ShadowedChildSchema"),
        "shadowed same-file references must not credit unrelated exports, found: {unused_export_names:?}"
    );
    assert!(
        !unused_export_names.contains(&"BlockScopedChildSchema"),
        "block-local shadows must not hide later real same-file export references, found: {unused_export_names:?}"
    );
    assert!(
        unused_export_names.contains(&"HoistedShadowChildSchema"),
        "same-scope hoisted declarations must shadow earlier references, found: {unused_export_names:?}"
    );
}
