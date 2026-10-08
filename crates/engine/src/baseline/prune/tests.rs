use std::path::{Path, PathBuf};

use fallow_output::{ExceededThreshold, FindingSeverity};
use fallow_types::semantic::SemanticAnalysisIdentity;

use super::super::tests::{
    make_clone_group, make_duplication_report, make_health_finding_with, make_results,
};
use super::*;
use crate::baseline::{
    DeadCodeBaselineOutcome, apply_dead_code_baseline, filter_new_clone_groups,
    filter_new_health_findings, filter_new_issues,
};

const ROOT: &str = "/project";

fn dead_code_content(results: &crate::results::AnalysisResults) -> String {
    serde_json::to_string_pretty(&BaselineData::from_results(results, Path::new(ROOT))).unwrap()
}

fn prune_dead_code(
    content: &str,
    results: &crate::results::AnalysisResults,
) -> Result<BaselinePrune, BaselinePruneRefusal> {
    prune_dead_code_baseline(
        content,
        results,
        Path::new(ROOT),
        &SemanticAnalysisIdentity::syntactic(),
    )
}

#[test]
fn dead_code_prune_leaves_a_fresh_save_unchanged() {
    let results = make_results();
    let pruned = prune_dead_code(&dead_code_content(&results), &results).unwrap();
    assert!(pruned.removed.is_empty());
    assert_eq!(pruned.content, None);
    assert_eq!(pruned.entries_before, pruned.entries_after);
}

#[test]
fn dead_code_prune_removes_a_fixed_finding_and_hides_the_same_findings() {
    let saved = make_results();
    let content = dead_code_content(&saved);
    let mut current = make_results();
    current.unused_files.pop();

    let pruned = prune_dead_code(&content, &current).unwrap();
    assert_eq!(pruned.removed.len(), 1);
    assert_eq!(pruned.removed[0].category, "unused_files");
    assert_eq!(pruned.entries_after, pruned.entries_before - 1);
    let new_content = pruned.content.expect("a pruned file has new content");

    let original: BaselineData = serde_json::from_str(&content).unwrap();
    let rewritten: BaselineData = serde_json::from_str(&new_content).unwrap();
    let root = Path::new(ROOT);
    assert_eq!(
        filter_new_issues(current.clone(), &original, root).total_issues(),
        filter_new_issues(current.clone(), &rewritten, root).total_issues(),
    );

    let mut applied = current;
    let outcome = apply_dead_code_baseline(
        &mut applied,
        &new_content,
        root,
        &SemanticAnalysisIdentity::syntactic(),
        false,
    )
    .unwrap();
    let DeadCodeBaselineOutcome::Applied { staleness, .. } = outcome else {
        panic!("a pruned dead-code baseline stays a dead-code baseline");
    };
    assert_eq!(staleness.stale_entries(), 0, "{staleness:?}");
}

/// The saved results, with every export and member finding moved to the
/// cascade state, as the rule pass does for the findings of an unused file.
fn results_with_cascade_hidden() -> crate::results::AnalysisResults {
    let mut current = make_results();
    current.hide_cascade_findings(&|_| true);
    assert!(current.cascade_hidden > 0, "the fixture must hide findings");
    current
}

#[test]
fn dead_code_prune_keeps_entries_of_cascade_hidden_findings() {
    let content = dead_code_content(&make_results());

    let pruned = prune_dead_code(&content, &results_with_cascade_hidden()).unwrap();

    assert!(pruned.removed.is_empty(), "{:?}", pruned.removed);
    assert_eq!(pruned.content, None);
}

#[test]
fn cascade_hidden_findings_keep_their_baseline_entries_fresh() {
    let content = dead_code_content(&make_results());
    let mut current = results_with_cascade_hidden();

    let outcome = apply_dead_code_baseline(
        &mut current,
        &content,
        Path::new(ROOT),
        &SemanticAnalysisIdentity::syntactic(),
        false,
    )
    .unwrap();

    let DeadCodeBaselineOutcome::Applied { staleness, .. } = outcome else {
        panic!("a saved dead-code baseline stays a dead-code baseline");
    };
    assert_eq!(staleness.stale_entries(), 0, "{staleness:?}");
    assert!(!staleness.trips_gate(), "{staleness:?}");
    assert_eq!(current.total_issues(), 0);
}

#[test]
fn dead_code_prune_never_adds_a_new_finding() {
    let saved = make_results();
    let content = dead_code_content(&saved);
    let mut current = make_results();
    current.unused_files.push(current.unused_files[0].clone());
    current.unused_files[2].file.path = PathBuf::from("src/new.ts");

    let pruned = prune_dead_code(&content, &current).unwrap();
    assert!(pruned.removed.is_empty());
    assert_eq!(pruned.content, None);
}

#[test]
fn dead_code_prune_refuses_legacy_keys() {
    let results = make_results();
    let legacy = BaselineData::legacy_from_results(&results, Path::new(ROOT));
    let content = serde_json::to_string(&legacy).unwrap();
    assert_eq!(
        prune_dead_code(&content, &results),
        Err(BaselinePruneRefusal::LegacyKeys)
    );
}

