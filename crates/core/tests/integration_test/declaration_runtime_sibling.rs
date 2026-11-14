//! A hand-written declaration file next to a JS module (`lib.d.mts` next to
//! `lib.mjs`, `legacy.d.ts` next to `legacy.js`) types the imports of that
//! module. Under `includeEntryExports`, an export of the declaration file that
//! an importer of the JS module uses must not report as unused.

use super::common::{create_config, fixture_path};
use fallow_types::results::AnalysisResults;

fn unused_names_in(results: &AnalysisResults, file: &str) -> Vec<String> {
    results
        .unused_exports
        .iter()
        .map(|finding| &finding.export)
        .chain(results.unused_types.iter().map(|finding| &finding.export))
        .filter(|export| {
            export
                .path
                .to_string_lossy()
                .replace('\\', "/")
                .ends_with(file)
        })
        .map(|export| export.export_name.clone())
        .collect()
}

#[test]
fn declaration_sibling_exports_are_credited_by_runtime_imports() {
    let root = fixture_path("declaration-runtime-sibling");
    let mut config = create_config(root);
    config.include_entry_exports = true;

    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    for (file, used, unused) in [
        (
            "src/lib.d.mts",
            &["helper", "HelperOpts"][..],
            &["unusedJs", "UnusedOpts"][..],
        ),
        (
            "src/legacy.d.ts",
            &["default", "LegacyOpts"][..],
            &["unusedLegacy"][..],
        ),
    ] {
        let reported = unused_names_in(&results, file);
        for name in used {
            assert!(
                !reported.contains(&(*name).to_string()),
                "{file} {name} is used through the JS sibling, got: {reported:?}"
            );
        }
        for name in unused {
            assert!(
                reported.contains(&(*name).to_string()),
                "{file} {name} has no importer and must report, got: {reported:?}"
            );
        }
    }
    let runtime = unused_names_in(&results, "src/lib.mjs");
    assert_eq!(
        runtime,
        vec!["unusedJs".to_string()],
        "the JS module keeps its own findings"
    );
}
