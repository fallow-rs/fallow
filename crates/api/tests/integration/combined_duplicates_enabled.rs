//! `fallow_api::run_combined` reads `duplicates.enabled` as the bare `fallow`
//! command does. With the key off, the duplication domain does not run and
//! the health score has no duplication measure. A run that selects only the
//! duplication domain names it, so it still runs.

#![expect(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests and benches use unwrap and expect to keep fixture setup concise"
)]

use std::fmt::Write as _;
use std::path::Path;

use fallow_api::{
    AnalysisOptions, CombinedOptions, ComplexityOptions, run_combined,
    serialize_combined_programmatic_json,
};
use serde_json::Value;

use crate::common::write;

/// A function body long enough to form a clone group with a second copy.
fn cloned(name: &str) -> String {
    let mut source = format!("export function {name}(items) {{\n  const out = [];\n");
    for step in 0..12 {
        writeln!(
            source,
            "  out.push(items.map((item) => item.value * {step}).filter((value) => value > {step}));"
        )
        .unwrap();
    }
    source.push_str("  return out;\n}\n");
    source
}

/// A project with one clone group. `config` is the content of
/// `.fallowrc.json`.
fn project(config: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    write(
        root,
        "package.json",
        r#"{"name": "duplicates-enabled", "private": true, "type": "module", "main": "src/index.js"}"#,
    );
    write(root, ".fallowrc.json", config);
    write(
        root,
        "src/index.js",
        "import { first } from './first.js';\nimport { second } from './second.js';\nconsole.log(first, second);\n",
    );
    write(root, "src/first.js", &cloned("first"));
    write(root, "src/second.js", &cloned("second"));
    dir
}

const DISABLED: &str = r#"{"duplicates": {"enabled": false}}"#;

fn combined(root: &Path, selection: CombinedOptions) -> Value {
    run_combined(&CombinedOptions {
        analysis: AnalysisOptions {
            root: Some(root.to_path_buf()),
            no_cache: true,
            ..AnalysisOptions::default()
        },
        health_options: ComplexityOptions {
            complexity: true,
            score: true,
            ..ComplexityOptions::default()
        },
        ..selection
    })
    .and_then(serialize_combined_programmatic_json)
    .expect("run the programmatic combined analysis")
}

fn clone_groups(json: &Value) -> usize {
    json["dupes"]["clone_groups"].as_array().map_or(0, Vec::len)
}

#[test]
fn combined_reports_duplication_by_default() {
    let dir = project("{}");
    let json = combined(dir.path(), CombinedOptions::default());

    assert_eq!(clone_groups(&json), 1, "{json:#}");
    assert!(
        json["health"]["health_score"]["penalties"]["duplication"].is_number(),
        "{json:#}"
    );
}

#[test]
fn combined_skips_duplication_with_duplicates_enabled_false() {
    let dir = project(DISABLED);
    let json = combined(dir.path(), CombinedOptions::default());

    assert!(json.get("dupes").is_none(), "{json:#}");
    assert!(json.get("check").is_some(), "{json:#}");
    assert!(
        json["health"]["health_score"]["score"].is_number(),
        "{json:#}"
    );
    assert!(
        json["health"]["health_score"]["penalties"]
            .get("duplication")
            .is_none(),
        "{json:#}"
    );
}

#[test]
fn combined_runs_a_duplication_only_selection_with_duplicates_enabled_false() {
    let dir = project(DISABLED);
    let json = combined(
        dir.path(),
        CombinedOptions {
            dead_code: false,
            health: false,
            ..CombinedOptions::default()
        },
    );

    assert_eq!(clone_groups(&json), 1, "{json:#}");
}
