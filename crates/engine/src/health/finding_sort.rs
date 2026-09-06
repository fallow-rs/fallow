use fallow_output::{ComplexityViolation, ExceededThreshold, FindingSeverity};

use super::HealthSort;

/// Sort findings by the specified criteria.
pub fn sort_findings(findings: &mut [ComplexityViolation], sort: HealthSort) {
    findings.sort_by(|left, right| {
        let metric_order = match sort {
            HealthSort::Severity => {
                let priority = |finding: &ComplexityViolation| {
                    (
                        exceeded_priority(finding.exceeded),
                        severity_priority(finding.severity),
                        finding.crap.is_some(),
                        finding.cyclomatic,
                        finding.cognitive,
                        finding.line_count,
                    )
                };
                priority(right).cmp(&priority(left))
            }
            HealthSort::Cyclomatic => right.cyclomatic.cmp(&left.cyclomatic),
            HealthSort::Cognitive => right.cognitive.cmp(&left.cognitive),
            HealthSort::Lines => right.line_count.cmp(&left.line_count),
        };
        // Discovery order can change with the checkout root or parser threads.
        metric_order.then_with(|| {
            (&left.path, left.line, left.col, &left.name).cmp(&(
                &right.path,
                right.line,
                right.col,
                &right.name,
            ))
        })
    });
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
