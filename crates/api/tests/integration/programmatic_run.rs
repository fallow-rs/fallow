//! Each typed runner reads and validates the project config once for each
//! call, with the same validation on each route.

#![expect(
    clippy::expect_used,
    reason = "tests use expect to keep fixture setup concise"
)]

use std::path::{Path, PathBuf};

use fallow_api::{
    AnalysisOptions, CombinedOptions, ComplexityOptions, FeatureFlagsOptions, run_combined,
    run_feature_flags, run_health, serialize_combined_programmatic_json,
};
use fallow_engine::project_config::load_count::config_loads;

use crate::common::write;

/// An external plugin with an entry pattern outside the project root.
const TRAVERSAL_PLUGIN: &str = r#"{ "name": "escape", "entryPoints": ["../outside/**"] }"#;

fn project(dir: &Path, config: &str) -> PathBuf {
    let root = dir.join("project");
    std::fs::create_dir_all(root.join(".git")).expect("git marker");
    let root = dunce::canonicalize(&root).expect("canonical root");
    write(
        &root,
        "package.json",
        r#"{"name":"programmatic-run","main":"src/index.ts"}"#,
    );
    write(&root, "src/index.ts", "export const value = 1;\n");
    write(&root, ".fallowrc.json", config);
    root
}

fn analysis(root: &Path) -> AnalysisOptions {
    AnalysisOptions {
        root: Some(root.to_path_buf()),
        no_cache: true,
        threads: Some(1),
        ..AnalysisOptions::default()
    }
}

#[test]
fn feature_flags_reject_an_invalid_external_plugin() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = project(dir.path(), "{}");
    write(&root, ".fallow/plugins/escape.json", TRAVERSAL_PLUGIN);
    let err = run_feature_flags(&FeatureFlagsOptions {
        analysis: analysis(&root),
        ..FeatureFlagsOptions::default()
    })
    .expect_err("the plugin is rejected");
    assert_eq!(
        err.code.as_deref(),
        Some("FALLOW_CONFIG_LOAD_FAILED"),
        "{}",
        err.message
    );
    assert_eq!(err.context.as_deref(), Some("analysis.configPath"));
    assert!(
        err.message.contains("invalid external plugin definition"),
        "{}",
        err.message
    );
}

/// The three sections run in three production modes split over two mode
/// entries, so the old runner loaded the config for each section and for the
/// plan and the production modes.
#[test]
fn programmatic_call_loads_the_config_once() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = project(dir.path(), r#"{"production": {"deadCode": true}}"#);
    let before = config_loads(&root);
    run_combined(&CombinedOptions {
        analysis: analysis(&root),
        health_options: ComplexityOptions {
            complexity: true,
            ..ComplexityOptions::default()
        },
        ..CombinedOptions::default()
    })
    .expect("combined runs");
    assert_eq!(config_loads(&root) - before, 1);
}

/// A health call with no section also checks the type-aware layers of the
/// config, and it reads them from the config of its run.
#[test]
fn a_health_call_loads_the_config_once() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = project(dir.path(), "{}");
    for complexity in [false, true] {
        let before = config_loads(&root);
        run_health(&ComplexityOptions {
            analysis: analysis(&root),
            complexity,
            ..ComplexityOptions::default()
        })
        .expect("health runs");
        assert_eq!(config_loads(&root) - before, 1, "complexity {complexity}");
    }
}

#[test]
fn a_combined_call_lists_each_skipped_analysis_with_its_reason() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = project(dir.path(), r#"{"duplicates": {"enabled": false}}"#);
    let output = run_combined(&CombinedOptions {
        analysis: analysis(&root),
        health: false,
        ..CombinedOptions::default()
    })
    .expect("combined runs");
    assert!(output.duplication.is_none());
    let json = serialize_combined_programmatic_json(output).expect("json");
    assert_eq!(
        json["skipped"],
        serde_json::json!([
            {
                "analysis": "duplication",
                "reason": "off-by-config",
                "config_key": "duplicates.enabled"
            },
            { "analysis": "health", "reason": "not-requested" }
        ])
    );

    let dir = tempfile::tempdir().expect("tempdir");
    let root = project(dir.path(), "{}");
    let json = serialize_combined_programmatic_json(
        run_combined(&CombinedOptions {
            analysis: analysis(&root),
            ..CombinedOptions::default()
        })
        .expect("combined runs"),
    )
    .expect("json");
    assert!(json.get("skipped").is_none(), "{json:#}");
}
