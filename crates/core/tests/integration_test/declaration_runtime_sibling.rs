//! A hand-written declaration file next to a JS module (`lib.d.mts` next to
//! `lib.mjs`, `legacy.d.ts` next to `legacy.js`) types the imports of that
//! module. Under `includeEntryExports`, an export of the declaration file that
//! an importer of the JS module uses must not report as unused. A type read
//! through a namespace import of the JS module (`ns.WrapOpts`) counts as a
//! use (issue #4551).

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
        (
            "src/ns.d.mts",
            &["wrap", "WrapOpts"][..],
            &["UnusedWrapOpts"][..],
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

/// A `usedExports` rule on the path of the JS module also credits the export
/// with that name in the declaration file next to it (issue #4551).
#[test]
fn used_exports_rule_on_runtime_module_credits_declaration_sibling() {
    let dir = tempfile::tempdir().expect("project");
    let root = dir.path();
    for (path, source) in [
        (
            "package.json",
            r#"{"name":"rule-sibling","private":true,"type":"module","main":"src/index.ts","dependencies":{"hook-lib":"1.0.0"}}"#,
        ),
        (
            "src/index.ts",
            "import { run } from './plugin.mjs';\nconsole.log(run());\n",
        ),
        (
            "src/plugin.mjs",
            "export const run = () => 1;\nexport const hook = () => 2;\n",
        ),
        (
            "src/plugin.d.mts",
            "export declare const run: () => number;\nexport declare const hook: () => number;\nexport declare const unusedDecl: number;\n",
        ),
        (
            ".fallowrc.json",
            r#"{
                "includeEntryExports": true,
                "framework": [{
                    "name": "hook-exports",
                    "enablers": ["hook-lib"],
                    "usedExports": [{ "pattern": "src/plugin.mjs", "exports": ["hook"] }]
                }]
            }"#,
        ),
    ] {
        let path = root.join(path);
        std::fs::create_dir_all(path.parent().expect("parent")).expect("directory");
        std::fs::write(path, source).expect("fixture file");
    }
    let config = fallow_config::FallowConfig::load(&root.join(".fallowrc.json"))
        .expect("config loads")
        .resolve(
            root.to_path_buf(),
            fallow_config::OutputFormat::Json,
            1,
            true,
            true,
            None,
        );

    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let mut declaration = unused_names_in(&results, "src/plugin.d.mts");
    declaration.sort();
    assert_eq!(
        declaration,
        vec!["unusedDecl".to_string()],
        "the rule on src/plugin.mjs credits `hook` in src/plugin.d.mts"
    );
    assert!(
        unused_names_in(&results, "src/plugin.mjs").is_empty(),
        "the rule credits `hook` in the JS module"
    );
}
