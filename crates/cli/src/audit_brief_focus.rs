//! The weighted focus map of the review brief: the inputs that the brief
//! gathers for each focus unit and the call into the focus scorer.

use rustc_hash::FxHashSet;

use super::{AuditResult, ReviewDeltas, classify_test_reach};

/// Build the Stage 4 weighted focus map from the audit result's retained
/// per-file graph facts plus the deltas / coordination signals. Returns an
/// empty focus map when no graph facts were retained (off the brief path or no
/// changed file mapped to a module).
///
/// The boundary risk-zone signal reuses the `from_path` of boundary violations
/// whose introduced edge is in `deltas.boundary_introduced` (the same surface
/// the decision surface reads). The security taint signal is wired as an EMPTY
/// slice today: the brief path runs the bare dead-code analysis, not the opt-in
/// `fallow security` taint engine, so `results.security_findings` is empty. The
/// seam lights up the moment a security pass is threaded onto the brief, with no
/// focus-map code change.
#[must_use]
pub(super) fn build_focus_map(
    result: &AuditResult,
    deltas: &ReviewDeltas,
    diff_index: Option<&fallow_output::DiffIndex>,
) -> crate::audit_focus::FocusMap {
    use crate::audit_focus::{BoundaryZoneFile, FocusInputs, build_focus_map};

    let Some(check) = result.check.as_ref() else {
        return crate::audit_focus::FocusMap::default();
    };
    let Some(graph_facts) = check.focus_facts.as_ref() else {
        return crate::audit_focus::FocusMap::default();
    };
    let root = &check.config.root;

    // Boundary risk-zone files: the importing `from_path` of each boundary
    // violation whose introduced zone-pair edge is in the delta set, deduped.
    let mut seen_pairs: FxHashSet<String> = FxHashSet::default();
    let mut boundary_files: Vec<BoundaryZoneFile> = Vec::new();
    for finding in &check.results.boundary_violations {
        let key = crate::audit::review_deltas::boundary_edge_key(finding);
        if !deltas.boundary_introduced.contains(&key) || !seen_pairs.insert(key) {
            continue;
        }
        boundary_files.push(BoundaryZoneFile {
            from_file: crate::audit::keys::relative_key_path(&finding.violation.from_path, root),
        });
    }

    // Coordination-gap changed files (the signature-change change-shape proxy):
    // the changed files whose contract is consumed outside the diff.
    let coordination_changed_files: Vec<String> = check
        .impact_closure
        .as_ref()
        .map(|c| {
            let mut files: Vec<String> = c
                .coordination_gap
                .iter()
                .map(|gap| gap.changed_file.clone())
                .collect();
            files.sort_unstable();
            files.dedup();
            files
        })
        .unwrap_or_default();

    // Security taint touch: the brief path carries no security findings (the taint
    // engine is the opt-in `fallow security` command), so this is empty today. The
    // seam is a pure function of this slice; it lights up when a security pass is
    // threaded onto the brief.
    let taint_touched_files = taint_touched_files(result.check.as_ref());

    // Runtime evidence (paid): `Some` only on the `--runtime-coverage` path.
    // It weights hot files and enables safe-skip; `None` in free mode, where the
    // focus map degrades to the deterministic no-runtime baseline byte-for-byte.
    let runtime_focus = build_runtime_focus(result, root);

    build_focus_map(&FocusInputs {
        graph_facts,
        boundary_files: &boundary_files,
        public_api_added: &deltas.public_api_added,
        coordination_changed_files: &coordination_changed_files,
        taint_touched_files: &taint_touched_files,
        runtime: runtime_focus.as_ref(),
        churn_files: &focus_churn_files(check, graph_facts),
        worsened_files: &focus_worsened_files(result, root),
        weak_test_reach_files: &focus_weak_test_reach_files(check, diff_index),
        mutation_survivors: check.brief_mutation_survivors.as_deref(),
    })
}

