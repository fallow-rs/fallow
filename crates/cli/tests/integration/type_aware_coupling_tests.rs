#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests and benches use unwrap and expect to keep fixture setup concise"
)]

//! A project-wide type-coupling report must not become partial only because it
//! has more edges than one per-symbol result may carry (#3563).

use std::fmt::Write as _;

use crate::common::{parse_json, run_fallow_raw_with_type_aware_sidecar};

/// More coupled modules than the per-symbol evidence limit of 40.
const MODULE_COUNT: usize = 45;

fn write_wide_coupling_project(root: &std::path::Path) {
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(
        root.join("package.json"),
        r#"{"name":"wide-coupling","private":true,"exports":{".":"./src/index.ts"}}"#,
    )
    .unwrap();
    std::fs::write(
        root.join("tsconfig.json"),
        r#"{"compilerOptions":{"strict":true},"include":["src/**/*.ts"]}"#,
    )
    .unwrap();
    std::fs::write(
        root.join("src/model.ts"),
        "export interface Model { value: string }\n",
    )
    .unwrap();
    let mut index = String::new();
    for module in 0..MODULE_COUNT {
        std::fs::write(
            root.join(format!("src/api{module}.ts")),
            format!(
                "import type {{ Model }} from \"./model\";\nexport interface Api{module} {{ model: Model }}\n"
            ),
        )
        .unwrap();
        writeln!(
            index,
            "export type {{ Api{module} }} from \"./api{module}\";"
        )
        .unwrap();
    }
    std::fs::write(root.join("src/index.ts"), index).unwrap();
}

#[test]
fn health_type_coupling_reports_every_edge_under_require_complete() {
    let dir = tempfile::tempdir().expect("temp project");
    write_wide_coupling_project(dir.path());
    let root = dir.path().to_string_lossy().to_string();

    let output = run_fallow_raw_with_type_aware_sidecar(&[
        "health",
        "--root",
        &root,
        "--type-aware",
        "--type-coupling",
        "--type-aware-require",
        "complete",
        "--format",
        "json",
        "--quiet",
    ]);

    assert_eq!(
        output.code, 0,
        "stdout: {}\nstderr: {}",
        output.stdout, output.stderr
    );
    let json = parse_json(&output);
    let coupling = &json["_meta"]["type_aware"]["type_coupling"];
    assert_eq!(coupling["status"], "complete", "coupling: {coupling}");
    assert!(
        coupling["omissions"]
            .as_array()
            .is_none_or(std::vec::Vec::is_empty),
        "coupling: {coupling}"
    );
    let summary = &coupling["summary"];
    assert!(
        summary["edge_count"].as_u64().expect("edge_count") > 40,
        "summary: {summary}"
    );
    let files = coupling["files"].as_array().expect("files");
    assert_eq!(
        files.len() as u64,
        summary["distinct_coupled_files"]
            .as_u64()
            .expect("distinct_coupled_files"),
        "summary: {summary}"
    );
}
