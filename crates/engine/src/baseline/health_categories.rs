//! Finding categories of a health baseline and the count match over them.
//!
//! A count bucket holds one tally per category, for example `complexity_high`.
//! A current finding is new when its category in its bucket has more findings
//! than the baseline allows. A saved slot also covers a lower severity.

use rustc_hash::{FxHashMap, FxHashSet};
use std::collections::BTreeMap;
use std::path::Path;

use super::{
    HealthBaselineCount, HealthBaselineData, HealthBaselineMode, HealthFindingCountMap,
    effective_identity_counts, health_bucket_key, health_finding_key,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum HealthFindingDimension {
    Complexity,
    Crap,
    FileLength,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct HealthFindingCategory {
    pub(super) dimension: HealthFindingDimension,
    pub(super) severity: fallow_output::FindingSeverity,
}

impl HealthFindingCategory {
    pub(super) const fn key(self) -> &'static str {
        match (self.dimension, self.severity) {
            (HealthFindingDimension::Complexity, fallow_output::FindingSeverity::Moderate) => {
                "complexity_moderate"
            }
            (HealthFindingDimension::Complexity, fallow_output::FindingSeverity::High) => {
                "complexity_high"
            }
            (HealthFindingDimension::Complexity, fallow_output::FindingSeverity::Critical) => {
                "complexity_critical"
            }
            (HealthFindingDimension::Crap, fallow_output::FindingSeverity::Moderate) => {
                "crap_moderate"
            }
            (HealthFindingDimension::Crap, fallow_output::FindingSeverity::High) => "crap_high",
            (HealthFindingDimension::Crap, fallow_output::FindingSeverity::Critical) => {
                "crap_critical"
            }
            (HealthFindingDimension::FileLength, fallow_output::FindingSeverity::Moderate) => {
                "file_lines_moderate"
            }
            (HealthFindingDimension::FileLength, fallow_output::FindingSeverity::High) => {
                "file_lines_high"
            }
            (HealthFindingDimension::FileLength, fallow_output::FindingSeverity::Critical) => {
                "file_lines_critical"
            }
        }
    }
}

pub(super) const HEALTH_FINDING_DIMENSIONS: [HealthFindingDimension; 3] = [
    HealthFindingDimension::Complexity,
    HealthFindingDimension::Crap,
    HealthFindingDimension::FileLength,
];

/// The category that a health baseline gives a file-length finding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum FileLengthCounting {
    /// `file_lines_<severity>`, which every save of this version writes. A
    /// function finding that the baseline accepted and that was fixed then
    /// leaves no slot for a file that became too long.
    Separate,
    /// `complexity_<severity>`, which a baseline saved before the
    /// `file_lines_categories` marker holds. A run reads such a file as it
    /// was saved, so the save of a new version does not report its long files
    /// as new.
    AsComplexity,
}

pub(super) fn health_finding_counts(
    findings: &[fallow_output::ComplexityViolation],
    root: &Path,
    mode: HealthBaselineMode,
    counting: FileLengthCounting,
) -> HealthFindingCountMap {
    let mut counts = BTreeMap::new();
    for finding in findings {
        let bucket = health_bucket_key(finding, root, mode);
        let file_counts = counts.entry(bucket).or_insert_with(BTreeMap::new);
        for category in health_finding_categories(finding, counting)
            .into_iter()
            .flatten()
        {
            file_counts
                .entry(category.key().to_string())
                .and_modify(|entry: &mut HealthBaselineCount| entry.count += 1)
                .or_insert(HealthBaselineCount { count: 1 });
        }
    }
    counts
}

fn health_finding_categories(
    finding: &fallow_output::ComplexityViolation,
    counting: FileLengthCounting,
) -> [Option<HealthFindingCategory>; 2] {
    let category = |dimension| HealthFindingCategory {
        dimension,
        severity: finding.severity,
    };
    if finding.exceeded.is_file_lines() {
        let dimension = match counting {
            FileLengthCounting::Separate => HealthFindingDimension::FileLength,
            FileLengthCounting::AsComplexity => HealthFindingDimension::Complexity,
        };
        return [Some(category(dimension)), None];
    }
    let has_complexity =
        finding.exceeded.includes_cyclomatic() || finding.exceeded.includes_cognitive();
    let has_crap = finding.exceeded.includes_crap();
    [
        has_complexity.then(|| category(HealthFindingDimension::Complexity)),
        has_crap.then(|| category(HealthFindingDimension::Crap)),
    ]
}

pub(super) fn severity_index(severity: fallow_output::FindingSeverity) -> usize {
    match severity {
        fallow_output::FindingSeverity::Moderate => 0,
        fallow_output::FindingSeverity::High => 1,
        fallow_output::FindingSeverity::Critical => 2,
    }
}

