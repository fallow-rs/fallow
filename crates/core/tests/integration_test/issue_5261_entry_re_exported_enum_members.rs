use super::common::{create_config, fixture_path};

const FIXTURE: &str = "issue-5261-entry-re-exported-enum-members";

fn enum_members(results: &fallow_core::results::AnalysisResults) -> Vec<String> {
    let mut names: Vec<String> = results
        .unused_enum_members
        .iter()
        .map(|finding| {
            format!(
                "{}.{}",
                finding.member.parent_name, finding.member.member_name
            )
        })
        .collect();
    names.sort_unstable();
    names
}

#[test]
fn members_of_an_enum_that_the_entry_re_exports_are_public() {
    let config = create_config(fixture_path(FIXTURE));
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    // The entry publishes `JobName` (`export *`), `Priority` (named
    // re-export), `Level` (`export * as levels`), `Mode` (two barrels),
    // `Alias` (renamed re-export) and `Kind` (type-only re-export). It does
    // not publish `Unpublished` and `Internal`.
    assert_eq!(
        enum_members(&results),
        ["Internal.Unused", "Unpublished.Unused"]
    );
}

#[test]
fn include_entry_exports_checks_the_members_of_a_re_exported_enum() {
    let mut config = create_config(fixture_path(FIXTURE));
    config.include_entry_exports = true;
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    assert_eq!(
        enum_members(&results),
        [
            "Alias.Unused",
            "Internal.Unused",
            "JobName.Stop",
            "Kind.Unused",
            "Level.Error",
            "Mode.Slow",
            "Priority.High",
            "Unpublished.Unused",
        ]
    );
}

#[test]
fn a_private_package_keeps_the_members_of_a_re_exported_enum_checked() {
    let config = create_config(fixture_path("issue-5261-private-package-enum-members"));
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    assert_eq!(enum_members(&results), ["JobName.Stop"]);
}

#[test]
fn a_source_index_without_exports_publishes_its_re_exported_enums() {
    let config = create_config(fixture_path("issue-5261-source-index-enum-members"));
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    assert!(
        enum_members(&results).is_empty(),
        "found: {:?}",
        enum_members(&results)
    );
}
