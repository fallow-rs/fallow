//! The drift contract predicates. Each function checks one invariant from
//! `docs/development/drift-contract.md` and returns a readable diff on failure.

use std::collections::{BTreeMap, BTreeSet};

use similar::TextDiff;

use crate::common::{CommandOutput, canonical_report};
use crate::keys::{AuditKeys, FindingKey, IdentifiedFinding, KeySet, render};

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

/// I3: each section of bare `fallow` holds the finding keys of the standalone
/// command with the same baseline. Each entry is (analysis label, section
/// keys, standalone keys).
pub fn i3_sections_equal_standalone(
    context: &str,
    sections: &[(String, KeySet, KeySet)],
) -> Verdict {
    let failures: Vec<String> = sections
        .iter()
        .filter_map(|(label, section, standalone)| {
            keys_equal(
                &format!("bare `fallow` {label} section"),
                section,
                &format!("`{label}`"),
                standalone,
            )
            .err()
        })
        .collect();
    if failures.is_empty() {
        return Ok(());
    }
    Err(format!("{context}:\n{}", failures.join("\n")))
}

/// The verdict an envelope states in `gate_outcomes`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StatedVerdict {
    /// Some entry reports `fail`.
    pub failed: bool,
    /// Some entry reports `fail` and is `enforced`.
    pub enforced_failure: bool,
    /// The exit code of the enforced entries that fail, 0 when none fails.
    pub enforced_code: i32,
    /// The exit code of every entry that fails, enforced or not, 0 when none
    /// fails.
    pub failed_code: i32,
}

impl StatedVerdict {
    /// The verdict of a run that armed no gate.
    pub const PASS: Self = Self {
        failed: false,
        enforced_failure: false,
        enforced_code: 0,
        failed_code: 0,
    };
}

/// The exit code that a failed gate gives the process, as the CLI documents
/// it: `security --gate` exits 8, every other gate exits 1.
fn gate_failure_code(gate: &str) -> i32 {
    if gate == "security" { 8 } else { 1 }
}

/// Read the verdict of `gate_outcomes`, `None` when the object is absent.
pub fn stated_verdict(envelope: &serde_json::Value) -> Option<StatedVerdict> {
    let gates = envelope.get("gate_outcomes")?.as_object()?;
    let failing = gates
        .iter()
        .filter(|(_, outcome)| outcome["status"] == "fail")
        .collect::<Vec<_>>();
    let code = |enforced_only: bool| {
        failing
            .iter()
            .filter(|(_, outcome)| !enforced_only || outcome["enforced"] == true)
            .map(|(gate, _)| gate_failure_code(gate))
            .max()
            .unwrap_or(0)
    };
    Some(StatedVerdict {
        failed: !failing.is_empty(),
        enforced_failure: failing
            .iter()
            .any(|(_, outcome)| outcome["enforced"] == true),
        enforced_code: code(true),
        failed_code: code(false),
    })
}

/// How a command turns its verdict into an exit code in a machine format.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExitRule {
    /// The machine run and the human run both exit with the code of the
    /// enforced gates that fail.
    Enforced,
    /// Bare `fallow` without `--fail-on-issues`: the machine run exits with
    /// the code of the enforced gates that fail (`regression`,
    /// `stale-baseline`, `baseline-growth`, `type-aware-require` and
    /// `parse-error`), and the human run fails on every gate that fails. With
    /// `--fail-on-issues`, bare `fallow` follows [`ExitRule::Enforced`].
    CombinedMachine,
}

/// One command of an I7 comparison: its machine runs and its human run.
pub struct VerdictRuns<'a> {
    pub command: &'a str,
    pub rule: ExitRule,
    /// Whether the envelope must carry `gate_outcomes`. `dupes` has no default
    /// exit rule, so its object is absent when no gate armed.
    pub requires_object: bool,
    /// Each machine envelope with its exit code, labelled.
    pub machine: Vec<(String, serde_json::Value, i32)>,
    pub human_code: i32,
}

/// I7: every machine envelope states the verdict of the human run, and each
/// exit code follows the rule of its command.
pub fn i7_verdicts_agree(runs: &VerdictRuns<'_>) -> Verdict {
    let mut problems = Vec::new();
    for (label, envelope, code) in &runs.machine {
        let stated = match stated_verdict(envelope) {
            Some(stated) => stated,
            None if runs.requires_object => {
                problems.push(format!("{label}: the envelope has no gate_outcomes"));
                continue;
            }
            None => StatedVerdict::PASS,
        };
        let (expected_code, expected_human_code) = match runs.rule {
            ExitRule::Enforced => (stated.enforced_code, stated.enforced_code),
            ExitRule::CombinedMachine => (stated.enforced_code, stated.failed_code),
        };
        if *code != expected_code {
            problems.push(format!(
                "{label}: exit {code}, but the stated verdict {stated:?} gives exit {expected_code}: {}",
                envelope["gate_outcomes"]
            ));
        }
        if expected_human_code != runs.human_code {
            problems.push(format!(
                "{label}: the stated verdict {stated:?} does not match the human run, which exits {}: {}",
                runs.human_code, envelope["gate_outcomes"]
            ));
        }
    }
    if problems.is_empty() {
        return Ok(());
    }
    Err(format!(
        "`{}` verdicts differ:\n{}",
        runs.command,
        problems.join("\n")
    ))
}

