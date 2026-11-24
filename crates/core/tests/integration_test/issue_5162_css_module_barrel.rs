use super::common::{create_config, fixture_path};

/// A barrel that re-exports the default of a CSS Module hands the class map
/// to its consumers. The consumer reads classes through the re-exported name,
/// so each read class and the re-export itself are used.
#[test]
fn barrel_re_export_of_css_module_default_credits_read_classes() {
    let root = fixture_path("issue-5162-css-module-barrel");
    let results = fallow_core::analyze(&create_config(root)).expect("analysis should succeed");
    let unused: Vec<(String, String)> = results
        .unused_exports
        .iter()
        .map(|finding| {
            (
                finding.export.path.to_string_lossy().replace('\\', "/"),
                finding.export.export_name.clone(),
            )
        })
        .collect();
    let is_reported = |path: &str, name: &str| {
        unused
            .iter()
            .any(|(reported_path, reported)| reported_path.ends_with(path) && reported == name)
    };

    for (path, used) in [
        ("src/styles/index.ts", "ui"),
        ("src/styles/index.ts", "outer"),
        ("src/styles/index.ts", "whole"),
        ("src/theme.ts", "outer"),
        ("src/styles/plain.ts", "default"),
        ("src/styles/ui.module.css", "uiUsed"),
        ("src/styles/outer.module.css", "outerUsed"),
        ("src/styles/plain.module.css", "plainUsed"),
        ("src/styles/whole.module.css", "wholeFirst"),
        ("src/styles/whole.module.css", "wholeSecond"),
        ("src/alt/index.ts", "ui"),
        ("src/alt/ui.module.css", "altUsed"),
        ("src/styles/exposed.module.css", "exposedSpare"),
        ("src/config/index.ts", "settings"),
        ("src/app-config.ts", "settings"),
    ] {
        assert!(
            !is_reported(path, used),
            "{path}:{used} must be credited by its consumer: {unused:?}"
        );
    }

    for (path, spare) in [
        ("src/styles/ui.module.css", "uiSpare"),
        ("src/styles/outer.module.css", "outerSpare"),
        ("src/styles/plain.module.css", "plainSpare"),
    ] {
        assert!(
            is_reported(path, spare),
            "{path}:{spare} is never read and must keep reporting: {unused:?}"
        );
    }

    let duplicates: Vec<&str> = results
        .duplicate_exports
        .iter()
        .map(|finding| finding.export.export_name.as_str())
        .collect();
    assert_eq!(
        duplicates,
        ["ui"],
        "a barrel chain that forwards one default under one name has one origin, \
         and two barrels that forward two class maps under one name stay a duplicate"
    );
}
