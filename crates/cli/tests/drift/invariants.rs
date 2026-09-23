//! The drift contract predicates. Each function checks one invariant from
//! `docs/development/drift-contract.md` and returns a readable diff on failure.

use similar::TextDiff;

use crate::common::{CommandOutput, canonical_report};
use crate::keys::{AuditKeys, KeySet, render};

/// Issue kinds that report a suppression comment itself. Invariant I6 exempts
/// them: a suppression comment that matches nothing is a finding by design.
pub const SUPPRESSION_REPORT_KINDS: &[&str] = &["stale_suppressions"];

/// Result of one predicate: `Err` carries the readable failure report.
pub type Verdict = Result<(), String>;

/// Report the difference between two key sets.
pub fn diff(label_a: &str, a: &KeySet, label_b: &str, b: &KeySet) -> String {
    let only_a: KeySet = a.difference(b).cloned().collect();
    let only_b: KeySet = b.difference(a).cloned().collect();
    format!(
        "  only in {label_a}:\n{}\n  only in {label_b}:\n{}",
        render(&only_a),
        render(&only_b)
    )
}

/// Two key sets must be equal.
pub fn keys_equal(label_a: &str, a: &KeySet, label_b: &str, b: &KeySet) -> Verdict {
    if a == b {
        return Ok(());
    }
    Err(format!(
        "{label_a} != {label_b}\n{}",
        diff(label_a, a, label_b, b)
    ))
}

/// `sub` must hold no key that `sup` lacks.
pub fn keys_subset(label_sub: &str, sub: &KeySet, label_sup: &str, sup: &KeySet) -> Verdict {
    let extra: KeySet = sub.difference(sup).cloned().collect();
    if extra.is_empty() {
        return Ok(());
    }
    Err(format!(
        "{label_sub} holds findings that {label_sup} does not:\n{}",
        render(&extra)
    ))
}

/// I1: `check` is byte-identical to `dead-code` after the volatile fields are
/// removed, and both runs exit with the same code.
pub fn i1_alias_identical(check: &CommandOutput, dead_code: &CommandOutput) -> Verdict {
    let check_report = pretty(&canonical_report(check));
    let dead_code_report = pretty(&canonical_report(dead_code));
    if check.code != dead_code.code {
        return Err(format!(
            "exit codes differ: check {} != dead-code {}",
            check.code, dead_code.code
        ));
    }
    if check_report == dead_code_report {
        return Ok(());
    }
    let unified = TextDiff::from_lines(&dead_code_report, &check_report)
        .unified_diff()
        .context_radius(2)
        .header("dead-code", "check")
        .to_string();
    Err(format!(
        "check output differs from dead-code output:\n{unified}"
    ))
}

fn pretty(canonical: &str) -> String {
    let value: serde_json::Value =
        serde_json::from_str(canonical).expect("canonical report is JSON");
    serde_json::to_string_pretty(&value).expect("pretty-print canonical report")
}

/// I2: every surface reports the same key set as the first one.
pub fn surfaces_agree(context: &str, results: &[(String, KeySet)]) -> Verdict {
    let Some((reference_label, reference)) = results.first() else {
        return Ok(());
    };
    let failures: Vec<String> = results
        .iter()
        .skip(1)
        .filter_map(|(label, keys)| keys_equal(reference_label, reference, label, keys).err())
        .collect();
    if failures.is_empty() {
        return Ok(());
    }
    Err(format!("{context}:\n{}", failures.join("\n")))
}

pub fn without_suppression_reports(keys: &KeySet) -> KeySet {
    keys.iter()
        .filter(|key| !SUPPRESSION_REPORT_KINDS.contains(&key.kind.as_str()))
        .cloned()
        .collect()
}

/// I6 (suppression half): the run with suppression comments holds no finding
/// that the run with plain comments on the same lines lacks.
pub fn i6_suppression_never_adds(context: &str, with: &KeySet, without: &KeySet) -> Verdict {
    keys_subset(
        "with suppression comments",
        &without_suppression_reports(with),
        "with plain comments",
        without,
    )
    .map_err(|err| format!("{context}: {err}"))
}

/// I6 (baseline half): more baseline entries never add a finding.
pub fn i6_baseline_never_adds(
    context: &str,
    none: &KeySet,
    partial: &KeySet,
    full: &KeySet,
) -> Verdict {
    keys_subset("partial baseline", partial, "no baseline", none)
        .and_then(|()| keys_subset("full baseline", full, "partial baseline", partial))
        .map_err(|err| format!("{context}: {err}"))
}

/// Two audit results must hold the same introduced keys and the same
/// inherited keys. With `compare_verdicts`, the verdicts must also be equal.
fn audit_keys_equal(
    label_a: &str,
    a: &AuditKeys,
    label_b: &str,
    b: &AuditKeys,
    compare_verdicts: bool,
) -> Verdict {
    let mut problems = Vec::new();
    if compare_verdicts && a.verdict != b.verdict {
        problems.push(format!(
            "verdict: {label_a} {:?} != {label_b} {:?}",
            a.verdict, b.verdict
        ));
    }
    for (split, keys_a, keys_b) in [
        ("introduced", &a.introduced, &b.introduced),
        ("inherited", &a.inherited, &b.inherited),
    ] {
        if let Err(err) = keys_equal(label_a, keys_a, label_b, keys_b) {
            problems.push(format!("{split}: {err}"));
        }
    }
    if problems.is_empty() {
        return Ok(());
    }
    Err(problems.join("\n"))
}

/// I4: the introduced and the inherited findings of `audit` are the expected
/// split of the head findings in scope.
pub fn i4_audit_attribution(expected: &AuditKeys, audit: &AuditKeys) -> Verdict {
    audit_keys_equal("expected", expected, "CLI audit", audit, false)
        .map_err(|err| format!("audit attribution differs from the expected split:\n{err}"))
}

/// I5: every surface gives the audit result of the first one: the same
/// introduced keys, the same inherited keys and the same verdict.
pub fn i5_audit_surfaces_agree(results: &[(String, AuditKeys)]) -> Verdict {
    let Some((reference_label, reference)) = results.first() else {
        return Ok(());
    };
    let failures: Vec<String> = results
        .iter()
        .skip(1)
        .filter_map(|(label, keys)| {
            audit_keys_equal(reference_label, reference, label, keys, true).err()
        })
        .collect();
    if failures.is_empty() {
        return Ok(());
    }
    Err(format!("audit results differ:\n{}", failures.join("\n")))
}
