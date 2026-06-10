// These tests pin EXACT metric values on purpose. If your change breaks them,
// you have changed scoring behavior that users gate CI on (`--max-crap`, score
// thresholds). Do not casually update the numbers: confirm the shift is
// intended, call it out in the PR description and changelog, and then update
// the pinned values and their arithmetic comments together.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests use unwrap/expect to keep fixture setup concise"
)]

#[path = "common/mod.rs"]
mod common;

use common::{parse_json, run_fallow_in_root};
use std::path::Path;
use tempfile::tempdir;

fn write_file(path: &Path, contents: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("create parent directories");
    }
    std::fs::write(path, contents).expect("write file");
}

/// Build a minimal temp project with the baseline fixture functions and return
/// the parsed JSON findings array for the given cyclomatic/cognitive thresholds.
///
/// Each function in `src/baseline.ts` exercises one construct family.
/// The fixture source is reproduced verbatim in the assertions below;
/// do NOT reformat it -- the metric values were pinned against this exact text.
fn run_baseline_fixture(max_cyclomatic: u16, max_cognitive: u16) -> serde_json::Value {
    let dir = tempdir().unwrap();
    write_file(
        &dir.path().join("package.json"),
        r#"{"name":"health-baseline-fixture","type":"module"}"#,
    );
    write_file(
        &dir.path().join("src/index.ts"),
        r#"export * from "./baseline";"#,
    );
    // IMPORTANT: keep whitespace / line endings exactly as written below.
    // The pinned metric values were measured against this source text.
    //
    // ifChain: 9 sequential top-level `if`s, no nesting.
    //   cyclomatic = 1 (base) + 9 (one per `if`) = 10
    //   cognitive  = 9 x (1 + nesting=0) = 9
    //
    // nestedConditions: 3 levels of nested `if`.
    //   cyclomatic = 1 + 3 = 4
    //   cognitive  = (1+0) + (1+1) + (1+2) = 1 + 2 + 3 = 6
    //
    // switchHeavy: switch with 10 non-default cases.
    //   switch contributes to cognitive (+1 at nesting=0); each case to cyclomatic only.
    //   cyclomatic = 1 + 10 cases = 11
    //   cognitive  = 1 (switch at nesting=0) = 1
    //
    // booleanOperators: two `if` guards with one `&&` and one `||` each.
    //   if1: cyclo+1, cog+1. `a && b`: cyclo+1, cog+1 (first &&).
    //   if2: cyclo+1, cog+1. `c || d`: cyclo+1, cog+1 (first ||).
    //   cyclomatic = 1 + 1(if1) + 1(&&) + 1(if2) + 1(||) = 5
    //   cognitive  = 1(if1) + 1(&&) + 1(if2) + 1(||) = 4
    //
    // loopsAndCatch: for + while (nested in for body) + try/catch.
    //   for at nesting=0: cyclo+1, cog+(1+0)=1; body visited at nesting=1.
    //   while at nesting=1: cyclo+1, cog+(1+1)=2.
    //   catch at nesting=0: cyclo+1, cog+(1+0)=1.
    //   cyclomatic = 1 + 1(for) + 1(while) + 1(catch) = 4
    //   cognitive  = 1(for) + 2(while at nest 1) + 1(catch) = 4
    write_file(
        &dir.path().join("src/baseline.ts"),
        "
export function ifChain(n: number): string {
  if (n === 1) return 'one';
  if (n === 2) return 'two';
  if (n === 3) return 'three';
  if (n === 4) return 'four';
  if (n === 5) return 'five';
  if (n === 6) return 'six';
  if (n === 7) return 'seven';
  if (n === 8) return 'eight';
  if (n === 9) return 'nine';
  return 'other';
}

export function nestedConditions(a: boolean, b: boolean, c: boolean): boolean {
  if (a) {
    if (b) {
      if (c) {
        return c;
      }
    }
  }
  return false;
}

export function switchHeavy(n: number): string {
  switch (n) {
    case 1: return 'one';
    case 2: return 'two';
    case 3: return 'three';
    case 4: return 'four';
    case 5: return 'five';
    case 6: return 'six';
    case 7: return 'seven';
    case 8: return 'eight';
    case 9: return 'nine';
    case 10: return 'ten';
    default: return 'other';
  }
}

export function booleanOperators(
  a: boolean,
  b: boolean,
  c: boolean,
  d: boolean,
): boolean {
  if (a && b) { return true; }
  if (c || d) { return true; }
  return false;
}

export function loopsAndCatch(items: number[]): number {
  let sum = 0;
  for (let i = 0; i < items.length; i++) {
    while (items[i] > 0) {
      items[i]--;
    }
    sum += items[i];
  }
  try {
    return sum;
  } catch (_e) {
    return 0;
  }
}
",
    );

    let output = run_fallow_in_root(
        "health",
        dir.path(),
        &[
            "--complexity",
            "--max-cyclomatic",
            &max_cyclomatic.to_string(),
            "--max-cognitive",
            &max_cognitive.to_string(),
            "--max-crap",
            "100000",
            "--format",
            "json",
            "--quiet",
        ],
    );
    let json = parse_json(&output);
    json.get("findings")
        .and_then(serde_json::Value::as_array)
        .expect("health JSON must contain a findings array")
        .clone()
        .into()
}