pub(super) fn severity_counts_for_dimension(
    file_counts: Option<&BTreeMap<String, HealthBaselineCount>>,
    dimension: HealthFindingDimension,
) -> [usize; 3] {
    let mut counts = [0; 3];
    for severity in [
        fallow_output::FindingSeverity::Moderate,
        fallow_output::FindingSeverity::High,
        fallow_output::FindingSeverity::Critical,
    ] {
        let category = HealthFindingCategory {
            dimension,
            severity,
        };
        counts[severity_index(severity)] = file_counts
            .and_then(|entries| entries.get(category.key()))
            .map_or(0, |entry| entry.count);
    }
    counts
}

pub(super) fn overflowing_severities(current: [usize; 3], baseline: [usize; 3]) -> [bool; 3] {
    let mut available = baseline;
    let mut overflow = [false; 3];

    for severity_idx in 0..3 {
        let compatible = available[severity_idx..].iter().sum::<usize>();
        overflow[severity_idx] = compatible < current[severity_idx];

        let mut matched = current[severity_idx].min(compatible);
        for slot in available.iter_mut().skip(severity_idx) {
            let taken = matched.min(*slot);
            *slot -= taken;
            matched -= taken;
            if matched == 0 {
                break;
            }
        }
    }

    overflow
}

fn health_overflow_categories(
    current_counts: &HealthFindingCountMap,
    baseline_counts: &HealthFindingCountMap,
) -> FxHashMap<String, FxHashSet<&'static str>> {
    let mut overflow_by_path = FxHashMap::default();

    for (path, current_file_counts) in current_counts {
        let mut overflow_categories: FxHashSet<&'static str> = FxHashSet::default();
        let baseline_file_counts = baseline_counts.get(path);

        for dimension in HEALTH_FINDING_DIMENSIONS {
            let current = severity_counts_for_dimension(Some(current_file_counts), dimension);
            let baseline = severity_counts_for_dimension(baseline_file_counts, dimension);
            let overflow = overflowing_severities(current, baseline);

            for severity in [
                fallow_output::FindingSeverity::Moderate,
                fallow_output::FindingSeverity::High,
                fallow_output::FindingSeverity::Critical,
            ] {
                if overflow[severity_index(severity)] {
                    overflow_categories.insert(
                        HealthFindingCategory {
                            dimension,
                            severity,
                        }
                        .key(),
                    );
                }
            }
        }

        if !overflow_categories.is_empty() {
            overflow_by_path.insert(path.clone(), overflow_categories);
        }
    }

    overflow_by_path
}

pub(super) fn health_overlap_entry_count(
    current_counts: &HealthFindingCountMap,
    baseline_counts: &HealthFindingCountMap,
) -> usize {
    let mut overlap = 0;

    for (path, baseline_file_counts) in baseline_counts {
        let current_file_counts = current_counts.get(path);

        for dimension in HEALTH_FINDING_DIMENSIONS {
            let current_total: usize =
                severity_counts_for_dimension(current_file_counts, dimension)
                    .into_iter()
                    .sum();
            let baseline_total: usize =
                severity_counts_for_dimension(Some(baseline_file_counts), dimension)
                    .into_iter()
                    .sum();
            overlap += current_total.min(baseline_total);
        }
    }

    overlap
}

impl HealthBaselineData {
    /// The category that this file gives a file-length finding.
    pub(super) const fn file_length_counting(&self) -> FileLengthCounting {
        if self.file_lines_categories {
            FileLengthCounting::Separate
        } else {
            FileLengthCounting::AsComplexity
        }
    }
}

/// Filter health findings to only include those not present in the baseline.
pub fn filter_new_health_findings(
    mut findings: Vec<fallow_output::ComplexityViolation>,
    baseline: &HealthBaselineData,
    root: &Path,
    mode: HealthBaselineMode,
) -> Vec<fallow_output::ComplexityViolation> {
    let baseline_counts = baseline.counts_for(mode);
    if !baseline_counts.is_empty() {
        let counting = baseline.file_length_counting();
        let current_counts = health_finding_counts(&findings, root, mode, counting);
        let remapped = (mode == HealthBaselineMode::Identity)
            .then(|| effective_identity_counts(baseline_counts, &current_counts, root))
            .flatten();
        let overflow_categories = health_overflow_categories(
            &current_counts,
            remapped.as_ref().unwrap_or(baseline_counts),
        );
        findings.retain(|finding| {
            let bucket = health_bucket_key(finding, root, mode);
            overflow_categories.get(&bucket).is_some_and(|categories| {
                health_finding_categories(finding, counting)
                    .into_iter()
                    .flatten()
                    .any(|category| categories.contains(category.key()))
            })
        });
        return findings;
    }

    let baseline_keys: FxHashSet<&str> = baseline.findings.iter().map(String::as_str).collect();
    findings.retain(|f| {
        let key = health_finding_key(f, root);
        !baseline_keys.contains(key.as_str())
    });
    findings
}