/// The churn of each focus unit file, from the churn pass of the brief. Empty
/// on a shallow clone, where the history window is incomplete. A test file
/// gets no churn: tests change with most source changes, so their commit
/// count does not tell where the risk is.
fn focus_churn_files(
    check: &crate::check::CheckResult,
    graph_facts: &[fallow_engine::module_graph::FocusFileFactsPaths],
) -> Vec<crate::audit_focus::ChurnFocusFile> {
    let Some(churn) = check
        .brief_churn
        .as_ref()
        .filter(|churn| !churn.shallow_clone)
    else {
        return Vec::new();
    };
    let root = &check.config.root;
    graph_facts
        .iter()
        .filter(|facts| !fallow_engine::test_paths::is_test_path_str(root, &facts.file))
        .filter_map(|facts| {
            let entry = churn.files.get(&root.join(&facts.file))?;
            Some(crate::audit_focus::ChurnFocusFile {
                file: facts.file.clone(),
                commits: entry.commits,
                weighted_commits: entry.weighted_commits,
            })
        })
        .collect()
}

/// The complexity findings with a grown metric against the base, counted by
/// root-relative file.
fn focus_worsened_files(
    result: &AuditResult,
    root: &std::path::Path,
) -> Vec<crate::audit_focus::FocusFileCount> {
    let Some(health) = result.health.as_ref() else {
        return Vec::new();
    };
    count_by_file(
        health
            .report
            .findings
            .iter()
            .filter(|finding| !finding.worsened_metrics.is_empty())
            .map(|finding| crate::audit::keys::relative_key_path(&finding.path, root)),
    )
}

/// The changed exports that no test runs for real, counted by file:
/// `mocked-only`, `none`, and `none-file`. A project without tests adds
/// nothing, because each of its exports is `none-file` and the class then
/// does not tell one file from another.
fn focus_weak_test_reach_files(
    check: &crate::check::CheckResult,
    diff_index: Option<&fallow_output::DiffIndex>,
) -> Vec<crate::audit_focus::FocusFileCount> {
    use fallow_output::TestReachClass;

    let (Some(diff_index), Some(reach)) = (diff_index, check.test_reach.as_ref()) else {
        return Vec::new();
    };
    if !reach.has_tests {
        return Vec::new();
    }
    count_by_file(
        reach
            .exports
            .iter()
            .filter(|export| {
                let key = diff_index.key_for_root_relative(&export.file);
                let (start, end) = export.declaration_lines;
                diff_index.range_overlaps_added(&key, u64::from(start), u64::from(end))
            })
            .filter(|export| {
                matches!(
                    classify_test_reach(check, reach.has_tests, export),
                    TestReachClass::MockedOnly | TestReachClass::None | TestReachClass::NoneFile
                )
            })
            .map(|export| export.file.clone()),
    )
}

/// Count the items of each file, sorted by file.
fn count_by_file(files: impl Iterator<Item = String>) -> Vec<crate::audit_focus::FocusFileCount> {
    let mut counts: std::collections::BTreeMap<String, u32> = std::collections::BTreeMap::new();
    for file in files {
        *counts.entry(file).or_default() += 1;
    }
    counts
        .into_iter()
        .map(|(file, count)| crate::audit_focus::FocusFileCount { file, count })
        .collect()
}

/// Build the per-file [`crate::audit_focus::RuntimeFocus`] from the
/// runtime-coverage health report, or `None` when the run carried no
/// `--runtime-coverage` data (free mode, where the focus map stays byte-identical
/// to the no-runtime baseline).
///
/// Hot files come from the report's `hot_paths` (peak invocation per file). Cold
/// files are the runtime-proven-unused ones: a file with at least one
/// `safe_to_delete` finding, NO finding of any other verdict, and no hot path.
///
/// Honest boundary: the report's `findings` omit `active` functions, so a file
/// can carry a live, executed function this signal never sees. `hot_paths` only
/// surfaces functions at/above the configured hot threshold (`min_invocations_hot`,
/// default 100, raisable via `--min-invocations-hot`), so an `active` function in
/// the `[low_traffic .. hot)` band shows up in NEITHER list:
/// such a file, if its only retained finding is `safe_to_delete`, is classified
/// cold here despite having run. This is why the cold signal is never trusted on
/// its own: the safe-skip label additionally requires zero static risk and no
/// confidence flag, applies only to a file already in the diff, and is always
/// advisory (the skip stays in the escape-hatch list, never hidden). Paths are
/// normalized to the brief's root-relative forward-slashed space so the
/// focus-map joins are byte-exact.
fn build_runtime_focus(
    result: &AuditResult,
    root: &std::path::Path,
) -> Option<crate::audit_focus::RuntimeFocus> {
    let report = result.health.as_ref()?.report.runtime_coverage.as_ref()?;

    let hot_pairs: Vec<(String, u64)> = report
        .hot_paths
        .iter()
        .map(|hot| {
            (
                crate::audit::keys::relative_key_path(&hot.path, root),
                hot.invocations,
            )
        })
        .collect();

    // Partition findings into safe_to_delete (cold candidate) vs any other verdict
    // (the disqualifier that keeps a mixed-verdict file out of the cold set).
    let mut safe_to_delete: FxHashSet<String> = FxHashSet::default();
    let mut other_verdict: FxHashSet<String> = FxHashSet::default();
    for finding in &report.findings {
        let file = crate::audit::keys::relative_key_path(&finding.path, root);
        if matches!(
            finding.verdict,
            fallow_output::RuntimeCoverageVerdict::SafeToDelete
        ) {
            safe_to_delete.insert(file);
        } else {
            other_verdict.insert(file);
        }
    }

    reconcile_runtime_focus(hot_pairs, &safe_to_delete, &other_verdict)
}