#[test]
fn dead_code_prune_refuses_another_kind() {
    let report = make_duplication_report(vec![make_clone_group(vec![
        ("/project/src/a.ts", 1, 10),
        ("/project/src/b.ts", 1, 10),
    ])]);
    let content = serde_json::to_string(&DuplicationBaselineData::from_report(
        &report,
        Path::new(ROOT),
    ))
    .unwrap();
    assert_eq!(
        prune_dead_code(&content, &make_results()),
        Err(BaselinePruneRefusal::NotThisKind {
            saved_by: Some(BaselineKind::Dupes)
        })
    );
}

fn three_clone_groups() -> Vec<crate::duplicates::CloneGroup> {
    vec![
        make_clone_group(vec![
            ("/project/src/a.ts", 1, 10),
            ("/project/src/b.ts", 1, 10),
        ]),
        make_clone_group(vec![
            ("/project/src/c.ts", 1, 10),
            ("/project/src/d.ts", 1, 10),
        ]),
        make_clone_group(vec![
            ("/project/src/e.ts", 1, 10),
            ("/project/src/f.ts", 1, 10),
        ]),
    ]
}

#[test]
fn dupes_prune_removes_whole_rows() {
    let root = Path::new(ROOT);
    let saved = make_duplication_report(three_clone_groups());
    let content =
        serde_json::to_string_pretty(&DuplicationBaselineData::from_report(&saved, root)).unwrap();
    let mut groups = three_clone_groups();
    groups.remove(1);
    let current = make_duplication_report(groups);

    let pruned = prune_dupes_baseline(&content, &current).unwrap();
    assert_eq!(pruned.removed.len(), 1);
    assert_eq!((pruned.entries_before, pruned.entries_after), (3, 2));
    let rewritten: DuplicationBaselineData =
        serde_json::from_str(&pruned.content.expect("rows were removed")).unwrap();
    assert_eq!(rewritten.normalized_clone_fingerprints.len(), 2);
    assert_eq!(rewritten.clone_fingerprints.len(), 2);
    assert_eq!(rewritten.clone_groups.len(), 2);
    assert!(
        rewritten
            .clone_groups
            .iter()
            .all(|key| !key.contains("src/c.ts")),
        "{:?}",
        rewritten.clone_groups
    );
    assert!(
        filter_new_clone_groups(current, &rewritten, root)
            .clone_groups
            .is_empty()
    );
}

#[test]
fn dupes_prune_leaves_a_fresh_save_unchanged() {
    let report = make_duplication_report(three_clone_groups());
    let content = serde_json::to_string(&DuplicationBaselineData::from_report(
        &report,
        Path::new(ROOT),
    ))
    .unwrap();
    let pruned = prune_dupes_baseline(&content, &report).unwrap();
    assert!(pruned.removed.is_empty());
    assert_eq!(pruned.content, None);
}

#[test]
fn dupes_prune_refuses_shared_clone_keys() {
    let content =
        r#"{"kind":"dupes","normalized_clone_fingerprints":["dup:c77b3abb6f87acd9-r1:2"]}"#;
    assert_eq!(
        prune_dupes_baseline(content, &make_duplication_report(Vec::new())),
        Err(BaselinePruneRefusal::SharedCloneKeys)
    );
}

fn finding(name: &str, severity: FindingSeverity) -> fallow_output::ComplexityViolation {
    make_health_finding_with(
        Path::new(ROOT),
        name,
        1,
        ExceededThreshold::Cyclomatic,
        severity,
    )
}

fn health_content(baseline: &HealthBaselineData) -> String {
    serde_json::to_string_pretty(baseline).unwrap()
}

#[test]
fn health_prune_keeps_the_slots_that_cover_current_findings() {
    let root = Path::new(ROOT);
    let saved = [
        finding("a", FindingSeverity::Moderate),
        finding("b", FindingSeverity::High),
    ];
    let content = health_content(&HealthBaselineData::from_findings(&saved, &[], &[], root));
    let current = vec![finding("b", FindingSeverity::High)];

    let pruned = prune_health_baseline(&content, &current, root).unwrap();
    assert_eq!((pruned.entries_before, pruned.entries_after), (2, 1));
    assert_eq!(pruned.removed.len(), 1);
    assert_eq!(
        pruned.removed[0].category,
        "finding_counts.complexity_moderate"
    );
    let original: HealthBaselineData = serde_json::from_str(&content).unwrap();
    let rewritten: HealthBaselineData =
        serde_json::from_str(&pruned.content.expect("a slot was removed")).unwrap();
    let names = |baseline: &HealthBaselineData| -> Vec<String> {
        filter_new_health_findings(current.clone(), baseline, root, HealthBaselineMode::Count)
            .into_iter()
            .map(|finding| finding.name)
            .collect()
    };
    assert_eq!(names(&original), names(&rewritten));
    let overlap = rewritten.overlap_entries(&current, root, HealthBaselineMode::Count);
    assert_eq!(overlap.matched_entries, rewritten.finding_entry_count());
}

