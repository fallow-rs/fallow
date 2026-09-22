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
            .contains("Their presence alone does not establish protection")
    );
}

#[test]
fn origin_observations_preserve_candidates_taint_and_severity() {
    let fixture = fixture_path("security-origin-observations");
    let project = tempfile::tempdir().expect("temporary project");
    std::fs::create_dir(project.path().join("src")).expect("source directory");
    std::fs::copy(
        fixture.join("package.json"),
        project.path().join("package.json"),
    )
    .expect("fixture manifest");
    let source = std::fs::read_to_string(fixture.join("src/index.ts")).expect("fixture source");
    let without_guards = source
        .lines()
        .map(|line| {
            if line.ends_with("// observed guard") {
                ""
            } else {
                line
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    let target = project.path().join("src/index.ts");
    std::fs::write(&target, without_guards).expect("baseline source");
    let config = create_config_with_rules(project.path().to_path_buf(), |rules| {
        rules.security_sink = Severity::Warn;
    });
    let before = fallow_core::analyze(&config).expect("baseline analysis");
    std::fs::write(&target, &source).expect("guarded source");
    let after = fallow_core::analyze(&config).expect("guarded analysis");
    assert!(!before.security_findings.is_empty());
    assert_eq!(
        before.security_findings.len(),
        after.security_findings.len()
    );
    for (before, after) in before
        .security_findings
        .iter()
        .zip(&after.security_findings)
    {
        let mut original = serde_json::to_value(before).expect("baseline finding");
        let mut observed = serde_json::to_value(after).expect("observed finding");
        original
            .as_object_mut()
            .expect("object")
            .remove("attack_surface");
        observed
            .as_object_mut()
            .expect("object")
            .remove("attack_surface");
        assert_eq!(original, observed, "only verification context may change");
        let surface = after
            .attack_surface
            .as_ref()
            .expect("surface for untrusted source");
        let controls = &surface.defensive_boundary.controls;
        assert!(
            !controls.is_empty(),
            "file-level hints also reach unrelated sinks"
        );
        for control in controls {
            assert_eq!(control.callee, "origin-equality-guard");
            assert_eq!(
                control.kind,
                fallow_types::extract::SecurityControlKind::Validation
            );
            assert!(
                source
                    .lines()
                    .nth(control.line as usize - 1)
                    .expect("control source line")
                    .ends_with("// observed guard")
            );
        }
        assert!(
            surface
                .defensive_boundary
                .verification_prompt
                .contains("files on this import trace")
        );
        assert!(
            surface
                .defensive_boundary
                .verification_prompt
                .contains("does not establish protection")
        );
    }
}