/// I8 (narrowing half): a scoped run holds no finding that the unscoped run lacks.
pub fn i8_narrows(context: &str, scoped: &KeySet, unscoped: &KeySet) -> Verdict {
    keys_subset("scoped run", scoped, "unscoped run", unscoped)
        .map_err(|err| format!("{context}: {err}"))
}

/// Dead-code kinds that `--changed-since` keeps whatever changed: whether a
/// dependency is unused is a fact about the whole graph, not about one file
/// (`filter_results_by_changed_files` in `crates/engine/src/changed_files.rs`).
pub const CHANGED_SINCE_UNFILTERED_KINDS: &[&str] = &[
    "unused_dependencies",
    "unused_dev_dependencies",
    "unused_optional_dependencies",
    "type_only_dependencies",
    "test_only_dependencies",
    "dev_dependencies_in_production",
    "unused_catalog_entries",
];

/// I8 (location half): every finding of a scoped run touches the scope,
/// except findings of the `exempt` kinds. A clone group touches the scope when
/// one of its instances does.
pub fn i8_inside_scope(
    context: &str,
    scoped: &KeySet,
    exempt: &[&str],
    in_scope: impl Fn(&str) -> bool,
) -> Verdict {
    let outside: KeySet = scoped
        .iter()
        .filter(|key| !exempt.contains(&key.kind.as_str()))
        .filter(|key| !key_paths(key).iter().any(|path| in_scope(path)))
        .cloned()
        .collect();
    if outside.is_empty() {
        return Ok(());
    }
    Err(format!(
        "{context}: findings outside the scope:\n{}",
        render(&outside)
    ))
}

/// The paths of a key. Findings over several files join them with ` -> `.
fn key_paths(key: &FindingKey) -> BTreeSet<&str> {
    key.path.split(" -> ").collect()
}

/// The `--performance` work counters of one CLI run, read from stderr.
fn work_counters(label: &str, output: &CommandOutput) -> Result<serde_json::Value, String> {
    output
        .stderr
        .lines()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .find_map(|value| value.get("counters").cloned())
        .ok_or_else(|| format!("{label}: no work counters on stderr:\n{}", output.stderr))
}

/// The sorted ids of the dead-code findings an envelope reports.
pub fn reported_finding_ids(envelope: &serde_json::Value) -> Vec<String> {
    let mut ids: Vec<String> = crate::keys::dead_code_finding_ids(envelope)
        .into_iter()
        .filter_map(|(_, id)| id)
        .collect();
    ids.sort();
    ids
}

/// I11: a finding-id query reports exactly the requested findings that the
/// full run reports, and every surface gives the same `finding_id_query`.
///
/// `runs` holds one `(label, envelope)` per surface. `expected_ids` are the
/// requested ids that the full run reports, sorted.
pub fn i11_finding_id_query_agrees(
    runs: &[(String, serde_json::Value)],
    expected_ids: &[String],
) -> Verdict {
    let mut first: Option<(&str, &serde_json::Value)> = None;
    for (label, envelope) in runs {
        let reported = reported_finding_ids(envelope);
        if reported != expected_ids {
            return Err(format!(
                "{label}: the query reported {reported:?}, expected {expected_ids:?}"
            ));
        }
        let query = envelope
            .get("finding_id_query")
            .ok_or_else(|| format!("{label}: no finding_id_query in the envelope"))?;
        match first {
            None => first = Some((label, query)),
            Some((first_label, expected)) if expected != query => {
                return Err(format!(
                    "finding_id_query differs: {first_label} {expected} != {label} {query}"
                ));
            }
            Some(_) => {}
        }
    }
    Ok(())
}

/// I9: the `--performance` work counters are equal for every run of the same
/// project, whatever the thread count and whether `check` or `dead-code` ran.
pub fn i9_work_counters_agree(runs: &[(&str, &CommandOutput)]) -> Verdict {
    let mut first: Option<(&str, serde_json::Value)> = None;
    for (label, output) in runs {
        let counters = work_counters(label, output)?;
        match &first {
            None => first = Some((label, counters)),
            Some((first_label, expected)) if *expected != counters => {
                return Err(format!(
                    "work counters differ: {first_label} {expected} != {label} {counters}"
                ));
            }
            Some(_) => {}
        }
    }
    Ok(())
}

/// The length of a security `finding_id`: an FNV-1a 64 digest in hex.
const SECURITY_ID_LEN: usize = 16;

