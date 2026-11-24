//! A re-export from a React Native platform family credits every member.
//!
//! `button.ts` re-exports `default`, `label.ts` re-exports `Label` by name and
//! `icons.ts` uses `export * from`. Each target is a family with a base file
//! and an `.ios` variant. Metro selects one member per platform, so the
//! re-exported names are used on each member, as for an import.

use super::common::{create_config, fixture_path};

fn unused_exports(results: &fallow_types::results::AnalysisResults) -> Vec<(String, String)> {
    results
        .unused_exports
        .iter()
        .map(|finding| {
            let path = finding.export.path.to_string_lossy().replace('\\', "/");
            let file = path.rsplit("/src/").next().unwrap_or(&path).to_string();
            (file, finding.export.export_name.clone())
        })
        .collect()
}

fn has(found: &[(String, String)], file: &str, name: &str) -> bool {
    found.iter().any(|(f, n)| f == file && n == name)
}

#[test]
fn re_export_credits_names_on_every_platform_family_member() {
    let root = fixture_path("react-native-platform-family-re-export");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    let found = unused_exports(&results);

    for file in ["x/x.tsx", "x/x.ios.tsx"] {
        assert!(
            !has(&found, file, "default"),
            "a default re-export credits `default` on {file}: {found:?}"
        );
    }
    for file in ["label/Label.tsx", "label/Label.ios.tsx"] {
        assert!(
            !has(&found, file, "Label"),
            "a named re-export credits `Label` on {file}: {found:?}"
        );
    }
    for file in ["icon/Icon.tsx", "icon/Icon.ios.tsx"] {
        assert!(
            !has(&found, file, "Icon"),
            "an `export *` credits the used name `Icon` on {file}: {found:?}"
        );
        assert!(
            has(&found, file, "iconSize"),
            "an `export *` must not credit the unused name `iconSize` on {file}: {found:?}"
        );
    }
}
