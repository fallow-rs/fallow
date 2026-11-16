//! `circularDependencies.allow` (issue #4623): an entry accepts one exact
//! cycle, and an entry that matches no cycle reaches every output as a
//! `circular-dependencies-allow-unmatched` diagnostic.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests use unwrap and expect to keep fixture setup concise"
)]

use crate::common::{parse_json, run_fallow_in_root};

const UNMATCHED_KIND: &str = "circular-dependencies-allow-unmatched";

/// The project of issue #4623: `a -> b -> a` and `a -> b -> c -> a`.
fn cycle_project(allow: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("temporary project");
    let root = dir.path();
    std::fs::create_dir_all(root.join("src")).expect("create src");
    std::fs::write(
        root.join("package.json"),
        r#"{"name":"cyc","private":true,"type":"module","main":"src/index.mjs"}"#,
    )
    .expect("write package");
    let files = [
        (
            "src/index.mjs",
            "import { a } from './a.mjs';\nconsole.log(a());\n",
        ),
        (
            "src/a.mjs",
            "import { b } from './b.mjs';\nexport function a() { return b(); }\nexport function a2() { return 2; }\n",
        ),
        (
            "src/b.mjs",
            "import { a2 } from './a.mjs';\nimport { c } from './c.mjs';\nexport function b() { return a2() + c(); }\n",
        ),
        (
            "src/c.mjs",
            "import { a2 } from './a.mjs';\nexport function c() { return a2(); }\n",
        ),
    ];
    for (path, source) in files {
        std::fs::write(root.join(path), source).expect("write source");
    }
    std::fs::write(
        root.join(".fallowrc.json"),
        format!(r#"{{"circularDependencies": {{"allow": {allow}}}}}"#),
    )
    .expect("write config");
    dir
}

fn cycle_files(json: &serde_json::Value) -> Vec<Vec<String>> {
    json["circular_dependencies"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|finding| {
            finding["files"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|file| file.as_str().map(str::to_owned))
                .collect()
        })
        .collect()
}

fn unmatched_entries(json: &serde_json::Value) -> Vec<serde_json::Value> {
    json["workspace_diagnostics"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|diagnostic| diagnostic["kind"] == UNMATCHED_KIND)
        .cloned()
        .collect()
}

#[test]
fn an_accepted_cycle_hides_only_that_cycle() {
    let dir = cycle_project(r#"[["src/a.mjs", "src/b.mjs"]]"#);
    let json = parse_json(&run_fallow_in_root(
        "dead-code",
        dir.path(),
        &["--format", "json", "--quiet"],
    ));
    assert_eq!(
        cycle_files(&json),
        vec![vec!["src/a.mjs", "src/b.mjs", "src/c.mjs"]],
        "{json}"
    );
    assert!(unmatched_entries(&json).is_empty(), "{json}");
}

#[test]
fn an_entry_that_matches_no_cycle_reaches_every_output() {
    let allow = r#"[["src/a.mjs", "src/b.mjs"], ["src/a.mjs", "src/gone.mjs"]]"#;
    let dir = cycle_project(allow);
    let json = parse_json(&run_fallow_in_root(
        "dead-code",
        dir.path(),
        &["--format", "json", "--quiet"],
    ));
    let entries = unmatched_entries(&json);
    assert_eq!(entries.len(), 1, "{json}");
    assert_eq!(entries[0]["path"], ".");
    assert_eq!(entries[0]["pattern"], "[src/a.mjs, src/gone.mjs]");
    assert_eq!(
        entries[0]["files"],
        serde_json::json!(["src/a.mjs", "src/gone.mjs"])
    );

    let human = run_fallow_in_root("dead-code", dir.path(), &[]);
    assert!(
        human.stderr.contains(
            "Note: circularDependencies.allow entry matched no cycle this run: [src/a.mjs, \
             src/gone.mjs]"
        ),
        "{}",
        human.stderr
    );

    let sarif = parse_json(&run_fallow_in_root(
        "dead-code",
        dir.path(),
        &["--format", "sarif", "--quiet"],
    ));
    let settings: Vec<&str> = sarif["runs"][0]["invocations"][0]["toolConfigurationNotifications"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|notification| notification["properties"]["setting"].as_str())
        .collect();
    assert_eq!(settings, vec!["circularDependencies.allow"], "{sarif}");

    let markdown = run_fallow_in_root("dead-code", dir.path(), &["--format", "markdown"]);
    assert!(
        markdown
            .stdout
            .contains("- `circularDependencies.allow`: `[src/a.mjs, src/gone.mjs]` matched nothing in this run"),
        "{}",
        markdown.stdout
    );
}

#[test]
fn no_entry_is_unmatched_when_the_rule_is_off_or_the_run_is_production() {
    let dir = cycle_project(r#"[["src/a.mjs", "src/gone.mjs"]]"#);
    let production = parse_json(&run_fallow_in_root(
        "dead-code",
        dir.path(),
        &["--production", "--format", "json", "--quiet"],
    ));
    assert!(unmatched_entries(&production).is_empty(), "{production}");

    std::fs::write(
        dir.path().join(".fallowrc.json"),
        r#"{"rules": {"circular-dependencies": "off"},
            "circularDependencies": {"allow": [["src/a.mjs", "src/gone.mjs"]]}}"#,
    )
    .expect("write config");
    let off = parse_json(&run_fallow_in_root(
        "dead-code",
        dir.path(),
        &["--format", "json", "--quiet"],
    ));
    assert!(unmatched_entries(&off).is_empty(), "{off}");
}
