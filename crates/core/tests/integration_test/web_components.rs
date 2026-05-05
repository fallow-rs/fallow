use crate::common::{create_config, fixture_path};

#[test]
fn native_web_component_lifecycle_does_not_require_lit_dependency() {
    let root = fixture_path("web-components-native");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_exports: Vec<&str> = results
        .unused_exports
        .iter()
        .map(|e| e.export_name.as_str())
        .collect();
    assert!(
        !unused_exports.contains(&"NativeElement"),
        "customElements.define should credit the registered class export: {unused_exports:?}"
    );

    let unused_members: Vec<String> = results
        .unused_class_members
        .iter()
        .map(|m| format!("{}.{}", m.parent_name, m.member_name))
        .collect();
    assert!(
        !unused_members.contains(&"NativeElement.connectedCallback".to_string()),
        "HTMLElement lifecycle methods should be allowlisted without Lit: {unused_members:?}"
    );
    assert!(
        !unused_members.contains(&"NativeElement.observedAttributes".to_string()),
        "HTMLElement static lifecycle properties should be allowlisted without Lit: {unused_members:?}"
    );
    assert!(
        unused_members.contains(&"NativeElement.unusedHelper".to_string()),
        "non-lifecycle members should still be reported: {unused_members:?}"
    );
}

#[test]
fn custom_element_named_decorator_must_come_from_lit() {
    let root = fixture_path("web-components-native");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_exports: Vec<&str> = results
        .unused_exports
        .iter()
        .map(|e| e.export_name.as_str())
        .collect();
    assert!(
        unused_exports.contains(&"NotLitElement"),
        "local @customElement decorator must not credit a class as side-effect-used: {unused_exports:?}"
    );
    assert!(
        unused_exports.contains(&"NotLitNamespaceElement"),
        "local namespace customElement decorator must not credit a class as side-effect-used: {unused_exports:?}"
    );
}