/// Find the finding for a specific function name in the findings array.
fn find_fn<'a>(findings: &'a serde_json::Value, name: &str) -> &'a serde_json::Value {
    findings
        .as_array()
        .expect("findings must be an array")
        .iter()
        .find(|f| f["name"] == name)
        .unwrap_or_else(|| panic!("expected finding for function '{name}', got:\n{findings:#?}"))
}

// -----------------------------------------------------------------------------
// ifChain
// -----------------------------------------------------------------------------

#[test]
fn baseline_if_chain_cyclomatic() {
    // ifChain: 9 sequential `if`s with no nesting.
    // Cyclomatic = 1 (base) + 9 (one per `if`) = 10.
    let findings = run_baseline_fixture(1, 9999);
    let f = find_fn(&findings, "ifChain");
    assert_eq!(
        f["cyclomatic"].as_u64().unwrap(),
        10,
        // 1 (base) + 9 x if = 10
        "ifChain cyclomatic: 1 base + 9 ifs = 10"
    );
}

#[test]
fn baseline_if_chain_cognitive() {
    // ifChain: 9 sequential top-level `if`s at nesting=0.
    // Each `if` contributes inc_cognitive_with_nesting: weight = 1 + 0 = 1.
    // Cognitive = 9 x 1 = 9.
    let findings = run_baseline_fixture(9999, 1);
    let f = find_fn(&findings, "ifChain");
    assert_eq!(
        f["cognitive"].as_u64().unwrap(),
        9,
        // 9 x (1 + nesting=0) = 9
        "ifChain cognitive: 9 ifs x weight-1 each = 9"
    );
}

// -----------------------------------------------------------------------------
// nestedConditions
// -----------------------------------------------------------------------------

#[test]
fn baseline_nested_conditions_cyclomatic() {
    // nestedConditions: 3 levels of nested `if`.
    // Cyclomatic = 1 (base) + 3 (one per `if`) = 4.
    let findings = run_baseline_fixture(1, 9999);
    let f = find_fn(&findings, "nestedConditions");
    assert_eq!(
        f["cyclomatic"].as_u64().unwrap(),
        4,
        // 1 base + 3 ifs = 4
        "nestedConditions cyclomatic: 1 base + 3 ifs = 4"
    );
}

#[test]
fn baseline_nested_conditions_cognitive() {
    // nestedConditions: if at depth 0 (+1), if at depth 1 (+2), if at depth 2 (+3).
    // Each `if` increments nesting before visiting its body, so the inner `if`
    // sees the nesting that the outer `if` established.
    // Cognitive = (1+0) + (1+1) + (1+2) = 1 + 2 + 3 = 6.
    let findings = run_baseline_fixture(9999, 1);
    let f = find_fn(&findings, "nestedConditions");
    assert_eq!(
        f["cognitive"].as_u64().unwrap(),
        6,
        // outer if: 1+0=1, middle if: 1+1=2, inner if: 1+2=3 => total 6
        "nestedConditions cognitive: (1+0)+(1+1)+(1+2) = 6"
    );
}

// -----------------------------------------------------------------------------
// switchHeavy
// -----------------------------------------------------------------------------

#[test]
fn baseline_switch_heavy_cyclomatic() {
    // switchHeavy: switch with 10 non-default cases.
    // visit_switch_statement contributes NO cyclomatic increment (the switch
    // itself is not a branch for cyclomatic); visit_switch_case adds +1 for
    // each case with a test (i.e., non-default), so 10 cases x 1 = 10.
    // Cyclomatic = 1 (base) + 10 cases = 11.
    let findings = run_baseline_fixture(1, 9999);
    let f = find_fn(&findings, "switchHeavy");
    assert_eq!(
        f["cyclomatic"].as_u64().unwrap(),
        11,
        // 1 base + 10 cases = 11
        "switchHeavy cyclomatic: 1 base + 10 cases = 11"
    );
}

#[test]
fn baseline_switch_heavy_cognitive() {
    // switchHeavy: switch at top level (nesting=0).
    // visit_switch_statement contributes inc_cognitive_with_nesting: weight = 1+0 = 1.
    // Individual cases do NOT increment cognitive (only cyclomatic).
    // Cognitive = 1 (switch at nesting=0) = 1.
    //
    // Note: max_cognitive=0 is used here because the threshold check is STRICT
    // greater-than (cognitive > max_cognitive). With max_cognitive=1, a function
    // whose cognitive is exactly 1 would not appear in findings.
    let findings = run_baseline_fixture(9999, 0);
    let f = find_fn(&findings, "switchHeavy");
    assert_eq!(
        f["cognitive"].as_u64().unwrap(),
        1,
        // switch: 1+nesting=0 => 1; cases add cyclomatic only
        "switchHeavy cognitive: switch itself = 1, cases add cyclomatic only"
    );
}

