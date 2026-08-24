use super::common::{create_config, fixture_path};

#[test]
fn every_css_module_spelling_credits_exact_or_whole_class_maps() {
    let root = fixture_path("issue-2395-css-module-credit");
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

    for (path, used) in [
        ("src/theme.module.less", "lessUsed"),
        ("src/theme.module.sass", "sassUsed"),
        ("src/alias.module.css", "aliasUsed"),
        ("src/whole.module.css", "wholeSpare"),
        ("src/extensionless.module.css", "extensionlessSpare"),
        ("src/aliased-target.module.css", "aliasedSpare"),
    ] {
        assert!(
            !unused
                .iter()
                .any(|(reported_path, name)| reported_path.ends_with(path) && name == used),
            "{path}:{used} must be credited by its consumer: {unused:?}"
        );
    }

    for (path, unused_name) in [
        ("src/theme.module.less", "lessUnused"),
        ("src/theme.module.sass", "sassUnused"),
        ("src/alias.module.css", "aliasUnused"),
        (
            "src/shadow-extensionless.module.css",
            "extensionlessShadowSpare",
        ),
        ("src/shadow-aliased.module.css", "aliasedShadowSpare"),
    ] {
        assert!(
            unused
                .iter()
                .any(|(reported_path, name)| reported_path.ends_with(path) && name == unused_name),
            "{path}:{unused_name} is never read and must keep reporting: {unused:?}"
        );
    }

    assert!(
        results.duplicate_exports.is_empty(),
        "CSS Module class maps are file-local and must not form duplicate-export groups: {:?}",
        results.duplicate_exports
    );
}
