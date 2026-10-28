use std::collections::BTreeSet;

use fallow_types::issue_meta::counted_result_issue_metas;

use super::{
    DEAD_CODE_CATEGORIES, LinkContext, render_audit_summary, render_combined_summary,
    render_health_summary,
};

/// One listed complexity finding.
fn complexity_finding(name: &str) -> serde_json::Value {
    serde_json::json!({
        "path": "src/a.ts", "name": name, "line": 1, "col": 0,
        "cyclomatic": 25, "cognitive": 30, "line_count": 40,
        "exceeded": "both", "severity": "high"
    })
}

/// A health summary with a loaded baseline: `above` functions exceed a
/// threshold before the baseline, and `remaining` after it.
fn health_with_baseline(above: u64, remaining: u64) -> serde_json::Value {
    let findings: Vec<serde_json::Value> = (0..remaining)
        .map(|index| complexity_finding(&format!("f{index}")))
        .collect();
    serde_json::json!({
        "elapsed_ms": 1,
        "findings": findings,
        "vital_signs": { "avg_cyclomatic": 2.0 },
        "summary": {
            "files_analyzed": 3,
            "functions_analyzed": 90,
            "functions_above_threshold": above,
            "max_cyclomatic_threshold": 20,
            "max_cognitive_threshold": 15,
            "max_crap_threshold": 30.0,
            "baseline_staleness": {
                "baseline_entries": above,
                "matched_entries": above,
                "stale_entries": 0,
                "current_findings": above,
                "remaining_findings": remaining,
                "stale": false
            }
        }
    })
}

fn no_links() -> LinkContext {
    LinkContext {
        prefix: String::new(),
        repo: String::new(),
        sha: String::new(),
    }
}

/// A skipped dead-code pass reports 0 issues, which must not read as a
/// clean row.
#[test]
fn audit_summary_marks_skipped_sub_analyses() {
    let audit = |dead_code: bool, complexity: bool, duplication: bool| {
        serde_json::json!({
            "command": "audit",
            "verdict": "warn",
            "changed_files_count": 1,
            "elapsed_ms": 3,
            "summary": {
                "dead_code_issues": 0,
                "dead_code_skipped": dead_code,
                "complexity_skipped": complexity,
                "duplication_skipped": duplication,
                "complexity_findings": 1,
                "duplication_clone_groups": 0
            },
            "attribution": {
                "gate": "new-only",
                "dead_code_introduced": 0,
                "dead_code_inherited": 0,
                "complexity_introduced": 1,
                "complexity_inherited": 0,
                "duplication_introduced": 0,
                "duplication_inherited": 0,
                "styling_introduced": 0,
                "styling_inherited": 0
            }
        })
    };
    let out = render_audit_summary(&audit(true, false, false));
    assert!(out.contains("| Dead code | skipped | - | - |"), "{out}");
    assert!(out.contains("| Complexity | 1 | 1 | 0 |"), "{out}");
    assert!(out.contains("| Styling | 0 | 0 | 0 |"), "{out}");
    let out = render_audit_summary(&audit(false, true, true));
    assert!(out.contains("| Dead code | 0 | 0 | 0 |"), "{out}");
    assert!(out.contains("| Complexity | skipped | - | - |"), "{out}");
    assert!(out.contains("| Duplication | skipped | - | - |"), "{out}");
    assert!(out.contains("| Styling | skipped | - | - |"), "{out}");
}

