//! The programmatic trace, which the MCP `trace_export` and `trace_file`
//! tools use, names the plugin that marks an export as used, the same as
//! `fallow dead-code --trace`.

#![expect(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests and benches use unwrap and expect to keep fixture setup concise"
)]

use std::fs;
use std::path::Path;

use fallow_api::{
    AnalysisOptions, TraceExportOptions, TraceFileOptions, run_trace_export, run_trace_file,
};

fn next_project() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    fs::write(
        root.join("package.json"),
        r#"{ "name": "app", "private": true, "dependencies": { "next": "^15.0.0" } }"#,
    )
    .unwrap();
    fs::create_dir_all(root.join("app")).unwrap();
    fs::write(
        root.join("app/page.tsx"),
        "export default function Page(): string {\n  return 'page';\n}\n\nexport const helper = 1;\n",
    )
    .unwrap();
    dir
}

fn analysis(root: &Path) -> AnalysisOptions {
    AnalysisOptions {
        root: Some(root.to_path_buf()),
        no_cache: true,
        ..AnalysisOptions::default()
    }
}

#[test]
fn run_trace_export_names_the_plugin_credit() {
    let dir = next_project();
    let trace = |name: &str| {
        run_trace_export(&TraceExportOptions {
            analysis: analysis(dir.path()),
            file: "app/page.tsx".to_string(),
            export_name: name.to_string(),
        })
        .expect("export trace")
    };

    let page = trace("default");
    let page = page.as_export().expect("export trace");
    assert!(page.is_used);
    assert_eq!(page.plugin_credits[0].plugin, "nextjs");

    let helper = trace("helper");
    let helper = helper.as_export().expect("export trace");
    assert!(!helper.is_used);
    assert!(helper.plugin_credits.is_empty());
}

#[test]
fn run_trace_file_names_the_plugin_credit_of_each_export() {
    let dir = next_project();
    let file = run_trace_file(&TraceFileOptions {
        analysis: analysis(dir.path()),
        file: "app/page.tsx".to_string(),
    })
    .expect("file trace");
    let credits = |name: &str| {
        file.output
            .exports
            .iter()
            .find(|export| export.name == name)
            .expect("export")
            .plugin_credits
            .len()
    };
    assert_eq!(credits("default"), 1);
    assert_eq!(credits("helper"), 0);
}
