//! End-to-end coverage for the `run_trace_export` class-member fallback that
//! gives the MCP `trace_export` tool and Code Mode parity with the CLI's
//! `--trace FILE:MEMBER` behavior. See issue #1744.

#![expect(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests and benches use unwrap and expect to keep fixture setup concise"
)]

use std::fs;
use std::path::Path;

use fallow_api::{
    AnalysisOptions, TraceExportOptions, run_trace_export, serialize_trace_export_programmatic_json,
};

fn write_fixture() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    fs::write(
        root.join("package.json"),
        r#"{"name":"trace-member-fixture","version":"0.0.0","main":"src/index.ts"}"#,
    )
    .unwrap();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(
        root.join("src/controller.ts"),
        "export class Ctrl {\n  used() { return 1; }\n  dead() { return 2; }\n}\n",
    )
    .unwrap();
    fs::write(
        root.join("src/consumer.ts"),
        "import { Ctrl } from \"./controller\";\nconst c = new Ctrl();\nexport function run() { return c.used(); }\n",
    )
    .unwrap();
    fs::write(
        root.join("src/index.ts"),
        "import { run } from \"./consumer\";\nrun();\n",
    )
    .unwrap();
    dir
}

fn opts(root: &Path, name: &str) -> TraceExportOptions {
    TraceExportOptions {
        analysis: AnalysisOptions {
            root: Some(root.to_path_buf()),
            no_cache: true,
            ..AnalysisOptions::default()
        },
        file: "src/controller.ts".to_string(),
        export_name: name.to_string(),
    }
}

#[test]
fn run_trace_export_falls_back_to_member_trace() {
    let dir = write_fixture();

    // A class MEMBER name (not a top-level export) resolves to a member trace
    // instead of a hard "not found" error.
    let out = run_trace_export(&opts(dir.path(), "dead")).expect("member trace should resolve");
    let member = out.as_member().expect("expected the Member variant");
    assert_eq!(member.owner_export, "Ctrl");
    assert_eq!(member.member_name, "dead");
    assert_eq!(member.member_kind, "class-method");
    assert!(out.as_export().is_none());

    // The wire shape is the flat member trace (member_name + owner_export, no
    // export_name), matching the CLI and distinguishable by field presence.
    let json = serialize_trace_export_programmatic_json(
        run_trace_export(&opts(dir.path(), "dead")).unwrap(),
    )
    .expect("serialize member trace");
    assert_eq!(json["member_name"], "dead");
    assert_eq!(json["owner_export"], "Ctrl");
    assert!(json.get("export_name").is_none());
}

#[test]
fn run_trace_export_still_traces_a_real_export_byte_compatibly() {
    let dir = write_fixture();

    let out = run_trace_export(&opts(dir.path(), "Ctrl")).expect("export trace should resolve");
    assert!(out.as_export().is_some());
    assert!(out.as_member().is_none());

    // The export shape stays flat (export_name present, member_name absent), so
    // the untagged enum does not change the historical export contract.
    let json = serialize_trace_export_programmatic_json(
        run_trace_export(&opts(dir.path(), "Ctrl")).unwrap(),
    )
    .expect("serialize export trace");
    assert_eq!(json["export_name"], "Ctrl");
    assert!(json.get("member_name").is_none());
}

#[test]
fn run_trace_export_absent_name_errors_with_export_or_member() {
    let dir = write_fixture();

    let err = run_trace_export(&opts(dir.path(), "doesNotExist"))
        .expect_err("a name that is neither an export nor a member must error");
    assert_eq!(err.code.as_deref(), Some("FALLOW_TRACE_TARGET_NOT_FOUND"));
    assert!(
        err.message.contains("export or member"),
        "error should name both shapes: {}",
        err.message
    );
}