#[test]
fn audit_summary_names_worsened_findings_and_their_mode() {
    let audit = |mode: &str, verdict: &str| {
        serde_json::json!({
            "command": "audit",
            "verdict": verdict,
            "changed_files_count": 1,
            "elapsed_ms": 3,
            "summary": {"dead_code_issues": 0, "complexity_findings": 1, "duplication_clone_groups": 0},
            "attribution": {
                "gate": "new-only",
                "worsened": mode,
                "dead_code_introduced": 0,
                "dead_code_inherited": 0,
                "complexity_introduced": 0,
                "complexity_inherited": 1,
                "complexity_worsened": 1,
                "duplication_introduced": 0,
                "duplication_inherited": 0
            },
            "complexity": {"findings": [{
                "path": "src/a.ts",
                "line": 1,
                "name": "run",
                "introduced": false,
                "attribution": "worsened",
                "effective_severity": "error",
                "cyclomatic": 30,
                "cognitive": 20
            }]}
        })
    };
    let out = render_audit_summary(&audit("error", "fail"));
    assert!(out.contains("| `run` | worsened |"), "{out}");
    assert!(
            out.contains("> 1 inherited complexity finding got worse than the base (status `worsened`): `--worsened error` counts them like introduced findings."),
            "{out}"
        );
    assert!(
        out.contains("*Audit gate: new-only, worsened: error."),
        "{out}"
    );
    assert!(
        !out.contains("Inherited findings are reported but"),
        "{out}"
    );

    let out = render_audit_summary(&audit("warn", "warn"));
    assert!(out.contains("| `run` | worsened |"), "{out}");
    assert!(
        out.contains("they make the verdict at least warn and do not fail it."),
        "{out}"
    );
    assert!(
        out.contains(
            "*Audit gate: new-only. Inherited findings are reported but do not fail the verdict.*"
        ),
        "{out}"
    );
}

/// With a baseline, the headline counts the functions after the
/// baseline and gives the accepted functions as context.
#[test]
fn health_summary_headline_counts_after_the_baseline() {
    let out = render_health_summary(&health_with_baseline(76, 0));
    assert!(
        out.contains("**No new functions exceed complexity thresholds (76 in the baseline)**"),
        "{out}"
    );
    let out = render_health_summary(&health_with_baseline(76, 2));
    assert!(
        out.contains("**2 new functions exceed thresholds (74 in the baseline)**"),
        "{out}"
    );
    assert!(!out.contains("76 functions"), "{out}");
}

/// The bare-run summary counts health findings after the baseline too.
#[test]
fn combined_summary_counts_health_after_the_baseline() {
    let combined = |health: serde_json::Value| {
        serde_json::json!({
            "check": { "total_issues": 0 },
            "dupes": { "clone_groups": [], "stats": {} },
            "health": health
        })
    };
    let out = render_combined_summary(&combined(health_with_baseline(76, 0)), &no_links());
    assert!(
        out.contains(":white_check_mark: No new complex functions (76 in the baseline)"),
        "{out}"
    );
    assert!(!out.contains("above threshold"), "{out}");

    let out = render_combined_summary(&combined(health_with_baseline(76, 2)), &no_links());
    assert!(out.contains(":warning: **2** health findings"), "{out}");
    assert!(
        out.contains("(2 new functions above threshold, 74 in the baseline)"),
        "{out}"
    );
}

/// Without `remaining_findings` the combined count stays the summary count.
#[test]
fn combined_summary_without_a_baseline_keeps_the_summary_count() {
    let mut health = health_with_baseline(3, 3);
    health["summary"]
        .as_object_mut()
        .expect("summary")
        .remove("baseline_staleness");
    let env = serde_json::json!({
        "check": { "total_issues": 0 },
        "dupes": { "clone_groups": [], "stats": {} },
        "health": health
    });
    let out = render_combined_summary(&env, &no_links());
    assert!(out.contains(":warning: **3** health findings"), "{out}");
    assert!(out.contains("(3 functions above threshold)"), "{out}");
    assert!(!out.contains("baseline"), "{out}");
}

