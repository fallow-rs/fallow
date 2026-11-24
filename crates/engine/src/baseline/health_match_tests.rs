use std::path::Path;

use fallow_output::{ExceededThreshold, FindingSeverity};

use super::super::tests::make_health_finding_with;
use super::*;
use crate::baseline::{HealthBaselineData, HealthBaselineMode, filter_new_health_findings};

const ROOT: &str = "/project";

/// A complexity finding of the function `name` in `src/utils.ts`.
fn finding(name: &str, severity: FindingSeverity) -> fallow_output::ComplexityViolation {
    make_health_finding_with(
        Path::new(ROOT),
        name,
        1,
        ExceededThreshold::Cyclomatic,
        severity,
    )
}

/// A baseline saved the way `--baseline-mode identity` saves it.
fn identity_baseline(findings: &[fallow_output::ComplexityViolation]) -> HealthBaselineData {
    HealthBaselineData::from_findings(findings, &[], &[], Path::new(ROOT))
        .with_identity(findings, Path::new(ROOT))
}

#[test]
fn used_slots_follow_the_greedy_match() {
    let used = |current, saved| match_severity_slots(current, saved).used;
    assert_eq!(used([0, 1, 0], [1, 1, 0]), [0, 1, 0]);
    assert_eq!(used([1, 0, 0], [0, 0, 1]), [0, 0, 1]);
    assert_eq!(used([0, 0, 1], [1, 0, 0]), [0, 0, 0]);
    assert_eq!(used([2, 1, 0], [1, 1, 2]), [1, 1, 1]);
    assert_eq!(used([5, 5, 5], [1, 1, 1]), [1, 1, 1]);
}

#[test]
fn a_severity_overflows_when_no_compatible_slot_is_free() {
    let overflow = |current, saved| match_severity_slots(current, saved).overflow;
    assert_eq!(overflow([1, 0, 0], [0, 0, 1]), [false, false, false]);
    assert_eq!(overflow([0, 0, 1], [1, 1, 0]), [false, false, true]);
    assert_eq!(overflow([1, 1, 0], [0, 1, 0]), [false, true, false]);
    assert_eq!(overflow([2, 0, 0], [1, 0, 0]), [true, false, false]);
}

#[test]
fn a_placeholder_slot_that_covers_its_own_finding_is_not_lent() {
    let baseline = identity_baseline(&[finding("<arrow>", FindingSeverity::Critical)]);
    let filtered = filter_new_health_findings(
        vec![
            finding("<arrow>", FindingSeverity::Moderate),
            finding("newHotspot", FindingSeverity::Critical),
        ],
        &baseline,
        Path::new(ROOT),
        HealthBaselineMode::Identity,
    );
    assert_eq!(filtered.len(), 1, "one saved slot hides one finding");
    assert_eq!(filtered[0].name, "newHotspot");
}

#[test]
fn a_slot_below_the_current_severity_is_not_a_matched_entry() {
    let root = Path::new(ROOT);
    let baseline = HealthBaselineData::from_findings(
        &[finding("parse", FindingSeverity::High)],
        &[],
        &[],
        root,
    );
    let current = vec![finding("parse", FindingSeverity::Critical)];
    let overlap = baseline.overlap_entries(&current, root, HealthBaselineMode::Count);
    assert_eq!(overlap.matched_entries, 0, "the filter reports the finding");
    assert_eq!(overlap.moved_entries, 0);
    let filtered = filter_new_health_findings(current, &baseline, root, HealthBaselineMode::Count);
    assert_eq!(filtered.len(), 1);
}

