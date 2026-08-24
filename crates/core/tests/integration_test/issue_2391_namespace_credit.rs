use super::common::{create_config, fixture_path};

const FIXTURE: &str = "issue-2391-namespace-credit";

fn unused_export_pairs(results: &fallow_core::results::AnalysisResults) -> Vec<(String, String)> {
    results
        .unused_exports
        .iter()
        .map(|finding| {
            (
                finding.export.path.to_string_lossy().replace('\\', "/"),
                finding.export.export_name.clone(),
            )
        })
        .collect()
}

fn unused_type_pairs(results: &fallow_core::results::AnalysisResults) -> Vec<(String, String)> {
    results
        .unused_types
        .iter()
        .map(|finding| {
            (
                finding.export.path.to_string_lossy().replace('\\', "/"),
                finding.export.export_name.clone(),
            )
        })
        .collect()
}

#[test]
fn require_bindings_use_semantic_lanes_and_unused_binding_rules() {
    let results = fallow_core::analyze(&create_config(fixture_path(FIXTURE)))
        .expect("analysis should succeed");
    let exports = unused_export_pairs(&results);
    let types = unused_type_pairs(&results);

    for used in ["ReqA", "ReqB"] {
        assert!(
            !types
                .iter()
                .any(|(path, name)| path.ends_with("src/req-types.ts") && name == used),
            "a type reached through a require namespace must be credited: {types:?}"
        );
    }
    assert!(
        types
            .iter()
            .any(|(path, name)| path.ends_with("src/req-types.ts") && name == "ReqUnused"),
        "an unread type sibling must keep reporting: {types:?}"
    );
    for unused in ["unusedOne", "unusedTwo"] {
        assert!(
            exports
                .iter()
                .any(|(path, name)| path.ends_with("src/unused-target.ts") && name == unused),
            "an unreferenced require binding must not credit `{unused}`: {exports:?}"
        );
    }
}

#[test]
fn namespace_handover_credits_type_and_member_surfaces() {
    let mut config = create_config(fixture_path(FIXTURE));
    config.rules.unused_class_members = fallow_config::Severity::Error;
    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    let types = unused_type_pairs(&results);

    for (path, name) in [
        ("src/reexport-target.ts", "ReexportType"),
        ("src/ambient-target.ts", "AmbientType"),
    ] {
        assert!(
            !types
                .iter()
                .any(|(reported_path, reported)| reported_path.ends_with(path) && reported == name),
            "{path}:{name} is reachable through its namespace binding: {types:?}"
        );
    }

    let class_members: Vec<String> = results
        .unused_class_members
        .iter()
        .filter(|finding| finding.member.parent_name == "NamespaceWidget")
        .map(|finding| finding.member.member_name.clone())
        .collect();
    let enum_members: Vec<String> = results
        .unused_enum_members
        .iter()
        .filter(|finding| finding.member.parent_name == "NamespaceMode")
        .map(|finding| finding.member.member_name.clone())
        .collect();
    assert!(
        class_members.is_empty() && enum_members.is_empty(),
        "a whole namespace handover must abstain member detectors: class={class_members:?}, enum={enum_members:?}"
    );
}

#[test]
fn require_dynamic_and_vue_template_handover_credit_sibling_exports() {
    let results = fallow_core::analyze(&create_config(fixture_path(FIXTURE)))
        .expect("analysis should succeed");
    let exports = unused_export_pairs(&results);

    for (path, sibling) in [
        ("src/require-icons.ts", "RequireMoon"),
        ("src/dynamic-icons.ts", "DynamicMoon"),
        ("src/vue-icons.ts", "VueMoon"),
        ("src/vue-icons.ts", "VueSun"),
    ] {
        assert!(
            !exports
                .iter()
                .any(|(reported_path, name)| reported_path.ends_with(path) && name == sibling),
            "{path}:{sibling} is reachable through a whole-object handover: {exports:?}"
        );
    }
}