/// A run without a finding list (for example `--score`) still counts the
/// functions above a threshold. The summary must give that count.
#[test]
fn health_summary_counts_unlisted_functions() {
    let envelope = |above: u64| {
        serde_json::json!({
            "elapsed_ms": 1,
            "summary": {
                "functions_analyzed": 5,
                "functions_above_threshold": above,
                "max_cyclomatic_threshold": 20,
                "max_cognitive_threshold": 15,
                "max_crap_threshold": 30.0
            }
        })
    };
    let out = render_health_summary(&envelope(1));
    assert!(out.contains("**1 function exceeds thresholds**"), "{out}");
    assert!(out.contains("`fallow health --complexity`"), "{out}");
    assert!(!out.contains("baseline"), "{out}");
    assert!(!out.contains("No functions exceed"), "{out}");
    let out = render_health_summary(&envelope(0));
    assert!(
        out.contains("**No functions exceed complexity thresholds**"),
        "{out}"
    );
}

/// A baselined envelope. A score-only run omits `vital_signs`; a run that
/// lists findings carries it.
fn baselined_envelope(lists_findings: bool) -> serde_json::Value {
    let mut envelope = serde_json::json!({
        "elapsed_ms": 1,
        "findings": [],
        "summary": {
            "functions_analyzed": 5,
            "functions_above_threshold": 1,
            "max_cyclomatic_threshold": 20,
            "max_cognitive_threshold": 15,
            "max_crap_threshold": 30.0,
            "baseline_staleness": {
                "baseline_entries": 2,
                "matched_entries": 2,
                "stale_entries": 0,
                "current_findings": 1,
                "stale": false
            }
        }
    });
    if lists_findings {
        envelope["vital_signs"] = serde_json::json!({ "avg_cyclomatic": 2.0 });
    }
    envelope
}

/// A run that lists findings and has a baseline that accepts every
/// finding is clean. The summary must not give the count or ask for a
/// rerun.
#[test]
fn health_summary_keeps_a_baselined_listed_run_clean() {
    let out = render_health_summary(&baselined_envelope(true));
    assert!(
        out.contains("**No functions exceed complexity thresholds**"),
        "{out}"
    );
    assert!(!out.contains("--complexity"), "{out}");
}

/// A score-only run does not list findings, so a baseline does not make
/// it clean. The count comes before the baseline, and the summary says so.
#[test]
fn health_summary_counts_unlisted_functions_on_a_baselined_score_run() {
    let out = render_health_summary(&baselined_envelope(false));
    assert!(out.contains("**1 function exceeds thresholds**"), "{out}");
    assert!(
        out.contains("The count includes the functions that the baseline accepts."),
        "{out}"
    );
    assert!(
        out.contains("Run `fallow health --complexity` with the same `--baseline`"),
        "{out}"
    );
    assert!(!out.contains("No functions exceed"), "{out}");
}

/// A run with `sections` that does not name `complexity` does not list
/// findings, also when it has `vital_signs` and a baseline. The summary
/// gives the count and the number of new functions.
#[test]
fn health_summary_reads_the_sections_member() {
    let mut env = baselined_envelope(true);
    env["sections"] = serde_json::json!(["vital-signs", "hotspots"]);
    env["summary"]["functions_above_threshold"] = serde_json::json!(3);
    env["summary"]["baseline_staleness"]["remaining_findings"] = serde_json::json!(1);
    let out = render_health_summary(&env);
    assert!(out.contains("**3 functions exceed thresholds**"), "{out}");
    assert!(out.contains("The baseline accepts 2. 1 is new."), "{out}");
    assert!(
        out.contains("Run `fallow health --complexity` with the same `--baseline`"),
        "{out}"
    );
    assert!(!out.contains("No functions exceed"), "{out}");
    assert!(!out.contains("The count includes"), "{out}");

    env["sections"] = serde_json::json!(["complexity", "vital-signs", "hotspots"]);
    let out = render_health_summary(&env);
    assert!(
        out.contains("**No functions exceed complexity thresholds**"),
        "{out}"
    );
}

