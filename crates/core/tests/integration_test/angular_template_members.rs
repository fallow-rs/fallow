use super::common::{create_config, fixture_path};

#[test]
fn angular_external_template_credits_inherited_and_di_injected_members() {
    let root = fixture_path("angular-template-inherited-members");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused: Vec<(&str, &str)> = results
        .unused_class_members
        .iter()
        .map(|m| (m.member.parent_name.as_str(), m.member.member_name.as_str()))
        .collect();

    assert!(
        !unused.contains(&("BaseFieldHandlerDirective", "trimValue")),
        "BaseFieldHandlerDirective.trimValue is used in child's external template via (blur)=\"trimValue()\", found: {unused:?}"
    );
    assert!(
        !unused.contains(&("BaseFieldHandlerDirective", "tooltipClass")),
        "BaseFieldHandlerDirective.tooltipClass is used in child's external template via [class]=\"tooltipClass\", found: {unused:?}"
    );

    assert!(
        !unused.contains(&("DataService", "getTotal")),
        "DataService.getTotal is used in external template via {{{{ dataService.getTotal() }}}}, found: {unused:?}"
    );
    assert!(
        !unused.contains(&("DataService", "getInjectedTotal")),
        "DataService.getInjectedTotal is used in external template via {{{{ injectedDataService.getInjectedTotal() }}}}, found: {unused:?}"
    );
    assert!(
        !unused.contains(&("DataService", "isEmpty")),
        "DataService.isEmpty is used in external template via @if (!dataService.isEmpty()), found: {unused:?}"
    );

    assert!(
        !unused.contains(&("DataService", "items")),
        "DataService.items is used in external template via @for (item of dataService.items), found: {unused:?}"
    );

    assert!(
        unused.contains(&("BaseFieldHandlerDirective", "unusedBaseMethod")),
        "BaseFieldHandlerDirective.unusedBaseMethod is never used and should be flagged, found: {unused:?}"
    );
    assert!(
        unused.contains(&("DataService", "unusedServiceMethod")),
        "DataService.unusedServiceMethod is never used and should be flagged, found: {unused:?}"
    );
}

#[test]
fn angular_at_if_alias_credits_condition_member() {
    let root = fixture_path("issue-308-at-if-alias");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused: Vec<(&str, &str)> = results
        .unused_class_members
        .iter()
        .map(|m| (m.member.parent_name.as_str(), m.member.member_name.as_str()))
        .collect();

    assert!(
        !unused.contains(&("InlineTemplateComponent", "withAlias")),
        "InlineTemplateComponent.withAlias is referenced via `@if (withAlias(); as aliased)`, found: {unused:?}"
    );
    assert!(
        !unused.contains(&("InlineTemplateComponent", "withoutAlias")),
        "InlineTemplateComponent.withoutAlias is referenced via `@if (withoutAlias())`, found: {unused:?}"
    );

    assert!(
        !unused.contains(&("ExternalTemplateComponent", "externalWithAlias")),
        "ExternalTemplateComponent.externalWithAlias is referenced in external template via `@if (externalWithAlias(); as aliased)`, found: {unused:?}"
    );

    assert!(
        unused.contains(&("InlineTemplateComponent", "genuinelyUnused")),
        "InlineTemplateComponent.genuinelyUnused is never referenced and must still be flagged, found: {unused:?}"
    );
    assert!(
        unused.contains(&("ExternalTemplateComponent", "externalUnused")),
        "ExternalTemplateComponent.externalUnused is never referenced and must still be flagged, found: {unused:?}"
    );
}