// -----------------------------------------------------------------------------
// booleanOperators
// -----------------------------------------------------------------------------

#[test]
fn baseline_boolean_operators_cyclomatic() {
    // booleanOperators: `if (a && b)` and `if (c || d)`.
    // if1: +1 (if); `a && b`: +1 (&&).
    // if2: +1 (if); `c || d`: +1 (||).
    // Cyclomatic = 1 (base) + 1(if1) + 1(&&) + 1(if2) + 1(||) = 5.
    let findings = run_baseline_fixture(1, 9999);
    let f = find_fn(&findings, "booleanOperators");
    assert_eq!(
        f["cyclomatic"].as_u64().unwrap(),
        5,
        // 1 base + 1 if + 1 && + 1 if + 1 || = 5
        "booleanOperators cyclomatic: 1 base + if + && + if + || = 5"
    );
}

#[test]
fn baseline_boolean_operators_cognitive() {
    // booleanOperators: `if (a && b)` and `if (c || d)`.
    // if1 at nesting=0: +1. `&&` (first in its sequence): +1.
    // if2 at nesting=0: +1. `||` (first in its sequence): +1.
    // Cognitive = 1(if1) + 1(&&) + 1(if2) + 1(||) = 4.
    let findings = run_baseline_fixture(9999, 1);
    let f = find_fn(&findings, "booleanOperators");
    assert_eq!(
        f["cognitive"].as_u64().unwrap(),
        4,
        // if1=1, &&=1, if2=1, ||=1 => total 4
        "booleanOperators cognitive: if1 + && + if2 + || = 4"
    );
}

// -----------------------------------------------------------------------------
// loopsAndCatch
// -----------------------------------------------------------------------------

#[test]
fn baseline_loops_and_catch_cyclomatic() {
    // loopsAndCatch: `for` + `while` (nested inside for body) + `catch`.
    // for: +1. while: +1. catch: +1.
    // Cyclomatic = 1 (base) + 1(for) + 1(while) + 1(catch) = 4.
    let findings = run_baseline_fixture(1, 9999);
    let f = find_fn(&findings, "loopsAndCatch");
    assert_eq!(
        f["cyclomatic"].as_u64().unwrap(),
        4,
        // 1 base + 1 for + 1 while + 1 catch = 4
        "loopsAndCatch cyclomatic: 1 base + for + while + catch = 4"
    );
}

#[test]
fn baseline_loops_and_catch_cognitive() {
    // loopsAndCatch: for at nesting=0, while inside for body at nesting=1, catch at nesting=0.
    // for: inc_cognitive_with_nesting at nesting=0 => weight = 1+0 = 1.
    //   The for visitor does inc_nesting() before visiting body, so body is at nesting=1.
    // while: inc_cognitive_with_nesting at nesting=1 => weight = 1+1 = 2.
    // catch: inc_cognitive_with_nesting at nesting=0 => weight = 1+0 = 1.
    //   (the try/catch is outside the for loop body)
    // Cognitive = 1(for) + 2(while at nest 1) + 1(catch) = 4.
    let findings = run_baseline_fixture(9999, 1);
    let f = find_fn(&findings, "loopsAndCatch");
    assert_eq!(
        f["cognitive"].as_u64().unwrap(),
        4,
        // for=1, while(nest1)=2, catch=1 => total 4
        "loopsAndCatch cognitive: for=1 + while@nest1=2 + catch=1 = 4"
    );
}

// -----------------------------------------------------------------------------
// All five functions appear at threshold=1
// -----------------------------------------------------------------------------

#[test]
fn baseline_all_five_functions_surface_at_threshold_one() {
    // With max_cyclomatic=1 every function with any branch appears. The fixture
    // has exactly 5 functions each with at least one decision point.
    let findings = run_baseline_fixture(1, 9999);
    let names: Vec<&str> = findings
        .as_array()
        .expect("findings must be an array")
        .iter()
        .filter_map(|f| f["name"].as_str())
        .filter(|n| {
            matches!(
                *n,
                "ifChain"
                    | "nestedConditions"
                    | "switchHeavy"
                    | "booleanOperators"
                    | "loopsAndCatch"
            )
        })
        .collect();
    for expected in &[
        "ifChain",
        "nestedConditions",
        "switchHeavy",
        "booleanOperators",
        "loopsAndCatch",
    ] {
        assert!(
            names.contains(expected),
            "expected '{expected}' in findings, got: {names:?}"
        );
    }
}
