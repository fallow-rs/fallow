//! Human-output notes that say where the CRAP scores of a health run come from.

pub(super) fn crap_coverage_note(report: &fallow_output::HealthReport) -> Option<String> {
    if !report.findings.iter().any(|finding| finding.crap.is_some()) {
        return None;
    }

    let istanbul_counts = (
        report.summary.istanbul_matched,
        report.summary.istanbul_total,
    );
    let has_istanbul_counts = matches!(istanbul_counts, (Some(_), Some(total)) if total > 0);

    if matches!(
        report.summary.coverage_model,
        Some(fallow_output::CoverageModel::Istanbul)
    ) || has_istanbul_counts
    {
        let match_info = match (
            report.summary.istanbul_matched,
            report.summary.istanbul_total,
        ) {
            (Some(matched), Some(total)) if total > 0 && matched < total => {
                return Some(format!(
                    "CRAP scores use {} coverage where matched ({matched}/{total} functions); the rest fall back to the static estimate.",
                    measured_coverage_label(report)
                ));
            }
            (Some(matched), Some(total)) if total > 0 => {
                format!(" ({matched}/{total} functions matched)")
            }
            _ => String::new(),
        };
        return Some(format!(
            "CRAP scores use {} coverage data{match_info}.",
            measured_coverage_label(report)
        ));
    }

    Some(STATIC_CRAP_ESTIMATE_NOTE.to_string())
}

/// Findings note for a run that scored CRAP without a coverage file. The
/// `--explain` block keys on this text, so keep both in step.
pub(super) const STATIC_CRAP_ESTIMATE_NOTE: &str = "No coverage file found, so CRAP scores are a static estimate. Pass `--coverage <coverage-final.json>` for measured scores, or raise `health.maxCrap`.";

/// `--explain` text for [`STATIC_CRAP_ESTIMATE_NOTE`].
pub(super) const STATIC_CRAP_ESTIMATE_EXPLAIN: &str = "CRAP is cc^2 * (1 - coverage)^3 + cc. Without coverage, fallow estimates the coverage from the module graph: 85% for an export that a test imports, 40% for other code in a file that tests reach, and 0% for the rest. So an untested function scores cc^2 + cc, and a function with cyclomatic complexity 5 already scores 30. Measured coverage usually lowers the score. If the estimate is too strict for code without tests, raise `health.maxCrap` or set it to 0 to turn the CRAP check off.";

/// Name of the coverage input behind measured CRAP scores.
fn measured_coverage_label(report: &fallow_output::HealthReport) -> &'static str {
    match report.summary.coverage_input_format {
        Some(fallow_output::CoverageInputFormat::V8) => "V8",
        _ => "Istanbul",
    }
}

pub(super) fn file_scores_crap_note(report: &fallow_output::HealthReport) -> String {
    if matches!(
        report.summary.coverage_model,
        Some(fallow_output::CoverageModel::Istanbul)
    ) {
        let match_info = match (
            report.summary.istanbul_matched,
            report.summary.istanbul_total,
        ) {
            (Some(m), Some(t)) if t > 0 => format!(" ({m}/{t} functions matched)"),
            _ => String::new(),
        };
        format!(
            "CRAP from {} coverage data{match_info}.",
            measured_coverage_label(report)
        )
    } else {
        "CRAP estimated from export references (85% direct, 40% indirect, 0% untested). Run `fallow health --coverage <coverage-final.json>` for exact scores.".to_string()
    }
}