/// When the baseline accepts every function, the note says so and does
/// not ask for a rerun.
#[test]
fn health_summary_with_no_new_function_does_not_ask_for_a_rerun() {
    let mut env = baselined_envelope(false);
    env["sections"] = serde_json::json!(["score"]);
    env["summary"]["baseline_staleness"]["remaining_findings"] = serde_json::json!(0);
    let out = render_health_summary(&env);
    assert!(out.contains("**1 function exceeds thresholds**"), "{out}");
    assert!(out.contains("The baseline accepts 1. 0 are new."), "{out}");
    assert!(!out.contains("--complexity"), "{out}");
}

/// Every counted dead-code kind needs a summary row with the registry
/// label and docs anchor, and every summary row needs a registry kind.
#[test]
fn dead_code_categories_match_counted_result_metadata() {
    let native: BTreeSet<(&str, &str, &str)> = DEAD_CODE_CATEGORIES.iter().copied().collect();
    assert_eq!(
        native.len(),
        DEAD_CODE_CATEGORIES.len(),
        "DEAD_CODE_CATEGORIES has a duplicate row"
    );
    let registry: BTreeSet<(&str, &str, &str)> = counted_result_issue_metas()
        .map(|meta| (meta.summary_label, meta.result_key, meta.docs_anchor))
        .collect();
    let missing: Vec<_> = registry.difference(&native).collect();
    let extra: Vec<_> = native.difference(&registry).collect();
    assert!(
        missing.is_empty() && extra.is_empty(),
        "DEAD_CODE_CATEGORIES must match the counted result metadata \
             (label, result key, docs anchor).\nMissing rows: {missing:?}\nRows not in the registry: {extra:?}"
    );
}

/// Rows without a path or line tie on the first two keys. The serialized
/// row breaks the tie, so the rendered order does not depend on the input
/// order. The rows past the rendered limit are counted, not listed.
#[test]
fn pathless_rows_render_in_one_order_for_any_input_order() {
    let envelope = |names: &[String]| {
        serde_json::json!({
            "total_issues": names.len(),
            "elapsed_ms": 1,
            "unlisted_dependencies": names
                .iter()
                .map(|name| serde_json::json!({ "package_name": name, "imported_from": [] }))
                .collect::<Vec<_>>(),
        })
    };
    let names: Vec<String> = (0..40).map(|index| format!("pkg-{index:02}")).collect();
    let mut reversed = names.clone();
    reversed.reverse();
    let ordered = super::render_check_summary(&envelope(&names));
    assert_eq!(super::render_check_summary(&envelope(&reversed)), ordered);
    let first = ordered.find("pkg-00").expect("first row");
    let second = ordered.find("pkg-01").expect("second row");
    assert!(
        first < second,
        "rows follow the serialized order: {ordered}"
    );
    assert!(
        !ordered.contains("pkg-39"),
        "rows past the limit are counted only"
    );
    assert!(
        ordered.contains("15 more"),
        "the tail counts the rest: {ordered}"
    );
}

/// A saved `--group-by` envelope lists rows group by group. The summary
/// renders them in one order whatever the input order is.
#[test]
fn dead_code_rows_render_in_one_order_for_any_input_order() {
    let envelope = |paths: [&str; 3]| {
        serde_json::json!({
            "total_issues": 3,
            "elapsed_ms": 1,
            "unused_files": paths.map(|path| serde_json::json!({ "path": path })),
        })
    };
    let flat = super::render_check_summary(&envelope(["src/a.ts", "src/b.ts", "src/c.ts"]));
    let grouped = super::render_check_summary(&envelope(["src/a.ts", "src/c.ts", "src/b.ts"]));
    assert_eq!(grouped, flat);
    let a = flat.find("`src/a.ts`").expect("a");
    let b = flat.find("`src/b.ts`").expect("b");
    let c = flat.find("`src/c.ts`").expect("c");
    assert!(a < b && b < c, "rows follow path order: {flat}");
}
