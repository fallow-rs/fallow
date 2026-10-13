//! Runs `run_mutation` on a real project for manual validation.
//!
//! The test is ignored by default because it runs the tests of an external
//! project. Set the environment and run it explicitly:
//!
//! ```text
//! FALLOW_MUTATION_CORPUS_ROOT=/abs/project \
//! FALLOW_MUTATION_CORPUS_REPORT=/abs/report.json \
//! FALLOW_MUTATION_CORPUS_MAX=150 \
//! cargo test -p fallow-api --test mutation_corpus -- --ignored --nocapture
//! ```
#![expect(
    clippy::expect_used,
    clippy::print_stderr,
    reason = "a manual validation harness that reports to the terminal"
)]

use std::path::PathBuf;

use fallow_api::{AnalysisOptions, MutationRunOptions, MutationRunScope, run_mutation};

#[test]
#[ignore = "runs the tests of an external project; set FALLOW_MUTATION_CORPUS_ROOT"]
fn run_mutation_on_a_corpus_project() {
    let Some(root) = std::env::var_os("FALLOW_MUTATION_CORPUS_ROOT").map(PathBuf::from) else {
        eprintln!("skipped: FALLOW_MUTATION_CORPUS_ROOT is not set");
        return;
    };
    let max_mutants = std::env::var("FALLOW_MUTATION_CORPUS_MAX")
        .ok()
        .and_then(|value| value.parse().ok());
    let report = run_mutation(&MutationRunOptions {
        analysis: AnalysisOptions {
            root: Some(root),
            ..AnalysisOptions::default()
        },
        scope: MutationRunScope::All,
        max_mutants,
        ..MutationRunOptions::default()
    })
    .expect("the mutation run completes");
    let json = serde_json::to_string_pretty(&report).expect("the report serializes");
    if let Some(path) = std::env::var_os("FALLOW_MUTATION_CORPUS_REPORT") {
        std::fs::write(path, &json).expect("the report is written");
    }
    eprintln!(
        "completion={:?} counts={:?} score={:?} timings={:?} elapsed={:?}",
        report.completion,
        report.summary.counts,
        report.summary.score,
        report.timings,
        report.elapsed_ms
    );
}

/// Writes the engine plan of the same request, with the replacement text of
/// each mutant, for the differential oracle (decision 18). The public report
/// carries no source text, so the oracle reads the plan instead.
#[test]
#[ignore = "reads an external project; set FALLOW_MUTATION_CORPUS_ROOT"]
fn dump_the_corpus_plan_for_the_oracle() {
    use fallow_engine::mutation::{MutationPlanRequest, MutationScope, plan_mutations_for_session};
    let (Some(root), Some(out)) = (
        std::env::var_os("FALLOW_MUTATION_CORPUS_ROOT").map(PathBuf::from),
        std::env::var_os("FALLOW_MUTATION_CORPUS_PLAN"),
    ) else {
        eprintln!("skipped: set FALLOW_MUTATION_CORPUS_ROOT and FALLOW_MUTATION_CORPUS_PLAN");
        return;
    };
    let project = fallow_engine::project_config::config_for_project_with_load_options(
        &root,
        None,
        fallow_config::ConfigLoadOptions::default(),
    )
    .expect("the config loads");
    let session = fallow_engine::session::AnalysisSession::from_config(project);
    let mut request = MutationPlanRequest::new(MutationScope::All);
    request.max_mutants = std::env::var("FALLOW_MUTATION_CORPUS_MAX")
        .ok()
        .and_then(|value| value.parse().ok());
    let plan = plan_mutations_for_session(&session, &request).expect("the plan builds");
    let mutants: Vec<serde_json::Value> = plan
        .mutants
        .iter()
        .map(|planned| {
            serde_json::json!({
                "mutant_id": planned.mutant.id.as_str(),
                "file": planned.file,
                "span": planned.mutant.span,
                "original": planned.mutant.original,
                "replacement": planned.mutant.replacement,
            })
        })
        .collect();
    std::fs::write(
        out,
        serde_json::to_string_pretty(&mutants).expect("the plan serializes"),
    )
    .expect("the plan is written");
}