/// Reconcile the projected runtime signals into a [`crate::audit_focus::RuntimeFocus`]:
/// peak-aggregate hot invocations per file, and keep a file cold only when it has
/// a `safe_to_delete` finding, no other-verdict finding, and no hot path (so the
/// hot and cold lists are disjoint by construction). Returns `None` when both
/// lists are empty. Pure (no I/O), so the mixed-verdict exclusion, the
/// hot-excludes-cold filter, and the peak aggregation are unit-tested without
/// constructing a full health report.
pub(super) fn reconcile_runtime_focus(
    hot_pairs: Vec<(String, u64)>,
    safe_to_delete: &FxHashSet<String>,
    other_verdict: &FxHashSet<String>,
) -> Option<crate::audit_focus::RuntimeFocus> {
    use crate::audit_focus::{RuntimeFocus, RuntimeHotFile};

    // Peak invocation per hot file (max across the file's hot functions).
    let mut hot_by_file: rustc_hash::FxHashMap<String, u64> = rustc_hash::FxHashMap::default();
    for (file, invocations) in hot_pairs {
        let entry = hot_by_file.entry(file).or_insert(0);
        *entry = (*entry).max(invocations);
    }

    let mut hot_files: Vec<RuntimeHotFile> = hot_by_file
        .into_iter()
        .map(|(file, invocations)| RuntimeHotFile { file, invocations })
        .collect();
    hot_files.sort_by(|a, b| a.file.cmp(&b.file));
    let hot_set: FxHashSet<&str> = hot_files.iter().map(|hot| hot.file.as_str()).collect();

    let mut cold_files: Vec<String> = safe_to_delete
        .iter()
        .filter(|file| !other_verdict.contains(*file) && !hot_set.contains(file.as_str()))
        .cloned()
        .collect();
    cold_files.sort();

    if hot_files.is_empty() && cold_files.is_empty() {
        return None;
    }
    Some(RuntimeFocus {
        hot_files,
        cold_files,
    })
}

/// Collect the root-relative file paths a security source -> sink taint trace
/// touches, from any retained `security_findings` (anchor + every trace hop).
///
/// Today the brief path runs the bare dead-code analysis, so `security_findings`
/// is empty and this returns an empty Vec (the security-taint seam contributes
/// 0). The function is a pure projection over the findings slice, so the moment a
/// future epic threads a security pass onto the brief, the focus map's taint
/// signal lights up with no focus-map code change.
fn taint_touched_files(check: Option<&crate::check::CheckResult>) -> Vec<String> {
    let Some(check) = check else {
        return Vec::new();
    };
    let root = &check.config.root;
    let mut touched: FxHashSet<String> = FxHashSet::default();
    for finding in &check.results.security_findings {
        touched.insert(crate::audit::keys::relative_key_path(&finding.path, root));
        for hop in &finding.trace {
            touched.insert(crate::audit::keys::relative_key_path(&hop.path, root));
        }
    }
    let mut files: Vec<String> = touched.into_iter().collect();
    files.sort();
    files
}
