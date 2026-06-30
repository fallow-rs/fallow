use fallow_output::{ComplexityViolation, ExceededThreshold, FindingSeverity};

use super::HealthSort;

/// Sort findings by the specified criteria.
pub fn sort_findings(findings: &mut [ComplexityViolation], sort: HealthSort) {
    match sort {
        HealthSort::Severity => findings.sort_by_key(|finding| {
            std::cmp::Reverse((
                exceeded_priority(finding.exceeded),
                severity_priority(finding.severity),
                finding.crap.is_some(),
                finding.cyclomatic,
                finding.cognitive,
                finding.line_count,
            ))
        }),
        HealthSort::Cyclomatic => {
            findings.sort_by_key(|finding| std::cmp::Reverse(finding.cyclomatic));
        }
        HealthSort::Cognitive => {
            findings.sort_by_key(|finding| std::cmp::Reverse(finding.cognitive));
        }
        HealthSort::Lines => {
            findings.sort_by_key(|finding| std::cmp::Reverse(finding.line_count));
        }
    }
}

const fn exceeded_priority(exceeded: ExceededThreshold) -> u8 {
    match exceeded {
        ExceededThreshold::All => 5,
        ExceededThreshold::CyclomaticCrap | ExceededThreshold::CognitiveCrap => 4,
        ExceededThreshold::Crap => 3,
        ExceededThreshold::Both => 2,
        ExceededThreshold::Cyclomatic | ExceededThreshold::Cognitive => 1,
    }
}

const fn severity_priority(severity: FindingSeverity) -> u8 {
    match severity {
        FindingSeverity::Critical => 3,
        FindingSeverity::High => 2,
        FindingSeverity::Moderate => 1,
    }
}