#[test]
fn health_prune_keeps_a_higher_slot_that_covers_a_lower_finding() {
    let root = Path::new(ROOT);
    let saved = [finding("a", FindingSeverity::Critical)];
    let content = health_content(&HealthBaselineData::from_findings(&saved, &[], &[], root));
    let current = vec![finding("a", FindingSeverity::Moderate)];

    let pruned = prune_health_baseline(&content, &current, root).unwrap();
    assert!(pruned.removed.is_empty(), "{:?}", pruned.removed);
}

#[test]
fn health_prune_removes_a_bucket_whose_findings_are_gone() {
    let root = Path::new(ROOT);
    let saved = [finding("a", FindingSeverity::High)];
    let content = health_content(&HealthBaselineData::from_findings(&saved, &[], &[], root));

    let pruned = prune_health_baseline(&content, &[], root).unwrap();
    assert_eq!(pruned.entries_after, 0);
    let rewritten: HealthBaselineData =
        serde_json::from_str(&pruned.content.expect("the bucket was removed")).unwrap();
    assert!(rewritten.finding_counts.is_empty());
}

#[test]
fn health_prune_keeps_the_saved_key_of_a_moved_identity_bucket() {
    let root = Path::new("/nonexistent-fallow-prune-root");
    let mut saved = make_health_finding_with(
        root,
        "parseConfig",
        1,
        ExceededThreshold::Cyclomatic,
        FindingSeverity::High,
    );
    saved.path = root.join("src/old.ts");
    let baseline = HealthBaselineData::from_findings(std::slice::from_ref(&saved), &[], &[], root)
        .with_identity(std::slice::from_ref(&saved), root);
    let content = health_content(&baseline);
    let mut moved = saved;
    moved.path = root.join("src/new.ts");

    let pruned = prune_health_baseline(&content, &[moved], root).unwrap();
    let identity_removed: Vec<_> = pruned
        .removed
        .iter()
        .filter(|entry| entry.category.starts_with("identity_finding_counts"))
        .collect();
    assert!(identity_removed.is_empty(), "{identity_removed:?}");
}

#[test]
fn health_prune_refuses_legacy_keys() {
    let content = r#"{"findings":["src/a.ts:f:1"]}"#;
    assert_eq!(
        prune_health_baseline(content, &[], Path::new(ROOT)),
        Err(BaselinePruneRefusal::LegacyKeys)
    );
}

#[test]
fn consumed_slots_follow_the_greedy_match() {
    assert_eq!(consumed_severity_slots([0, 1, 0], [1, 1, 0]), [0, 1, 0]);
    assert_eq!(consumed_severity_slots([1, 0, 0], [0, 0, 1]), [0, 0, 1]);
    assert_eq!(consumed_severity_slots([0, 0, 1], [1, 0, 0]), [0, 0, 0]);
    assert_eq!(consumed_severity_slots([2, 1, 0], [1, 1, 2]), [1, 1, 1]);
    assert_eq!(consumed_severity_slots([5, 5, 5], [1, 1, 1]), [1, 1, 1]);
}

fn located(
    root: &Path,
    path: &str,
    exceeded: ExceededThreshold,
) -> fallow_output::ComplexityViolation {
    let mut finding = make_health_finding_with(root, "f", 1, exceeded, FindingSeverity::High);
    finding.path = root.join(path);
    finding
}

#[test]
fn health_prune_keeps_an_emptied_identity_key_that_blocks_a_second_move_candidate() {
    let root = Path::new("/nonexistent-fallow-prune-root");
    let saved = [
        located(root, "src/old.ts", ExceededThreshold::Cyclomatic),
        located(root, "src/a.ts", ExceededThreshold::Crap),
    ];
    let baseline =
        HealthBaselineData::from_findings(&saved, &[], &[], root).with_identity(&saved, root);
    let content = health_content(&baseline);
    let current = vec![
        located(root, "src/x.ts", ExceededThreshold::Cyclomatic),
        located(root, "src/a.ts", ExceededThreshold::Cyclomatic),
    ];

    let pruned = prune_health_baseline(&content, &current, root).unwrap();
    let rewritten: HealthBaselineData =
        serde_json::from_str(&pruned.content.expect("the crap slot was removed")).unwrap();
    let hidden = |baseline: &HealthBaselineData| -> Vec<String> {
        filter_new_health_findings(
            current.clone(),
            baseline,
            root,
            HealthBaselineMode::Identity,
        )
        .into_iter()
        .map(|finding| finding.path.display().to_string())
        .collect()
    };
    assert_eq!(hidden(&baseline), hidden(&rewritten));
}

#[test]
fn prune_refuses_a_file_with_unknown_fields() {
    let results = make_results();
    let mut value: serde_json::Value = serde_json::from_str(&dead_code_content(&results)).unwrap();
    value["future_findings"] = serde_json::json!(["x"]);
    assert_eq!(
        prune_dead_code(&value.to_string(), &results),
        Err(BaselinePruneRefusal::UnknownFields(vec![
            "future_findings".to_owned()
        ]))
    );
}
