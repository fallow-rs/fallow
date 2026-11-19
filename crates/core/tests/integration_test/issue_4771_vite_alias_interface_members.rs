//! Issue #4771: a class in a workspace package implements an interface that
//! the file imports through a Vite `resolve.alias`. The file is reachable
//! only through the `rollupOptions.input` of the package config. Each member
//! that the interface declares counts as used on the class. A member that
//! the interface does not declare stays unused.

use super::common::{create_config, fixture_path};

fn unused_member_names(results: &fallow_types::results::AnalysisResults) -> Vec<String> {
    results
        .unused_class_members
        .iter()
        .map(|m| format!("{}.{}", m.member.parent_name, m.member.member_name))
        .collect()
}

#[test]
fn members_that_implement_an_aliased_interface_are_used() {
    let root = fixture_path("issue-4771-vite-alias-interface-members");
    let mut config = create_config(root);
    config.rules.unused_class_members = fallow_config::Severity::Error;
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let files: Vec<String> = results
        .unused_files
        .iter()
        .map(|f| f.file.path.to_string_lossy().replace('\\', "/"))
        .collect();
    assert!(
        files.is_empty(),
        "every fixture file is reachable. Got: {files:?}"
    );

    let members = unused_member_names(&results);
    for used in [
        "ZipBackend.isLive",
        "ZipBackend.readText",
        "FetchBackend.isLive",
        "FetchBackend.readText",
    ] {
        assert!(
            !members.iter().any(|m| m == used),
            "{used} implements a member of the aliased interface. Got: {members:?}"
        );
    }
    assert!(
        members.iter().any(|m| m == "ZipBackend.unusedZipHelper"),
        "the interface does not declare unusedZipHelper, so it stays unused. Got: {members:?}"
    );
}
