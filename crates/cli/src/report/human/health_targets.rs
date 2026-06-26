use std::path::Path;

use colored::Colorize;

use super::{MAX_FLAT_ITEMS, relative_path, split_dir_filename};

const DOCS_HEALTH: &str = "https://docs.fallow.tools/explanations/health";

fn render_direct_import_symbol(symbol: &fallow_output::DirectCallerSymbolEvidence) -> String {
    let imported = if symbol.imported == "side-effect" {
        "side effect"
    } else {
        symbol.imported.as_str()
    };

    if symbol.local.is_empty() || symbol.imported == symbol.local {
        imported.to_string()
    } else {
        format!("{imported} as {}", symbol.local)
    }
}

pub(super) fn render_refactoring_targets(
    lines: &mut Vec<String>,
    report: &fallow_output::HealthReport,
    root: &Path,
) {
    if report.targets.is_empty() {
        return;
    }

    push_refactoring_targets_header(lines, report);

    let shown_targets = report.targets.len().min(MAX_FLAT_ITEMS);
    for target in &report.targets[..shown_targets] {
        push_refactoring_target_row(lines, target, root);
        render_target_evidence(lines, target, root);
        lines.push(String::new());
    }
    push_refactoring_targets_overflow(lines, report.targets.len());
    lines.push(format!(
        "  {}",
        format!(
            "Prioritized refactoring recommendations based on complexity, churn, and coupling signals: {DOCS_HEALTH}#refactoring-targets"
        )
        .dimmed()
    ));
    lines.push(String::new());
}

fn push_refactoring_targets_header(lines: &mut Vec<String>, report: &fallow_output::HealthReport) {
    lines.push(format!(
        "{} {}",
        "\u{25cf}".cyan(),
        format!("Refactoring targets ({})", report.targets.len())
            .cyan()
            .bold()
    ));
    lines.push(format!(
        "  {}",
        refactoring_effort_summary(&report.targets).dimmed()
    ));
    lines.push(format!(
        "  {}",
        "  score = quick-win ROI (higher = better) \u{00b7} pri = absolute priority".dimmed()
    ));
    lines.push(String::new());
}

fn refactoring_effort_summary(targets: &[fallow_output::RefactoringTargetFinding]) -> String {
    let low = targets
        .iter()
        .filter(|t| matches!(t.effort, fallow_output::EffortEstimate::Low))
        .count();
    let medium = targets
        .iter()
        .filter(|t| matches!(t.effort, fallow_output::EffortEstimate::Medium))
        .count();
    let high = targets
        .iter()
        .filter(|t| matches!(t.effort, fallow_output::EffortEstimate::High))
        .count();
    let mut effort_parts = Vec::new();
    if low > 0 {
        effort_parts.push(format!("{low} low effort"));
    }
    if medium > 0 {
        effort_parts.push(format!("{medium} medium"));
    }
    if high > 0 {
        effort_parts.push(format!("{high} high"));
    }
    effort_parts.join(" \u{00b7} ")
}

fn push_refactoring_target_row(
    lines: &mut Vec<String>,
    target: &fallow_output::RefactoringTarget,
    root: &Path,
) {
    let file_str = relative_path(&target.path, root).display().to_string();
    let (dir, filename) = split_dir_filename(&file_str);
    lines.push(format!(
        "  {}  {}    {}{}",
        target_efficiency_colored(target.efficiency),
        format!("pri:{:.1}", target.priority).dimmed(),
        dir.dimmed(),
        filename,
    ));
    lines.push(format!(
        "         {} \u{00b7} effort:{} \u{00b7} confidence:{}  {}{}",
        target.category.label().yellow(),
        target_effort_colored(&target.effort),
        target_confidence_colored(&target.confidence),
        target.recommendation.dimmed(),
        generated_recommendation_tag(&target.recommendation),
    ));
}

fn target_efficiency_colored(efficiency: f64) -> String {
    let eff_str = format!("{efficiency:>5.1}");
    if efficiency >= 40.0 {
        eff_str.green().to_string()
    } else if efficiency >= 20.0 {
        eff_str.yellow().to_string()
    } else {
        eff_str.dimmed().to_string()
    }
}

fn target_effort_colored(effort: &fallow_output::EffortEstimate) -> String {
    let label = effort.label();
    match effort {
        fallow_output::EffortEstimate::Low => label.green().to_string(),
        fallow_output::EffortEstimate::Medium => label.yellow().to_string(),
        fallow_output::EffortEstimate::High => label.red().to_string(),
    }
}

fn target_confidence_colored(confidence: &fallow_output::Confidence) -> String {
    let label = confidence.label();
    match confidence {
        fallow_output::Confidence::High => label.green().to_string(),
        fallow_output::Confidence::Medium => label.yellow().to_string(),
        fallow_output::Confidence::Low => label.dimmed().to_string(),
    }
}

fn generated_recommendation_tag(recommendation: &str) -> String {
    if recommendation_mentions_generated(recommendation) {
        format!(" {}", "(generated)".dimmed())
    } else {
        String::new()
    }
}

fn push_refactoring_targets_overflow(lines: &mut Vec<String>, target_count: usize) {
    if target_count <= MAX_FLAT_ITEMS {
        return;
    }
    lines.push(format!(
        "  {}",
        format!(
            "... and {} more targets (--format json for full list)",
            target_count - MAX_FLAT_ITEMS
        )
        .dimmed()
    ));
    lines.push(String::new());
}

fn render_target_evidence(
    lines: &mut Vec<String>,
    target: &fallow_output::RefactoringTarget,
    root: &Path,
) {
    let Some(evidence) = &target.evidence else {
        return;
    };

    if !evidence.direct_callers.is_empty() {
        let callers = evidence
            .direct_callers
            .iter()
            .map(|caller| {
                let path = relative_path(&caller.path, root).display().to_string();
                if caller.symbols.is_empty() {
                    path
                } else {
                    let symbols = caller
                        .symbols
                        .iter()
                        .map(render_direct_import_symbol)
                        .collect::<Vec<_>>()
                        .join(", ");
                    format!("{path} ({symbols})")
                }
            })
            .collect::<Vec<_>>()
            .join("; ");
        lines.push(format!(
            "         {}",
            format!("importers: {callers}").dimmed()
        ));
    }

    if !evidence.clone_siblings.is_empty() {
        let siblings = evidence
            .clone_siblings
            .iter()
            .map(|sibling| {
                let path = relative_path(&sibling.path, root).display().to_string();
                format!(
                    "{}:{}-{} {}",
                    path, sibling.start_line, sibling.end_line, sibling.fingerprint
                )
            })
            .collect::<Vec<_>>()
            .join("; ");
        lines.push(format!(
            "         {}",
            format!("clones: {siblings}").dimmed()
        ));
    }
}

fn recommendation_mentions_generated(recommendation: &str) -> bool {
    let mut rest = recommendation;
    while let Some(pos) = rest.find("validate") {
        let after_validate = &rest[pos + 8..];
        if !after_validate.is_empty() {
            let digits: String = after_validate
                .chars()
                .take_while(|c| c.is_ascii_digit())
                .collect();
            if !digits.is_empty() {
                let next = after_validate.chars().nth(digits.len());
                if !next.is_some_and(|c| c.is_alphanumeric() || c == '_') {
                    return true;
                }
            }
        }
        rest = &rest[pos + 8..];
    }
    false
}