#[test]
fn compare_gives_the_filter_and_the_overlap_of_one_match() {
    let root = Path::new(ROOT);
    let saved = [finding("parse", FindingSeverity::High)];
    let current = vec![
        finding("parse", FindingSeverity::High),
        finding("render", FindingSeverity::High),
    ];

    let count = HealthBaselineData::from_findings(&saved, &[], &[], root);
    let compared = count.compare(current.clone(), root, HealthBaselineMode::Count);
    assert_eq!(
        compared.new_findings.len(),
        2,
        "count mode reports every finding of a category that overflows"
    );
    assert_eq!(compared.overlap.matched_entries, 1);

    let identity = identity_baseline(&saved);
    let compared = identity.compare(current.clone(), root, HealthBaselineMode::Identity);
    assert_eq!(compared.new_findings.len(), 1);
    assert_eq!(compared.new_findings[0].name, "render");
    assert_eq!(compared.overlap.matched_entries, 1);

    let legacy: HealthBaselineData = serde_json::from_value(serde_json::json!({
        "findings": ["src/utils.ts:parse:1"],
    }))
    .unwrap();
    let compared = legacy.compare(current, root, HealthBaselineMode::Count);
    assert_eq!(compared.new_findings.len(), 1);
    assert_eq!(compared.new_findings[0].name, "render");
    assert_eq!(compared.overlap.matched_entries, 1);
}

mod invariants {
    use proptest::prelude::*;

    use super::super::*;

    const CATEGORY_KEYS: [&str; 9] = [
        "complexity_moderate",
        "complexity_high",
        "complexity_critical",
        "crap_moderate",
        "crap_high",
        "crap_critical",
        "file_lines_moderate",
        "file_lines_high",
        "file_lines_critical",
    ];
    const FILE_BUCKETS: [&str; 3] = ["src/a.ts", "src/b.ts", "src/c.ts"];
    /// One file, so no bucket follows a move.
    const FUNCTION_BUCKETS: [&str; 5] = [
        "src/a.ts\0<anonymous>",
        "src/a.ts\0<arrow>",
        "src/a.ts\0f",
        "src/a.ts\0g",
        "src/a.ts\0h",
    ];

    fn count_map(buckets: &'static [&'static str]) -> impl Strategy<Value = HealthFindingCountMap> {
        proptest::collection::btree_map(
            proptest::sample::select(buckets),
            proptest::collection::btree_map(
                proptest::sample::select(&CATEGORY_KEYS[..]),
                1usize..=3,
                0..=4,
            ),
            0..=4,
        )
        .prop_map(|buckets| {
            buckets
                .into_iter()
                .map(|(bucket, counts)| {
                    let counts = counts
                        .into_iter()
                        .map(|(category, count)| {
                            (category.to_owned(), HealthBaselineCount { count })
                        })
                        .collect();
                    (bucket.to_owned(), counts)
                })
                .collect()
        })
    }

    fn total(counts: &HealthFindingCountMap) -> usize {
        counts.values().map(slot_total).sum()
    }

    fn check(
        saved: &HealthFindingCountMap,
        current: &HealthFindingCountMap,
        mode: HealthBaselineMode,
    ) -> Result<(), TestCaseError> {
        let root = Path::new("/nonexistent-fallow-health-match");
        let matched = match_health_baseline(saved, current, mode, root);
        let mut kept = HealthFindingCountMap::new();
        for (bucket, counts) in saved {
            for (category, entry) in counts {
                let used = matched.used_slots(bucket, category);
                prop_assert!(used <= entry.count, "a slot is used at most one time");
                if used > 0 {
                    add_slots(&mut kept, bucket, category, used);
                }
            }
        }
        let matched_entries = matched.overlap().matched_entries;
        prop_assert!(
            matched_entries <= total(current),
            "no more matches than findings"
        );
        prop_assert_eq!(matched_entries, total(&kept));

        let pruned = match_health_baseline(&kept, current, mode, root);
        prop_assert_eq!(pruned.overlap().matched_entries, total(&kept));
        for bucket in current.keys() {
            for category in CATEGORY_KEYS {
                prop_assert_eq!(
                    matched.overflows(bucket, category),
                    pruned.overflows(bucket, category),
                    "the prune changes no overflow"
                );
            }
        }
        Ok(())
    }

    proptest! {
        #[test]
        fn count_match_is_stable_under_prune(
            saved in count_map(&FILE_BUCKETS),
            current in count_map(&FILE_BUCKETS),
        ) {
            check(&saved, &current, HealthBaselineMode::Count)?;
        }

        #[test]
        fn identity_match_is_stable_under_prune(
            saved in count_map(&FUNCTION_BUCKETS),
            current in count_map(&FUNCTION_BUCKETS),
        ) {
            check(&saved, &current, HealthBaselineMode::Identity)?;
        }
    }
}
