#![expect(
    clippy::unwrap_used,
    reason = "benches use unwrap and expect to keep fixture setup concise"
)]
#![allow(
    clippy::significant_drop_tightening,
    reason = "the external Criterion macro owns the benchmark lifecycle"
)]

use criterion::{BatchSize, Criterion, criterion_group, criterion_main};
use fallow_cli::report::{
    github_annotations::EnvelopeKind,
    github_summary::{LinkContext, render_summary},
};
use fallow_output::{
    CiIssue, CiProvider, ExplainOutput, InspectEvidence, InspectEvidenceScope,
    InspectEvidenceSection, InspectFileIdentity, InspectIdentity, InspectOutput,
    InspectTargetDescriptor, PrCommentRenderInput, RootEnvelopeMode, render_pr_comment,
    serialize_explain_json_output, serialize_inspect_json_output,
};
use serde_json::json;

const ISSUE_COUNT: usize = 250;
const SAVED_REPORT_FINDING_COUNT: usize = 1_000;

fn create_ci_issues() -> Vec<CiIssue> {
    (0..ISSUE_COUNT)
        .map(|index| CiIssue {
            rule_id: if index % 3 == 0 {
                "fallow/unused-export".to_string()
            } else {
                "fallow/code-duplication".to_string()
            },
            description: format!("Finding {index} needs attention"),
            severity: if index % 5 == 0 { "major" } else { "minor" }.to_string(),
            path: format!("src/module-{}.ts", index % 40),
            line: u64::try_from(index + 1).unwrap(),
            fingerprint: format!("fp-{index:04}"),
        })
        .collect()
}

fn category_for_rule(rule: &str) -> &'static str {
    if rule.contains("duplication") {
        "duplication"
    } else {
        "dead-code"
    }
}

fn create_explain_output() -> ExplainOutput {
    ExplainOutput {
        id: "unused-export".to_string(),
        name: "Unused export".to_string(),
        summary: "Export is not referenced by reachable code.".to_string(),
        rationale: "Unused exports increase maintenance cost.".to_string(),
        example: "export const unused = true;".to_string(),
        how_to_fix: "Remove the export or reference it from an entry point.".to_string(),
        docs: "https://docs.fallow.tools/rules/unused-export".to_string(),
    }
}

fn create_inspect_output() -> InspectOutput {
    let section = InspectEvidenceSection::ok(
        InspectEvidenceScope::File,
        json!({"imports": 8, "exports": 12, "reachable": true}),
    );
    InspectOutput {
        target: InspectTargetDescriptor::File {
            file: "src/module.ts".to_string(),
        },
        identity: InspectIdentity::File(InspectFileIdentity {
            file: "src/module.ts".to_string(),
            is_reachable: Some(json!(true)),
            is_entry_point: Some(json!(false)),
            export_count: Some(12),
            import_count: Some(8),
            imported_by_count: Some(4),
        }),
        evidence: InspectEvidence {
            trace_file: section.clone(),
            trace_export: None,
            dead_code: section.clone(),
            duplication: section.clone(),
            complexity: section.clone(),
            security: section.clone(),
            impact_closure: section,
            churn: None,
            symbol_chain: None,
            semantic_trace: None,
            api_surface: None,
            symbol_impact: None,
            targeted_tests: None,
        },
        warnings: Vec::new(),
        meta: None,
    }
}

fn create_saved_dead_code_envelope() -> serde_json::Value {
    let unused_exports = (0..SAVED_REPORT_FINDING_COUNT)
        .map(|index| {
            json!({
                "path": format!("src/module-{}.ts", index % 200),
                "line": index + 1,
                "col": index % 80,
                "export_name": format!("unusedExport{index}"),
                "is_re_export": index % 5 == 0,
                "is_type_only": index % 7 == 0,
            })
        })
        .collect::<Vec<_>>();
    json!({
        "kind": "dead-code",
        "elapsed_ms": 125,
        "total_issues": SAVED_REPORT_FINDING_COUNT,
        "unused_exports": unused_exports,
    })
}

fn component_output_pr_comment_render(c: &mut Criterion) {
    c.bench_function("component_output_pr_comment_render", |bencher| {
        bencher.iter_batched_ref(
            create_ci_issues,
            |issues| {
                render_pr_comment(&PrCommentRenderInput {
                    command: "audit",
                    provider: CiProvider::Github,
                    issues,
                    marker_id: "bench".to_string(),
                    max_comments: 50,
                    category_for_rule: &category_for_rule,
                })
            },
            BatchSize::LargeInput,
        );
    });
}

fn component_output_explain_json_serialize(c: &mut Criterion) {
    c.bench_function("component_output_explain_json_serialize", |bencher| {
        bencher.iter_batched(
            create_explain_output,
            |output| {
                serialize_explain_json_output(output, RootEnvelopeMode::Tagged, Some("bench-run"))
                    .unwrap()
            },
            BatchSize::SmallInput,
        );
    });
}

fn component_output_inspect_json_serialize(c: &mut Criterion) {
    c.bench_function("component_output_inspect_json_serialize", |bencher| {
        bencher.iter_batched(
            create_inspect_output,
            |output| {
                serialize_inspect_json_output(output, RootEnvelopeMode::Tagged, Some("bench-run"))
                    .unwrap()
            },
            BatchSize::SmallInput,
        );
    });
}

fn component_output_saved_report_github_summary(c: &mut Criterion) {
    let source = serde_json::to_string(&create_saved_dead_code_envelope()).unwrap();
    let links = LinkContext::default();
    c.bench_function("component_output_saved_report_github_summary", |bencher| {
        bencher.iter(|| {
            let envelope = serde_json::from_str(&source).unwrap();
            render_summary(EnvelopeKind::DeadCode, &envelope, &links)
        });
    });
}

fn component_output_dead_code_github_summary_render(c: &mut Criterion) {
    let envelope = create_saved_dead_code_envelope();
    let links = LinkContext::default();
    c.bench_function(
        "component_output_dead_code_github_summary_render",
        |bencher| {
            bencher.iter(|| render_summary(EnvelopeKind::DeadCode, &envelope, &links));
        },
    );
}

criterion_group!(
    benches,
    component_output_pr_comment_render,
    component_output_explain_json_serialize,
    component_output_inspect_json_serialize,
    component_output_saved_report_github_summary,
    component_output_dead_code_github_summary_render
);
criterion_main!(benches);
