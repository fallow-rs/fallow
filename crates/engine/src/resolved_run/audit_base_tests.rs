use fallow_config::{
    CommandKind, EnvSnapshot, PlannedAnalysis, RunInputs, SelectionInput, Surface,
};
use fallow_types::sourced::{Given, Origin, Source};

use super::*;

fn inputs(surface: Surface, command: CommandKind, env: &[(&str, &str)]) -> RunInputs {
    RunInputs::new(
        surface,
        EnvSnapshot::from_pairs(env, None),
        command,
        SelectionInput::Single(PlannedAnalysis::DeadCode),
    )
}

fn value(sourced: &Sourced<String>) -> (&str, &Source) {
    (sourced.value().as_str(), sourced.source())
}

const BOTH: [(&str, &str); 2] = [
    ("FALLOW_AUDIT_BASE", " main "),
    ("FALLOW_CHANGED_SINCE", "HEAD~1"),
];

#[test]
fn a_given_base_wins_over_both_variables() {
    let mut run = inputs(Surface::Mcp, CommandKind::Audit, &BOTH);
    run.audit_base = Some(Given {
        value: "release".to_owned(),
        origin: Origin::ToolParam("base"),
    });
    let decision = decide_audit_base(&run);
    assert_eq!(
        decision.named.as_ref().map(value),
        Some((
            "release",
            &Source::Given {
                origin: Origin::ToolParam("base")
            }
        ))
    );
}

/// `FALLOW_CHANGED_SINCE` is not a named base: the audit base variable and
/// the detection come first.
#[test]
fn the_audit_base_variable_wins_over_the_changed_since_variable() {
    let decision = decide_audit_base(&inputs(Surface::Mcp, CommandKind::Audit, &BOTH));
    assert_eq!(
        decision.named.as_ref().map(value),
        Some((
            "main",
            &Source::Environment {
                name: "FALLOW_AUDIT_BASE"
            }
        ))
    );
    assert_eq!(
        decision.fallback.as_ref().map(value),
        Some((
            "HEAD~1",
            &Source::Environment {
                name: "FALLOW_CHANGED_SINCE"
            }
        ))
    );
}

#[test]
fn only_the_mcp_server_reads_the_changed_since_variable() {
    for surface in [Surface::Cli, Surface::Api, Surface::Node] {
        let decision = decide_audit_base(&inputs(surface, CommandKind::Audit, &BOTH));
        assert_eq!(decision.fallback, None, "{surface:?}");
        assert!(decision.named.is_some(), "{surface:?}");
    }
}

#[test]
fn another_command_has_no_base() {
    let decision = decide_audit_base(&inputs(Surface::Mcp, CommandKind::DeadCode, &BOTH));
    assert_eq!(decision, AuditBaseDecision::default());
}

#[test]
fn an_empty_variable_is_unset() {
    let decision = decide_audit_base(&inputs(
        Surface::Mcp,
        CommandKind::Audit,
        &[("FALLOW_AUDIT_BASE", "  "), ("FALLOW_CHANGED_SINCE", "")],
    ));
    assert_eq!(decision, AuditBaseDecision::default());
}
