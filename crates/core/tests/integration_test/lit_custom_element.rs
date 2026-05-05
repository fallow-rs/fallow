use crate::common::{create_config, fixture_path};

#[test]
fn lit_custom_element_class_exports_credited_through_decorator_and_define() {
    let root = fixture_path("lit-custom-element");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_export_names: Vec<&str> = results
        .unused_exports
        .iter()
        .map(|e| e.export_name.as_str())
        .collect();

    // Decorator-registered class.
    assert!(
        !unused_export_names.contains(&"MyElement"),
        "Lit @customElement-decorated class should be credited as side-effect-used, found unused exports: {unused_export_names:?}"
    );
    // Native customElements.define-registered class.
    assert!(
        !unused_export_names.contains(&"OtherElement"),
        "customElements.define-registered class should be credited as side-effect-used, found: {unused_export_names:?}"
    );
    // Class declared then exported then registered (order-independent).
    assert!(
        !unused_export_names.contains(&"SeparateElement"),
        "class registered after its export statement should still be credited, found: {unused_export_names:?}"
    );
    // Member-call decorator form (`@decorators.customElement(...)`).
    assert!(
        !unused_export_names.contains(&"AliasedElement"),
        "namespace-aliased @customElement form should also be credited, found: {unused_export_names:?}"
    );
    // Named import alias form (`import { customElement as ce }`).
    assert!(
        !unused_export_names.contains(&"NamedImportAliasedElement"),
        "named-import-aliased @customElement form should also be credited, found: {unused_export_names:?}"
    );

    // Anonymous `export default @customElement(...) class extends LitElement {}`
    // has no class identifier and unset local_name; the visitor flips the pending
    // Default export directly during visit_class.
    let anonymous_default_unused = results
        .unused_exports
        .iter()
        .any(|e| e.path.ends_with("anonymous-default.ts") && e.export_name == "default");
    assert!(
        !anonymous_default_unused,
        "anonymous default-exported @customElement class should be credited, unused exports were: {:?}",
        results
            .unused_exports
            .iter()
            .map(|e| (e.path.to_string_lossy().into_owned(), e.export_name.clone()))
            .collect::<Vec<_>>()
    );
}

#[test]
fn lit_lifecycle_methods_not_flagged_but_genuinely_unused_methods_are() {
    let root = fixture_path("lit-custom-element");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused: Vec<String> = results
        .unused_class_members
        .iter()
        .map(|m| format!("{}.{}", m.parent_name, m.member_name))
        .collect();

    // Lit lifecycle on LitElement subclass.
    assert!(
        !unused.contains(&"MyElement.render".to_string()),
        "render() on LitElement subclass should be plugin-allowlisted, found: {unused:?}"
    );
    assert!(
        !unused.contains(&"MyElement.connectedCallback".to_string()),
        "connectedCallback on LitElement should be allowlisted, found: {unused:?}"
    );
    // Native lifecycle on HTMLElement subclass.
    assert!(
        !unused.contains(&"OtherElement.connectedCallback".to_string()),
        "connectedCallback on HTMLElement subclass should be allowlisted, found: {unused:?}"
    );
    assert!(
        !unused.contains(&"OtherElement.observedAttributes".to_string()),
        "observedAttributes on HTMLElement should be allowlisted, found: {unused:?}"
    );
    // Genuinely unused helpers should still surface.
    assert!(
        unused.contains(&"MyElement.unusedHelper".to_string()),
        "non-lifecycle unused method on a LitElement should still be reported, found: {unused:?}"
    );
    assert!(
        unused.contains(&"OtherElement.unusedNativeHelper".to_string()),
        "non-lifecycle unused method on an HTMLElement should still be reported, found: {unused:?}"
    );
}
