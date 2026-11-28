#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests and benches use unwrap and expect to keep fixture setup concise"
)]

//! YAML aliases in a plugin config: a large config with merged defaults is
//! read in full, and a config whose aliases expand above the memory budget
//! is skipped with a warning that names it.

use std::fmt::Write as _;
use std::path::Path;

use crate::common::{parse_json, run_fallow_raw_with_env};

const FUNCTIONS: usize = 250;

fn write_project(root: &Path, serverless_yml: &str) {
    std::fs::write(
        root.join("package.json"),
        r#"{"name": "api", "main": "src/index.js", "devDependencies": {"serverless": "^3.0.0"}}"#,
    )
    .unwrap();
    std::fs::create_dir_all(root.join("src/handlers")).unwrap();
    std::fs::write(root.join("src/index.js"), "export const start = 1;\n").unwrap();
    std::fs::write(root.join("serverless.yml"), serverless_yml).unwrap();
}

/// Run dead-code analysis with warnings on, as a user run shows them.
fn dead_code(root: &Path) -> crate::common::CommandOutput {
    let root = root.to_str().unwrap();
    run_fallow_raw_with_env(
        &[
            "--root",
            root,
            "dead-code",
            "--format",
            "json",
            "--no-cache",
        ],
        &[("RUST_LOG", "warn")],
    )
}

#[test]
fn every_handler_of_a_large_config_with_merged_defaults_is_an_entry() {
    let dir = tempfile::tempdir().unwrap();
    let mut yaml = String::from("service: api\ncustom:\n  defaults: &defaults\n");
    for key in 0..15 {
        writeln!(yaml, "    setting{key}: value-of-the-shared-setting-{key}").unwrap();
    }
    yaml.push_str("functions:\n");
    for function in 0..FUNCTIONS {
        write!(
            yaml,
            "  function{function}:\n    <<: *defaults\n    handler: src/handlers/function{function}.main\n    events:\n      - http: {{ path: /function{function}, method: get }}\n"
        )
        .unwrap();
    }
    write_project(dir.path(), &yaml);
    for function in 0..FUNCTIONS {
        std::fs::write(
            dir.path()
                .join(format!("src/handlers/function{function}.js")),
            "export const main = () => 1;\n",
        )
        .unwrap();
    }

    let output = dead_code(dir.path());
    let json = parse_json(&output);
    let unused = json["unused_files"].as_array().unwrap();
    assert!(unused.is_empty(), "{unused:?}");
    assert!(
        !output.stderr.contains("YAML file skipped"),
        "{}",
        output.stderr
    );
}

#[test]
fn a_config_whose_aliases_expand_above_the_budget_is_skipped_with_a_warning() {
    let dir = tempfile::tempdir().unwrap();
    // 100,000 copies of a 4,000-byte string: about 400 MB from 4 KB.
    let mut yaml = format!("service: api\ns0: &s0 \"{}\"\n", "x".repeat(4_000));
    for level in 1..=5 {
        let items = vec![format!("*s{}", level - 1); 10].join(", ");
        writeln!(yaml, "s{level}: &s{level} [{items}]").unwrap();
    }
    write_project(dir.path(), &yaml);

    let output = dead_code(dir.path());
    assert!(
        output.stderr.contains(
            "config{path=serverless.yml}: YAML file skipped: aliases expand to more than 256 MiB"
        ),
        "{}",
        output.stderr
    );
}
