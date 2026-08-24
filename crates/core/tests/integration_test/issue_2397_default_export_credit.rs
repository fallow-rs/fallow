use super::common::{create_config, fixture_path};

#[test]
fn ambient_star_excludes_default_chains() {
    let root = fixture_path("issue-2397-default-export-credit");
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

    for (path, name) in [
        ("src/ambient-impl.ts", "default"),
        ("src/ambient-default.ts", "ambientDefaultOnly"),
    ] {
        assert!(
            unused
                .iter()
                .any(|(reported_path, reported)| reported_path.ends_with(path) && reported == name),
            "the ambient plain-star surface cannot reach {path}:{name}: {unused:?}"
        );
    }
}

#[test]
fn named_defaults_skip_duplicate_grouping() {
    let root = fixture_path("issue-2397-default-export-credit");
    let results = fallow_core::analyze(&create_config(root)).expect("analysis should succeed");
    assert!(
        !results
            .duplicate_exports
            .iter()
            .any(|finding| finding.export.export_name == "default"),
        "module-local default slots must not form a duplicate-export group: {:?}",
        results.duplicate_exports
    );
}

#[test]
fn proven_commonjs_object_maps_narrow_plain_default_imports() {
    let root = fixture_path("issue-2397-default-export-credit");
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

    assert!(
        !unused
            .iter()
            .any(|(path, name)| { path.ends_with("src/theme.cjs") && name == "primary" }),
        "the accessed object-map key must stay credited: {unused:?}"
    );
    for name in ["default", "accent"] {
        assert!(
            unused
                .iter()
                .any(|(path, reported)| path.ends_with("src/theme.cjs") && reported == name),
            "an unaccessed object-map key must keep reporting: {name}: {unused:?}"
        );
    }
    assert!(
        unused.iter().any(|(path, name)| {
            path.ends_with("src/aliased-member.cjs") && name == "aliasMemberSpare"
        }),
        "the named-default spelling must retain precise object-map narrowing: {unused:?}"
    );
}

#[test]
fn ambiguous_commonjs_forms_and_whole_handoffs_stay_conservative() {
    let root = fixture_path("issue-2397-default-export-credit");
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

    for (path, name) in [
        ("src/computed.cjs", "computedSpare"),
        ("src/compound.cjs", "compoundSpare"),
    ] {
        assert!(
            unused
                .iter()
                .any(|(reported_path, reported)| reported_path.ends_with(path) && reported == name),
            "ambiguous CommonJS forms must preserve the prior reporting behavior for {path}:{name}: {unused:?}"
        );
    }

    for (path, name) in [
        ("src/handoff.cjs", "handoffSpare"),
        ("src/aliased-handoff.cjs", "aliasHandoffSpare"),
    ] {
        assert!(
            !unused
                .iter()
                .any(|(reported_path, reported)| reported_path.ends_with(path) && reported == name),
            "a whole-object CommonJS handoff must retain {path}:{name}: {unused:?}"
        );
    }
}
