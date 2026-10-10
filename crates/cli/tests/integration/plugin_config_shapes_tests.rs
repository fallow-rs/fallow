//! Config shape regressions through the actual discovery and analysis pipeline.

use crate::common::{fixture_path, parse_json, run_fallow_in_root};
use std::path::Path;

fn unused_files(root: &Path) -> Vec<String> {
    let output = run_fallow_in_root(
        "dead-code",
        root,
        &[
            "--unused-files",
            "--format",
            "json",
            "--quiet",
            "--no-cache",
        ],
    );
    assert_eq!(
        output.code, 1,
        "dead sibling must report: {}",
        output.stderr
    );
    let report = parse_json(&output);
    report["unused_files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|finding| finding["path"].as_str().unwrap().to_string())
        .collect()
}

#[test]
fn function_config_arrays_credit_only_the_declared_tsup_and_tsdown_entries() {
    assert_eq!(
        unused_files(&fixture_path("plugin-function-config")),
        ["custom/dead.ts"],
    );
}

#[test]
fn nuxt_module_extension_configs_keep_configured_plugins_reachable() {
    for extension in ["mjs", "mts", "cjs", "cts"] {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        std::fs::create_dir(root.join("custom")).unwrap();
        std::fs::write(
            root.join("package.json"),
            "{\"name\":\"nuxt-config-fixture\"}",
        )
        .unwrap();
        let prefix = if matches!(extension, "cjs" | "cts") {
            "module.exports ="
        } else {
            "export default"
        };
        std::fs::write(
            root.join(format!("nuxt.config.{extension}")),
            format!("{prefix} {{ plugins: ['./custom/live.ts'] }};"),
        )
        .unwrap();
        std::fs::write(root.join("custom/live.ts"), "export default () => {};").unwrap();
        std::fs::write(root.join("custom/dead.ts"), "export const dead = 1;").unwrap();
        assert_eq!(
            unused_files(root),
            ["custom/dead.ts"],
            "extension: {extension}"
        );
    }
}

#[test]
fn config_callbacks_do_not_credit_unreachable_or_uninvoked_array_returns() {
    for tool in ["tsup", "tsdown"] {
        for (source, expected) in [
            (
                "export default () => { return { entry: 'custom/live.ts' }; return [{ entry: 'custom/dead.ts' }]; }",
                &["custom/dead.ts"][..],
            ),
            (
                "export default function* () { return [{ entry: 'custom/dead.ts' }]; }",
                &["custom/dead.ts", "custom/live.ts"][..],
            ),
            (
                "export default async function* () { return [{ entry: 'custom/dead.ts' }]; }",
                &["custom/dead.ts", "custom/live.ts"][..],
            ),
            (
                "export default () => () => [{ entry: 'custom/dead.ts' }]",
                &["custom/dead.ts", "custom/live.ts"][..],
            ),
        ] {
            let temp = tempfile::tempdir().unwrap();
            let root = temp.path();
            std::fs::create_dir(root.join("custom")).unwrap();
            std::fs::write(
                root.join("package.json"),
                format!(
                    "{{\"name\":\"callback-fixture\",\"devDependencies\":{{\"{tool}\":\"*\"}}}}"
                ),
            )
            .unwrap();
            std::fs::write(root.join(format!("{tool}.config.ts")), source).unwrap();
            std::fs::write(root.join("custom/live.ts"), "export const live = 1;").unwrap();
            std::fs::write(root.join("custom/dead.ts"), "export const dead = 1;").unwrap();
            assert_eq!(unused_files(root), expected, "{tool}: {source}");
        }
    }
}
