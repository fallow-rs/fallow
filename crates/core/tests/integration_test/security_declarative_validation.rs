//! Integration tests for declarative validation controls (#1094).

use fallow_config::Severity;
use fallow_core::results::{AnalysisResults, SecurityFindingKind};

use super::common::{create_config_with_rules, fixture_path};

fn analyze_fixture(name: &str) -> AnalysisResults {
    let root = fixture_path(name);
    let config = create_config_with_rules(root, |rules| {
        rules.security_sink = Severity::Warn;
    });
    fallow_core::analyze(&config).expect("analysis should succeed")
}

#[test]
fn trpc_input_validation_surfaces_as_defensive_boundary_control() {
    let results = analyze_fixture("security-declarative-validation-1094-trpc");
    let finding = results
        .security_findings
        .iter()
        .find(|finding| matches!(finding.kind, SecurityFindingKind::TaintedSink))
        .expect("tainted sink finding");
    let surface = finding
        .attack_surface
        .as_ref()
        .expect("attack surface entry");

    assert!(surface.defensive_boundary.controls.iter().any(|control| {
        control.callee == "trpc.procedure.input"
            && control.kind == fallow_types::extract::SecurityControlKind::Validation
    }));
    assert!(
        surface
            .defensive_boundary
            .verification_prompt
            .contains("Are they sufficient")
    );
}
