//! A private class method takes its coverage from its own record in native V8
//! coverage, the same as its public twin (#3816).

use std::path::Path;

use crate::common::{parse_json, run_fallow_in_root};

/// The fixture from the #3816 report: a private method and a public method
/// with the same body. The loop runs every return of both methods.
const TWIN_SOURCE: &str = "import assert from 'node:assert/strict'
class Twin {
  #privateMethod(value){
    if(value===0)return 10
    if(value===1)return 20
    if(value===2)return 30
    if(value===3)return 40
    if(value===4)return 50
    if(value===5)return 60
    return 70
  }
  publicMethod(value){
    if(value===0)return 10
    if(value===1)return 20
    if(value===2)return 30
    if(value===3)return 40
    if(value===4)return 50
    if(value===5)return 60
    return 70
  }
  callPrivate(value){return this.#privateMethod(value)}
}
const twin=new Twin()
for(let value=0;value<=6;value++){
  assert.equal(twin.callPrivate(value),(value+1)*10)
  assert.equal(twin.publicMethod(value),(value+1)*10)
}
";

const PRIVATE_NAME: &str = "#privateMethod";
const PUBLIC_NAME: &str = "publicMethod";
const CALL_COUNT: u32 = 7;

fn twin_project() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("temp dir");
    std::fs::write(
        dir.path().join(".fallowrc.json"),
        r#"{"entry":["fixture.mjs"]}"#,
    )
    .expect("write config");
    std::fs::write(dir.path().join("fixture.mjs"), TWIN_SOURCE).expect("write fixture");
    dir
}

/// Run `fallow health` on `root` with the V8 directory `coverage`.
fn health_with_v8(root: &Path, coverage: &str, max_crap: &str) -> serde_json::Value {
    let output = run_fallow_in_root(
        "health",
        root,
        &[
            "--complexity",
            "--coverage",
            coverage,
            "--max-crap",
            max_crap,
            "--format",
            "json",
            "--quiet",
        ],
    );
    parse_json(&output)
}

fn finding<'a>(json: &'a serde_json::Value, name: &str) -> Option<&'a serde_json::Value> {
    json["findings"]
        .as_array()?
        .iter()
        .find(|finding| finding["name"] == name)
}

/// With a threshold that every unit exceeds, both twins are findings with
/// measured coverage. With the default threshold, neither twin is a finding.
fn assert_twins_measured(root: &Path, coverage: &str) {
    let all = health_with_v8(root, coverage, "1");
    let summary = &all["summary"];
    assert_eq!(
        summary["istanbul_matched"], summary["istanbul_total"],
        "{all:#}"
    );
    for name in [PRIVATE_NAME, PUBLIC_NAME] {
        let unit = finding(&all, name).unwrap_or_else(|| panic!("expected {name}: {all:#}"));
        assert_eq!(unit["coverage_source"], "istanbul", "{name}: {unit:#}");
        assert_eq!(
            unit["coverage_pct"].as_f64(),
            Some(100.0),
            "{name}: {unit:#}"
        );
    }

    let default_gate = health_with_v8(root, coverage, "30");
    assert!(default_gate["findings"].is_array(), "{default_gate:#}");
    for name in [PRIVATE_NAME, PUBLIC_NAME] {
        assert!(
            finding(&default_gate, name).is_none(),
            "{name} has full coverage and needs no CRAP finding: {default_gate:#}"
        );
    }
}

fn function_entry(name: &str, start: usize, end: usize, count: u32) -> serde_json::Value {
    serde_json::json!({
        "functionName": name,
        "isBlockCoverage": false,
        "ranges": [{ "startOffset": start, "endOffset": end, "count": count }]
    })
}

/// The byte range of the method that starts at `head` and ends at the first
/// `\n  }` after it.
fn method_range(head: &str) -> (usize, usize) {
    let start = TWIN_SOURCE.find(head).expect("method in fixture");
    let end = start + TWIN_SOURCE[start..].find("\n  }").expect("method end") + "\n  }".len();
    (start, end)
}

