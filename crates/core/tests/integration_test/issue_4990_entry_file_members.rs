use super::common::fixture_path;

const FIXTURE: &str = "issue-4990-entry-file-members";

fn analyze_fixture(include_entry_exports: bool) -> fallow_core::results::AnalysisResults {
    let root = fixture_path(FIXTURE);
    let (mut loaded, _path) = fallow_config::FallowConfig::find_and_load(&root)
        .expect("config load")
        .expect("fixture has .fallowrc.json");
    if !include_entry_exports {
        loaded.workspaces = None;
    }
    let config = loaded.resolve(
        root,
        fallow_config::OutputFormat::Human,
        1,
        true,
        true,
        None,
    );
    fallow_core::analyze(&config).expect("analysis should succeed")
}

fn members(findings: impl Iterator<Item = fallow_core::results::UnusedMember>) -> Vec<String> {
    let mut names: Vec<String> = findings
        .map(|member| format!("{}.{}", member.parent_name, member.member_name))
        .collect();
    names.sort_unstable();
    names
}

#[test]
fn members_of_an_entry_file_export_are_not_checked_by_default() {
    let results = analyze_fixture(false);

    assert!(results.unused_namespace_members.is_empty());
    assert!(results.unused_enum_members.is_empty());
    assert!(results.unused_class_members.is_empty());
}

#[test]
fn include_entry_exports_checks_the_members_of_an_entry_file_export() {
    let results = analyze_fixture(true);

    // `@p/pub` has the same shapes, and `publicPackages` names it.
    assert_eq!(
        members(
            results
                .unused_namespace_members
                .iter()
                .map(|finding| finding.member.clone())
        ),
        ["Codes.unusedMember"]
    );
    assert_eq!(
        members(
            results
                .unused_enum_members
                .iter()
                .map(|finding| finding.member.clone())
        ),
        ["Level.High"]
    );
    assert_eq!(
        members(
            results
                .unused_class_members
                .iter()
                .map(|finding| finding.member.clone())
        ),
        ["Service.idle"]
    );
}
