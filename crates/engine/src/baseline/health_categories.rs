//! Finding categories of a health baseline and the counts over them.
//!
//! A count bucket holds one tally per category, for example `complexity_high`.

use std::collections::BTreeMap;
use std::path::Path;

use super::{
    HealthBaselineCount, HealthBaselineData, HealthBaselineMode, HealthFindingCountMap,
    health_bucket_key,
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

pub(super) fn health_finding_categories(
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
