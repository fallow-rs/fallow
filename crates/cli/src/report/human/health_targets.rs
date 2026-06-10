use std::path::Path;

use colored::Colorize;

use super::{MAX_FLAT_ITEMS, relative_path, split_dir_filename};

const DOCS_HEALTH: &str = "https://docs.fallow.tools/explanations/health";

fn render_direct_import_symbol(symbol: &crate::health_types::DirectCallerSymbolEvidence) -> String {
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
    report: &crate::health_types::HealthReport,
    root: &Path,
) {
    if report.targets.is_empty() {
        return;
    }

    lines.push(format!(
        "{} {}",
        "\u{25cf}".cyan(),
        format!("Refactoring targets ({})", report.targets.len())
            .cyan()
            .bold()
    ));

    let low = report
        .targets
        .iter()
        .filter(|t| matches!(t.effort, crate::health_types::EffortEstimate::Low))
        .count();
    let medium = report
        .targets
        .iter()
        .filter(|t| matches!(t.effort, crate::health_types::EffortEstimate::Medium))
        .count();
    let high = report
        .targets
        .iter()
        .filter(|t| matches!(t.effort, crate::health_types::EffortEstimate::High))
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
    lines.push(format!("  {}", effort_parts.join(" \u{00b7} ").dimmed()));
    lines.push(format!(
        "  {}",
        "  score = quick-win ROI (higher = better) \u{00b7} pri = absolute priority".dimmed()
    ));
    lines.push(String::new());

    let shown_targets = report.targets.len().min(MAX_FLAT_ITEMS);
    for target in &report.targets[..shown_targets] {
        let file_str = relative_path(&target.path, root).display().to_string();

        let eff_str = format!("{:>5.1}", target.efficiency);
        let eff_colored = if target.efficiency >= 40.0 {
            eff_str.green().to_string()
        } else if target.efficiency >= 20.0 {
            eff_str.yellow().to_string()
        } else {
            eff_str.dimmed().to_string()
        };

        let (dir, filename) = split_dir_filename(&file_str);

        lines.push(format!(
            "  {}  {}    {}{}",
            eff_colored,
            format!("pri:{:.1}", target.priority).dimmed(),
            dir.dimmed(),
            filename,
        ));

        let label = target.category.label();
        let effort = target.effort.label();
        let effort_colored = match target.effort {
            crate::health_types::EffortEstimate::Low => effort.green().to_string(),
            crate::health_types::EffortEstimate::Medium => effort.yellow().to_string(),
            crate::health_types::EffortEstimate::High => effort.red().to_string(),
        };
        let confidence = target.confidence.label();
        let confidence_colored = match target.confidence {
            crate::health_types::Confidence::High => confidence.green().to_string(),
            crate::health_types::Confidence::Medium => confidence.yellow().to_string(),
            crate::health_types::Confidence::Low => confidence.dimmed().to_string(),
        };
        let generated_tag = if recommendation_mentions_generated(&target.recommendation) {
            format!(" {}", "(generated)".dimmed())
        } else {
            String::new()
        };
        lines.push(format!(
            "         {} \u{00b7} effort:{} \u{00b7} confidence:{}  {}{}",
            label.yellow(),
            effort_colored,
            confidence_colored,
            target.recommendation.dimmed(),
            generated_tag,
        ));

        render_target_evidence(lines, target, root);

        lines.push(String::new());
    }
    if report.targets.len() > MAX_FLAT_ITEMS {
        lines.push(format!(
            "  {}",
            format!(
                "... and {} more targets (--format json for full list)",
                report.targets.len() - MAX_FLAT_ITEMS
            )
            .dimmed()
        ));
        lines.push(String::new());
    }
    lines.push(format!(
        "  {}",
        format!(
            "Prioritized refactoring recommendations based on complexity, churn, and coupling signals: {DOCS_HEALTH}#refactoring-targets"
        )
        .dimmed()
    ));
    lines.push(String::new());
}

fn render_target_evidence(
    lines: &mut Vec<String>,
    target: &crate::health_types::RefactoringTarget,
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