/// I12 (run half): every security finding of one run carries a `finding_id`
/// of 16 lowercase hex digits, and no two findings share an id.
pub fn i12_security_ids_present_and_unique(
    context: &str,
    findings: &[IdentifiedFinding],
) -> Verdict {
    let mut problems = Vec::new();
    let mut owners: BTreeMap<&str, Vec<&FindingKey>> = BTreeMap::new();
    for (key, id) in findings {
        match id.as_deref() {
            None => problems.push(format!("    {key}: no finding_id")),
            Some(id)
                if id.len() != SECURITY_ID_LEN
                    || !id
                        .bytes()
                        .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)) =>
            {
                problems.push(format!("    {key}: finding_id {id:?} is not 16 hex digits"));
            }
            Some(id) => owners.entry(id).or_default().push(key),
        }
    }
    for (id, keys) in owners.iter().filter(|(_, keys)| keys.len() > 1) {
        let keys = keys.iter().map(ToString::to_string).collect::<Vec<_>>();
        problems.push(format!(
            "    {id} is on {} findings: {}",
            keys.len(),
            keys.join("; ")
        ));
    }
    if problems.is_empty() {
        return Ok(());
    }
    Err(format!(
        "{context}: bad security finding ids:\n{}",
        problems.join("\n")
    ))
}

/// The prefix of every dead-code `finding_id`: the id scheme and its separator.
pub const FINDING_ID_PREFIX: &str = "dc1:";

/// I10 (run half): every dead-code finding of one run carries a `finding_id`
/// with the `dc1:` prefix, and no two findings share an id.
pub fn i10_ids_present_and_unique(context: &str, findings: &[IdentifiedFinding]) -> Verdict {
    let mut problems = Vec::new();
    let mut owners: BTreeMap<&str, Vec<&FindingKey>> = BTreeMap::new();
    for (key, id) in findings {
        match id.as_deref() {
            None => problems.push(format!("    {key}: no finding_id")),
            Some(id) if !id.starts_with(FINDING_ID_PREFIX) => {
                problems.push(format!(
                    "    {key}: finding_id {id:?} has no {FINDING_ID_PREFIX} prefix"
                ));
            }
            Some(id) => owners.entry(id).or_default().push(key),
        }
    }
    for (id, keys) in owners.iter().filter(|(_, keys)| keys.len() > 1) {
        let keys = keys.iter().map(ToString::to_string).collect::<Vec<_>>();
        problems.push(format!(
            "    {id} is on {} findings: {}",
            keys.len(),
            keys.join("; ")
        ));
    }
    if problems.is_empty() {
        return Ok(());
    }
    Err(format!(
        "{context}: bad finding ids:\n{}",
        problems.join("\n")
    ))
}

/// I10 (surface half): every surface reports the same findings with the same
/// `finding_id` values as the first one.
pub fn i10_ids_agree(context: &str, results: &[(String, Vec<IdentifiedFinding>)]) -> Verdict {
    let Some((reference_label, reference)) = results.first() else {
        return Ok(());
    };
    let failures: Vec<String> = results
        .iter()
        .skip(1)
        .filter(|(_, findings)| findings != reference)
        .map(|(label, findings)| {
            format!(
                "{reference_label} != {label}\n  only in {reference_label}:\n{}\n  only in {label}:\n{}",
                render_identified(reference, findings),
                render_identified(findings, reference),
            )
        })
        .collect();
    if failures.is_empty() {
        return Ok(());
    }
    Err(format!("{context}:\n{}", failures.join("\n")))
}

/// I6, I8 and I9: both runs pass the I10 run check, and a finding that is in
/// both runs has the same `finding_id` in both. Without the run check, two
/// runs with no ids at all would agree. A finding in only one run is not
/// compared here. The key checks of each invariant own that half.
pub fn ids_kept(
    context: &str,
    label_a: &str,
    a: &[IdentifiedFinding],
    label_b: &str,
    b: &[IdentifiedFinding],
) -> Verdict {
    i10_ids_present_and_unique(&format!("{context}, {label_a}"), a)?;
    i10_ids_present_and_unique(&format!("{context}, {label_b}"), b)?;
    let mut ids_b: BTreeMap<&FindingKey, BTreeSet<Option<&str>>> = BTreeMap::new();
    for (key, id) in b {
        ids_b.entry(key).or_default().insert(id.as_deref());
    }
    let changed: Vec<String> = a
        .iter()
        .filter_map(|(key, id)| {
            let other = ids_b.get(key)?;
            (!other.contains(&id.as_deref()))
                .then(|| format!("    {key}: {label_a} {id:?}, {label_b} {other:?}"))
        })
        .collect();
    if changed.is_empty() {
        return Ok(());
    }
    Err(format!(
        "{context}: findings in {label_a} and {label_b} have other finding ids:\n{}",
        changed.join("\n")
    ))
}

/// The findings of `a` that are not in `b`, one per line.
fn render_identified(a: &[IdentifiedFinding], b: &[IdentifiedFinding]) -> String {
    let only: Vec<String> = a
        .iter()
        .filter(|finding| !b.contains(finding))
        .map(|(key, id)| format!("    {key} {}", id.as_deref().unwrap_or("(no finding_id)")))
        .collect();
    if only.is_empty() {
        return "    (none)".to_string();
    }
    only.join("\n")
}
