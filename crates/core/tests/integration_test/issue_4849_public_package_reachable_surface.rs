//! `publicPackages` keeps the public surface of a package as used, but still
//! reports exports and members that the surface cannot reach (issue #4849).

use super::common::{create_config, fixture_path};

fn analyze(public_packages: &[&str]) -> fallow_core::results::AnalysisResults {
    let mut config = create_config(fixture_path("public-package-reachable-surface"));
    config.public_packages = public_packages.iter().map(ToString::to_string).collect();
    fallow_core::analyze(&config).expect("analysis should succeed")
}

fn unused_export_names(results: &fallow_core::results::AnalysisResults) -> Vec<String> {
    results
        .unused_exports
        .iter()
        .map(|e| e.export.export_name.clone())
        .collect()
}

fn unused_member_names(results: &fallow_core::results::AnalysisResults) -> Vec<String> {
    let class_members = results.unused_class_members.iter().map(|m| &m.member);
    let enum_members = results.unused_enum_members.iter().map(|m| &m.member);
    class_members
        .chain(enum_members)
        .map(|m| format!("{}.{}", m.parent_name, m.member_name))
        .collect()
}

#[test]
fn public_package_reports_exports_outside_public_surface() {
    let results = analyze(&["@x/lib"]);
    let exports = unused_export_names(&results);
    assert!(
        exports.contains(&"deadInternal".to_string()),
        "an export that the public surface cannot reach must stay reported: {exports:?}"
    );
    assert!(
        exports.contains(&"deadSurfaceSibling".to_string()),
        "a sibling of a re-exported symbol must stay reported: {exports:?}"
    );
    for name in [
        "api",
        "PublicSvc",
        "PublicMode",
        "StarSvc",
        "ToolBox",
        "helper",
        "Svc",
    ] {
        assert!(
            !exports.contains(&name.to_string()),
            "{name} is used or on the public surface: {exports:?}"
        );
    }
}

#[test]
fn public_package_reports_members_of_internal_classes() {
    let results = analyze(&["@x/lib"]);
    let members = unused_member_names(&results);
    assert!(
        members.contains(&"Svc.dead".to_string()),
        "a member of a class off the public surface must stay reported: {members:?}"
    );
}

#[test]
fn public_package_keeps_members_of_re_exported_class_and_enum() {
    let results = analyze(&["@x/lib"]);
    let members = unused_member_names(&results);
    for name in [
        "PublicSvc.consumerMethod",
        "PublicMode.On",
        "PublicMode.Off",
        "StarSvc.starMethod",
        "ToolBox.toolMethod",
    ] {
        assert!(
            !members.contains(&name.to_string()),
            "{name} is on the public surface: {members:?}"
        );
    }
}

#[test]
fn non_public_package_reports_the_surface_members() {
    let results = analyze(&[]);
    let members = unused_member_names(&results);
    assert!(
        members.contains(&"PublicSvc.consumerMethod".to_string()),
        "without publicPackages the surface members are reported: {members:?}"
    );
}

#[test]
fn private_manifest_in_public_packages_keeps_its_surface() {
    let results = analyze(&["@x/*"]);
    let members = unused_member_names(&results);
    assert!(
        !members.contains(&"PrivSvc.privMethod".to_string()),
        "publicPackages selects the package even with `private: true`: {members:?}"
    );
    let exports = unused_export_names(&results);
    assert!(
        exports.contains(&"privDead".to_string()),
        "an export off the surface of the private package stays reported: {exports:?}"
    );
}
