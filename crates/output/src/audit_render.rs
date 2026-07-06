use crate::CssAnalyticsSummary;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AuditDisplaySeverity {
    Off,
    Warn,
    Error,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AuditDisplayGate {
    NewOnly,
    All,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AuditStylingContextLabelInput<'a> {
    pub severity: AuditDisplaySeverity,
    pub rule: &'a str,
    pub base_state: Option<&'a str>,
    pub gate: AuditDisplayGate,
}

pub fn styling_candidate_count(summary: &CssAnalyticsSummary) -> u32 {
    [
        summary.tailwind_arbitrary_values,
        summary.duplicate_declaration_blocks,
        summary.unreferenced_css_classes,
        summary.unused_theme_tokens,
        summary.unused_font_faces,
        summary.unused_property_registrations,
        summary.unused_layers,
        summary.scoped_unused_classes,
        summary.keyframes_unreferenced,
        summary.keyframes_undefined,
        summary.unresolved_class_references,
    ]
    .into_iter()
    .fold(0u32, u32::saturating_add)
}

pub fn styling_audit_context_label(input: AuditStylingContextLabelInput<'_>) -> String {
    let severity_label = match input.severity {
        AuditDisplaySeverity::Off => "off",
        AuditDisplaySeverity::Warn => "warn",
        AuditDisplaySeverity::Error => "error",
    };
    let prefix = match (input.severity, input.gate, input.base_state) {
        (AuditDisplaySeverity::Error, AuditDisplayGate::NewOnly, Some(state))
            if state.starts_with("inherited") =>
        {
            "not gated"
        }
        (AuditDisplaySeverity::Error, _, _) => "gated",
        _ => "advisory",
    };
    match input.base_state {
        Some(state) => format!("({prefix}: {}={severity_label}, {state})", input.rule),
        None => format!("({prefix}: {}={severity_label})", input.rule),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn styling_audit_context_label_explains_gate_state() {
        assert_eq!(
            styling_audit_context_label(AuditStylingContextLabelInput {
                severity: AuditDisplaySeverity::Error,
                rule: "rules.css-selector-complexity",
                base_state: Some("introduced design-system drift since HEAD"),
                gate: AuditDisplayGate::NewOnly,
            }),
            "(gated: rules.css-selector-complexity=error, introduced design-system drift since HEAD)"
        );
        assert_eq!(
            styling_audit_context_label(AuditStylingContextLabelInput {
                severity: AuditDisplaySeverity::Error,
                rule: "rules.css-selector-complexity",
                base_state: Some("inherited styling debt from HEAD"),
                gate: AuditDisplayGate::NewOnly,
            }),
            "(not gated: rules.css-selector-complexity=error, inherited styling debt from HEAD)"
        );
        assert_eq!(
            styling_audit_context_label(AuditStylingContextLabelInput {
                severity: AuditDisplaySeverity::Warn,
                rule: "rules.css-selector-complexity",
                base_state: None,
                gate: AuditDisplayGate::All,
            }),
            "(advisory: rules.css-selector-complexity=warn)"
        );
    }

    #[test]
    fn styling_candidate_count_saturates_descriptive_css_counts() {
        let summary = CssAnalyticsSummary {
            tailwind_arbitrary_values: 2,
            duplicate_declaration_blocks: 3,
            unresolved_class_references: 5,
            ..CssAnalyticsSummary::default()
        };

        assert_eq!(styling_candidate_count(&summary), 10);
    }
}