#[test]
fn v8_coverage_maps_an_executed_private_method_like_its_public_twin() {
    let project = twin_project();
    let fixture = dunce::canonicalize(project.path().join("fixture.mjs")).expect("canonical");
    let (private_start, private_end) = method_range("#privateMethod(value){");
    let (public_start, public_end) = method_range("publicMethod(value){");
    let call_start = TWIN_SOURCE.find("callPrivate(value)").expect("call method");
    let call_end = call_start + TWIN_SOURCE[call_start..].find('}').expect("call end") + 1;
    let script = serde_json::json!({
        "url": url::Url::from_file_path(&fixture).expect("file url").to_string(),
        "functions": [
            function_entry("", 0, TWIN_SOURCE.len(), 1),
            function_entry(PRIVATE_NAME, private_start, private_end, CALL_COUNT),
            function_entry(PUBLIC_NAME, public_start, public_end, CALL_COUNT),
            function_entry("callPrivate", call_start, call_end, CALL_COUNT),
        ]
    });
    let coverage = project.path().join("v8");
    std::fs::create_dir_all(&coverage).expect("coverage dir");
    std::fs::write(
        coverage.join("coverage-1.json"),
        serde_json::json!({ "result": [script] }).to_string(),
    )
    .expect("write coverage");

    assert_twins_measured(project.path(), "v8");
}

#[test]
#[expect(
    clippy::print_stderr,
    reason = "a skipped test says why it did not run"
)]
fn native_node_v8_coverage_maps_an_executed_private_method() {
    let has_node = std::process::Command::new("node")
        .arg("--version")
        .output()
        .is_ok_and(|output| output.status.success());
    if !has_node {
        eprintln!("skipped: the native V8 coverage test needs `node` on PATH");
        return;
    }
    let project = twin_project();
    run_with_v8_coverage(project.path(), "fixture.mjs");

    assert_twins_measured(project.path(), "coverage");
}

/// Run `script` in `root` with Node, and write V8 coverage to `root/coverage`.
fn run_with_v8_coverage(root: &Path, script: &str) {
    let status = std::process::Command::new("node")
        .arg(script)
        .current_dir(root)
        .env("NODE_V8_COVERAGE", root.join("coverage"))
        .status()
        .expect("run node");
    assert!(status.success(), "the fixture must run without errors");
}

/// Private members that ran in part, or not at all. Each one keeps its own
/// count: an uncalled method stays at zero, and a getter and setter with the
/// same `#name` each take their own record.
const PRIVATE_KINDS_SOURCE: &str = "class Kinds {
  #state = 0
  #neverCalled(x){
    if(x)return 1
    return 2
  }
  get #value(){
    if(this.#state)return 1
    return 2
  }
  set #value(x){
    if(x)this.#state=1
    else this.#state=2
  }
  static #pick(x){
    if(x)return 1
    return 2
  }
  run(){ this.#value = 1; return this.#value + Kinds.#pick(1) }
}
new Kinds().run()
";

#[test]
#[expect(
    clippy::print_stderr,
    reason = "a skipped test says why it did not run"
)]
fn native_node_v8_coverage_keeps_each_private_member_to_its_own_record() {
    let has_node = std::process::Command::new("node")
        .arg("--version")
        .output()
        .is_ok_and(|output| output.status.success());
    if !has_node {
        eprintln!("skipped: the native V8 coverage test needs `node` on PATH");
        return;
    }
    let project = tempfile::tempdir().expect("temp dir");
    let root = project.path();
    std::fs::write(root.join(".fallowrc.json"), r#"{"entry":["kinds.mjs"]}"#)
        .expect("write config");
    std::fs::write(root.join("kinds.mjs"), PRIVATE_KINDS_SOURCE).expect("write fixture");
    run_with_v8_coverage(root, "kinds.mjs");

    let json = health_with_v8(root, "coverage", "1");
    let measured: Vec<(String, u64, f64)> = json["findings"]
        .as_array()
        .expect("findings array")
        .iter()
        .filter(|finding| finding["coverage_source"] == "istanbul")
        .map(|finding| {
            (
                finding["name"].as_str().expect("name").to_string(),
                finding["line"].as_u64().expect("line"),
                finding["coverage_pct"].as_f64().expect("coverage pct"),
            )
        })
        .collect();
    let two_of_three = 200.0 / 3.0;
    let expected = [
        ("#neverCalled", 3, 0.0),
        ("#value", 7, two_of_three),
        ("#value", 11, two_of_three),
        ("#pick", 15, two_of_three),
    ];
    for (name, line, pct) in expected {
        assert!(
            measured
                .iter()
                .any(|(n, l, p)| n == name && *l == line && (p - pct).abs() < 0.01),
            "expected {name} at line {line} with {pct}%: {json:#}"
        );
    }
}
