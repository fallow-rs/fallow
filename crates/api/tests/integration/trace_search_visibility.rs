//! `run_trace_export` carries the literal-search fields on each direct
//! reference, so the MCP `trace_export` tool and Code Mode return them.

#![expect(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests and benches use unwrap and expect to keep fixture setup concise"
)]

use std::fs;

use fallow_api::{
    AnalysisOptions, TraceExportOptions, run_trace_export, serialize_trace_export_programmatic_json,
};

#[test]
fn run_trace_export_reports_search_visibility_per_reference() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    fs::write(
        root.join("package.json"),
        r#"{"name":"trace-search-fixture","version":"0.0.0","main":"src/index.ts"}"#,
    )
    .unwrap();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(
        root.join("src/utils.ts"),
        "export function formatDate() { return 1; }\n",
    )
    .unwrap();
    fs::write(
        root.join("src/index.ts"),
        "import { formatDate as fmt } from \"./utils\";\nfmt();\n",
    )
    .unwrap();

    let out = run_trace_export(&TraceExportOptions {
        analysis: AnalysisOptions {
            root: Some(root.to_path_buf()),
            no_cache: true,
            ..AnalysisOptions::default()
        },
        file: "src/utils.ts".to_string(),
        export_name: "formatDate".to_string(),
    })
    .expect("export trace should resolve");
    let json = serialize_trace_export_programmatic_json(out).expect("serialize export trace");

    let references = json["direct_references"]
        .as_array()
        .expect("direct_references array");
    assert_eq!(references.len(), 1, "{json}");
    let reference = &references[0];
    assert_eq!(reference["from_file"], "src/index.ts");
    assert_eq!(reference["line"], 1);
    assert_eq!(reference["search_visibility"], "renamed");
    assert_eq!(reference["reason"], "import-alias");
    assert_eq!(reference["search_as"], "fmt");
}
