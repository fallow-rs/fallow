#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests use unwrap and expect to keep fixture setup concise"
)]

//! The bare `fallow` command and `fallow_api::run_combined` compute health
//! from the same duplication data. Health must be identical on both paths.

use std::fmt::Write as _;
use std::fs;
use std::path::Path;

use crate::common::{parse_json, run_fallow_raw};
use tempfile::{TempDir, tempdir};

fn write_file(path: &Path, contents: &str) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("create parent directories");
    }
    fs::write(path, contents).expect("write file");
}

/// A function body long enough to pass the default clone thresholds.
fn duplicated_function(name: &str) -> String {
    let mut body = format!("export function {name}(items: number[]): number {{\n");
    body.push_str("  let total = 0;\n");
    for index in 0..12 {
        writeln!(
            body,
            "  if (items[{index}] > {index}) {{ total += items[{index}] * {index}; }}"
        )
        .unwrap();
    }
    body.push_str("  return total;\n}\n");
    body
}

/// Two source files share a clone. A third file has many lines, and the
/// `duplicates.ignore` config excludes it from duplicate detection. Health
/// must compute the duplication percentage the same way as a detection over
/// its own files, where the ignored file does not count.
fn create_project() -> TempDir {
    let dir = tempdir().unwrap();
    let root = dir.path();
    write_file(
        &root.join("package.json"),
        r#"{"name":"combined-health-duplication","type":"module","main":"src/index.ts"}"#,
    );
    write_file(
        &root.join("src/index.ts"),
        "export { first } from './first';\nexport { second } from './second';\nexport { table } from '../generated/table';\n",
    );
    write_file(&root.join("src/first.ts"), &duplicated_function("first"));
    write_file(&root.join("src/second.ts"), &duplicated_function("second"));
    let mut table = String::from("export const table = [\n");
    for index in 0..400 {
        writeln!(table, "  {{ id: {index}, label: 'row {index}' }},").unwrap();
    }
    table.push_str("];\n");
    write_file(&root.join("generated/table.ts"), &table);
    write_file(
        &root.join(".fallowrc.json"),
        r#"{"duplicates":{"ignore":["generated/**"]}}"#,
    );
    dir
}

/// Remove the fields that change between two runs of the same analysis.
fn without_volatile_fields(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::Object(map) => {
            map.remove("elapsed_ms");
            map.remove("analysis_run_id");
            map.values_mut().for_each(without_volatile_fields);
        }
        serde_json::Value::Array(items) => items.iter_mut().for_each(without_volatile_fields),
        _ => {}
    }
}

fn health_section(mut envelope: serde_json::Value) -> serde_json::Value {
    let mut health = envelope
        .get_mut("health")
        .map(serde_json::Value::take)
        .expect("combined output has a health section");
    without_volatile_fields(&mut health);
    health
}

#[test]
fn bare_command_and_api_combined_runner_report_the_same_health() {
    let project = create_project();
    let root = project.path();

    let cli = run_fallow_raw(&[
        "--root",
        root.to_str().unwrap(),
        "--format",
        "json",
        "--quiet",
        "--no-cache",
        "--score",
    ]);
    let cli_health = health_section(parse_json(&cli));

    let api = fallow_api::run_combined(&fallow_api::CombinedOptions {
        analysis: fallow_api::AnalysisOptions {
            root: Some(root.to_path_buf()),
            no_cache: true,
            ..fallow_api::AnalysisOptions::default()
        },
        // The sections of the bare command with `--score`.
        health_options: fallow_api::ComplexityOptions {
            complexity: true,
            file_scores: true,
            hotspots: true,
            targets: true,
            score: true,
            ..fallow_api::ComplexityOptions::default()
        },
        ..fallow_api::CombinedOptions::default()
    })
    .expect("api combined run");
    let api_health =
        health_section(fallow_api::serialize_combined_programmatic_json(api).expect("serialize"));

    let duplication = &cli_health["vital_signs"]["duplication_pct"];
    assert!(
        duplication.as_f64().is_some_and(|pct| pct > 0.0),
        "the fixture must have duplication, got {duplication}"
    );
    assert_eq!(
        serde_json::to_string_pretty(&cli_health).unwrap(),
        serde_json::to_string_pretty(&api_health).unwrap(),
        "bare fallow and the API combined runner must report the same health"
    );

    // Standalone `fallow health` runs its own duplicate detection. The shared
    // report must give the same duplication penalty and score.
    let standalone = parse_json(&run_fallow_raw(&[
        "health",
        "--root",
        root.to_str().unwrap(),
        "--format",
        "json",
        "--quiet",
        "--no-cache",
        "--score",
    ]));
    for field in ["/score", "/penalties/duplication"] {
        assert_eq!(
            cli_health["health_score"].pointer(field),
            standalone["health_score"].pointer(field),
            "health_score{field} of bare fallow must match standalone fallow health"
        );
    }
}
