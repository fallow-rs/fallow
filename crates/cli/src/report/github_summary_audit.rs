//! GitHub summary for the audit command.

use super::*;

/// Port of `summary-audit.jq`.
#[must_use]
pub(super) fn render_audit_summary(env: &Value) -> String {
    let verdict = str_or(env, "verdict", "pass");
    let summary = env.get("summary").cloned().unwrap_or(Value::Null);
    let attribution = env.get("attribution").cloned().unwrap_or(Value::Null);
    let dead_code = env.get("dead_code").cloned().unwrap_or(Value::Null);
    let mut dead_rows = audit_dead_code_rows(&dead_code);
    dead_rows.sort_by(|a, b| {
        audit_priority(env, a.severity, a.status)
            .cmp(&audit_priority(env, b.severity, b.status))
            .then_with(|| a.location.cmp(&b.location))
            .then_with(|| a.kind.cmp(b.kind))
            .then_with(|| a.item.cmp(&b.item))
    });

    let styling_count = env
        .get("complexity")
        .and_then(|complexity| complexity.get("styling_findings"))
        .and_then(Value::as_array)
        .map_or(0, Vec::len)
        .to_string();
    let rows = [
        audit_category_row(
            "Dead code",
            &summary,
            "dead_code_skipped",
            &num(&summary, "dead_code_issues"),
            &attribution,
            "dead_code",
        ),
        audit_category_row(
            "Complexity",
            &summary,
            "complexity_skipped",
            &num(&summary, "complexity_findings"),
            &attribution,
            "complexity",
        ),
        audit_category_row(
            "Duplication",
            &summary,
            "duplication_skipped",
            &num(&summary, "duplication_clone_groups"),
            &attribution,
            "duplication",
        ),
        audit_category_row(
            "Styling",
            &summary,
            "complexity_skipped",
            &styling_count,
            &attribution,
            "styling",
        ),
    ]
    .join("\n");

    let mut out = format!(
        "## Fallow Audit\n\n> {} \u{b7} {} \u{b7} {}ms\n\n| Category | Findings | Introduced | Inherited |\n|:---------|---------:|-----------:|----------:|\n{rows}\n\n",
        audit_verdict_label(verdict),
        plural_n(u(env, "changed_files_count") as usize, "changed file"),
        num(env, "elapsed_ms"),
    );
    out.push_str(&audit_worsened_note(env, &attribution));
    if !dead_rows.is_empty() {
        let rows = dead_rows
            .iter()
            .take(10)
            .map(|row| {
                format!(
                    "| {} | {} | {} | {} | {} |",
                    row.kind, row.location, row.item, row.status, row.severity,
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        let tail = if dead_rows.len() > 10 {
            format!(
                "\n\n> {} more dead-code findings in the full audit report",
                dead_rows.len() - 10
            )
        } else {
            String::new()
        };
        let _ = write!(
            out,
            "### Dead Code\n\n| Type | Location | Item | Status | Rule severity |\n|:-----|:---------|:-----|:-------|:--------------|\n{rows}{tail}\n\n"
        );
    }
    out.push_str(&audit_complexity_section(env));
    out.push_str(&audit_duplication_section(env));
    out.push_str(&audit_styling_section(env));
    out.push_str(&audit_diff_risk_section(env));
    out.push_str(match s(&attribution, "gate") {
        "all" => "*Audit gate: all. Every finding in changed files affects the verdict.*",
        "new-only" if worsened_gates(env) => {
            "*Audit gate: new-only, worsened: error. Inherited findings that got worse fail the verdict like introduced findings. Other inherited findings do not fail the verdict.*"
        }
        "new-only" => {
            "*Audit gate: new-only. Inherited findings are reported but do not fail the verdict.*"
        }
        _ => "*Audit gate: unknown. New-versus-inherited attribution is unavailable.*",
    });
    out
}
