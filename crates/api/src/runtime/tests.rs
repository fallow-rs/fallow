use std::path::{Path, PathBuf};

use fallow_output::{CHECK_SCHEMA_VERSION, DiffIndex, HealthReport};
use fallow_types::duplicates::{CloneInstance, DuplicationReport, DuplicationStats};
use fallow_types::output::NextStep;

use super::*;
use crate::runtime_json::{
    serialize_boundary_violations_programmatic_json,
    serialize_circular_dependencies_programmatic_json, serialize_combined_programmatic_json,
    serialize_dead_code_programmatic_json, serialize_duplication_programmatic_json,
    serialize_feature_flags_programmatic_json, serialize_health_programmatic_json,
};
use crate::runtime_output::HEALTH_SCHEMA_VERSION;
use crate::{
    AnalysisOptions, CombinedOptions, DeadCodeFilters, DeadCodeOptions, DuplicationOptions,
    FeatureFlagsOptions,
    analysis_context::resolve_workspace_filters,
    duplication_filters::{filter_by_diff, filter_by_workspaces},
};

struct FakeHealthRunner {
    root: PathBuf,
    telemetry_analysis_run_id: Option<String>,
}

impl ProgrammaticHealthRunner for FakeHealthRunner {
    fn run_programmatic_health(
        &self,
        _options: &ComplexityOptions,
    ) -> Result<ProgrammaticHealthRun, ProgrammaticError> {
        Ok(ProgrammaticHealthRun {
            analysis: ProgrammaticHealthAnalysis {
                report: HealthReport::default(),
                grouping: None,
                root: self.root.clone(),
                elapsed: std::time::Duration::ZERO,
            },
            workspace_diagnostics: vec![WorkspaceDiagnostic::new(
                &self.root,
                self.root.join("package.json"),
                fallow_types::workspace::WorkspaceDiagnosticKind::UndeclaredWorkspace,
            )],
            next_step_facts: ProgrammaticHealthNextStepFacts {
                suggestions_enabled: true,
                offer_setup: false,
                impact_digest: Some(fallow_output::ImpactDigestCounts {
                    containment_count: 1,
                    resolved_total: 0,
                }),
                audit_changed: false,
            },
            telemetry_analysis_run_id: self.telemetry_analysis_run_id.clone(),
        })
    }
}

fn analysis_at(root: &Path) -> AnalysisOptions {
    AnalysisOptions {
        root: Some(root.to_path_buf()),
        ..AnalysisOptions::default()
    }
}

fn canonical_root(root: &Path) -> PathBuf {
    dunce::canonicalize(root).expect("canonical root")
}

fn health_json_with_runner(
    options: &ComplexityOptions,
    runner: &impl ProgrammaticHealthRunner,
) -> Result<serde_json::Value, ProgrammaticError> {
    serialize_health_programmatic_json(run_health_with_runner(options, runner)?)
}

fn duplication_json(options: &DuplicationOptions) -> Result<serde_json::Value, ProgrammaticError> {
    serialize_duplication_programmatic_json(run_duplication(options)?)
}

fn feature_flags_json(
    options: &FeatureFlagsOptions,
) -> Result<serde_json::Value, ProgrammaticError> {
    serialize_feature_flags_programmatic_json(run_feature_flags(options)?)
}

fn dead_code_json(options: &DeadCodeOptions) -> Result<serde_json::Value, ProgrammaticError> {
    serialize_dead_code_programmatic_json(run_dead_code(options)?)
}

fn circular_dependencies_json(
    options: &DeadCodeOptions,
) -> Result<serde_json::Value, ProgrammaticError> {
    serialize_circular_dependencies_programmatic_json(run_circular_dependencies(options)?)
}

fn boundary_violations_json(
    options: &DeadCodeOptions,
) -> Result<serde_json::Value, ProgrammaticError> {
    serialize_boundary_violations_programmatic_json(run_boundary_violations(options)?)
}

#[test]
fn runtime_entrypoint_modules_return_typed_outputs_before_json() {
    let json_result = concat!("Result<serde_json::", "Value");
    let programmatic_json_result = concat!("ProgrammaticResult<serde_json::", "Value");
    let detect_inner = concat!("detect_", "_inner");
    let typed_output_source = include_str!("../runtime_output.rs");

    assert!(
        !typed_output_source.contains("fn into_json"),
        "typed programmatic output structs must not own JSON serialization methods"
    );

    for (module, source) in [
        ("combined", include_str!("combined.rs")),
        ("dead_code", include_str!("dead_code.rs")),
        ("duplication", include_str!("duplication.rs")),
        ("feature_flags", include_str!("feature_flags.rs")),
        ("trace", include_str!("trace.rs")),
    ] {
        assert!(
            !source.contains(json_result),
            "runtime::{module} must return typed output and leave JSON to runtime_json serializers"
        );
        assert!(
            !source.contains(programmatic_json_result),
            "runtime::{module} must not expose JSON as its programmatic result"
        );
        assert!(
            !source.contains(detect_inner),
            "runtime::{module} must use typed run_* naming for inner execution"
        );
    }
}

#[test]
fn run_combined_returns_typed_sections_before_json() {
    let project = tempfile::tempdir().expect("project");
    let root = project.path();
    std::fs::write(
        root.join("package.json"),
        r#"{"name":"combined-api","type":"module","main":"src/index.ts"}"#,
    )
    .expect("write package");
    std::fs::create_dir_all(root.join("src")).expect("create src");
    std::fs::write(
        root.join("src/index.ts"),
        "export const used = 1;\nconsole.log(used);\n",
    )
    .expect("write entry");
    std::fs::write(root.join("src/unused.ts"), "export const unused = 1;\n").expect("write unused");

    let output = run_combined(&CombinedOptions {
        analysis: analysis_at(root),
        health_options: ComplexityOptions {
            complexity: true,
            file_scores: true,
            score: true,
            ..ComplexityOptions::default()
        },
        ..CombinedOptions::default()
    })
    .expect("combined output");

    assert!(output.dead_code.is_some());
    assert!(output.duplication.is_some());
    assert!(output.health.is_some());

    let json = serialize_combined_programmatic_json(output).expect("combined json");
    assert_eq!(json["kind"], "combined");
    assert!(json.get("check").is_some());
    assert!(json.get("dupes").is_some());
    assert!(json.get("health").is_some());
}

#[test]
fn derives_programmatic_health_execution_options_from_api_contracts() {
    let project = tempfile::tempdir().expect("temp dir");
    let root = project.path();
    let options = ComplexityOptions {
        analysis: AnalysisOptions {
            root: Some(root.to_path_buf()),
            no_cache: true,
            threads: Some(2),
            production_override: Some(true),
            explain: true,
            ..AnalysisOptions::default()
        },
        max_cyclomatic: Some(12),
        top: Some(5),
        complexity_breakdown: true,
        complexity: true,
        ownership: true,
        score: true,
        min_commits: Some(3),
        ..ComplexityOptions::default()
    };
    let resolved = resolve_programmatic_analysis_context(&options.analysis)
        .expect("programmatic context resolves");

    let execution = derive_programmatic_health_execution_options(&resolved, &options);

    assert_eq!(execution.root, canonical_root(root));
    assert!(matches!(execution.output, OutputFormat::Human));
    assert!(execution.no_cache);
    assert_eq!(execution.threads, 2);
    assert!(execution.quiet);
    assert!(execution.complexity_breakdown);
    assert_eq!(execution.thresholds.max_cyclomatic, Some(12));
    assert_eq!(execution.top, Some(5));
    assert!(execution.production);
    assert_eq!(execution.production_override, Some(true));
    assert!(execution.complexity);
    assert!(execution.hotspots);
    assert!(execution.ownership);
    assert!(execution.score);
    assert_eq!(execution.min_commits, Some(3));
    assert!(execution.explain);
    assert!(execution.enforce_coverage_gap_gate);
    assert!(!execution.performance);
    assert!(execution.runtime_coverage.is_none());
    assert!(execution.group_by.is_none());
}

#[test]
fn run_health_with_session_reuses_existing_discovery() {
    let project = tempfile::tempdir().expect("temp dir");
    let src = project.path().join("src");
    std::fs::create_dir(&src).expect("src dir");
    std::fs::write(
        src.join("index.ts"),
        "export function existing(value: boolean) { if (value) { return 1; } return 0; }\n",
    )
    .expect("source file");

    let options = ComplexityOptions {
        analysis: analysis_at(project.path()),
        max_cyclomatic: Some(1),
        complexity: true,
        ..ComplexityOptions::default()
    };
    let resolved = resolve_programmatic_analysis_context(&options.analysis)
        .expect("programmatic context resolves");
    let session =
        fallow_engine::session::AnalysisSession::load(project.path(), None).expect("session loads");

    std::fs::write(
        src.join("late.ts"),
        "export function late(value: boolean) { if (value) { return 1; } return 0; }\n",
    )
    .expect("late source file");

    let output = resolved
        .install(|| run_health_with_session(&options, &resolved, &session, None))
        .expect("health succeeds");

    assert!(
        output
            .report
            .findings
            .iter()
            .any(|finding| finding.path.ends_with("src/index.ts"))
    );
    assert!(
        output
            .report
            .findings
            .iter()
            .all(|finding| !finding.path.ends_with("src/late.ts")),
        "session-backed health must not rediscover files added after session load"
    );
}

#[test]
fn audit_reuses_dead_code_artifacts_when_only_health_scope_matches() {
    let source = include_str!("audit.rs");
    assert!(
        source.contains("fn run_dead_code_and_health_with_session("),
        "programmatic audit must keep dead-code plus health artifact reuse in one helper"
    );
    assert!(
        source.contains("production_modes.dead_code == production_modes.health"),
        "programmatic audit must reuse dead-code artifacts when effective health scope matches dead-code scope"
    );
    assert!(
        !source.contains("if options.production_dead_code == options.production_health"),
        "programmatic audit must not compare raw production overrides before config resolution"
    );
    assert!(
        source.contains("duplication: run_duplication(&duplication_options)?"),
        "the mixed-scope audit branch must only isolate duplication instead of isolating health too"
    );
}

#[test]
fn effective_production_modes_resolve_per_analysis_config() {
    let project = tempfile::tempdir().expect("temp dir");
    std::fs::write(
        project.path().join("package.json"),
        r#"{"name":"production-config","type":"module"}"#,
    )
    .expect("package json");
    std::fs::write(
        project.path().join(".fallowrc.json"),
        r#"{"production":{"deadCode":false,"health":true,"dupes":false}}"#,
    )
    .expect("config");

    let analysis = analysis_at(project.path());
    let resolved = resolve_programmatic_analysis_context(&analysis).expect("context resolves");
    let modes =
        resolve_effective_production_modes(&resolved, None, None, None).expect("modes resolve");

    assert!(!modes.dead_code);
    assert!(modes.health);
    assert!(!modes.dupes);
}

#[test]
fn combined_reuse_uses_effective_production_modes() {
    let source = include_str!("combined.rs");
    assert!(
        source.contains("resolve_effective_production_modes(&resolved, None, None, None)"),
        "programmatic combined must resolve config-derived production modes before sharing sessions"
    );
    assert!(
        source.contains("production_modes.dead_code == production_modes.health"),
        "programmatic combined must compare effective production modes before sharing health artifacts"
    );
}

#[test]
fn run_health_with_session_reuses_styling_reference_surface() {
    let project = tempfile::tempdir().expect("temp dir");
    std::fs::write(
        project.path().join("package.json"),
        r#"{"name":"slidev-css"}"#,
    )
    .expect("package file");
    let stylesheet = project.path().join("style.css");
    std::fs::write(
        &stylesheet,
        ".cover-sub { color: red; }\n.really-dead-class{}\n",
    )
    .expect("style file");
    let slides = project.path().join("slides.md");
    std::fs::write(&slides, r#"<p class="cover-sub">Intro</p>"#).expect("slides file");

    let options = ComplexityOptions {
        analysis: analysis_at(project.path()),
        css: true,
        css_deep: true,
        ..ComplexityOptions::default()
    };
    let resolved = resolve_programmatic_analysis_context(&options.analysis)
        .expect("programmatic context resolves");
    let session =
        fallow_engine::session::AnalysisSession::load(project.path(), None).expect("session loads");

    let first = resolved
        .install(|| run_health_with_session(&options, &resolved, &session, None))
        .expect("first health succeeds");
    assert_eq!(
        first
            .report
            .css_analytics
            .as_ref()
            .expect("css analytics")
            .unreferenced_css_classes
            .iter()
            .map(|item| item.class.as_str())
            .collect::<Vec<_>>(),
        vec!["really-dead-class"]
    );

    std::fs::remove_file(slides).expect("remove slides after artifact cache");
    std::fs::write(&stylesheet, ".cover-sub{}\n").expect("change style after artifact cache");

    let second = resolved
        .install(|| run_health_with_session(&options, &resolved, &session, None))
        .expect("second health succeeds");
    assert_eq!(
        second
            .report
            .css_analytics
            .as_ref()
            .expect("css analytics")
            .unreferenced_css_classes
            .iter()
            .map(|item| item.class.as_str())
            .collect::<Vec<_>>(),
        vec!["really-dead-class"],
        "session-backed health should reuse the styling reference surface and CSS class inventory already built for the session"
    );
    assert!(
        second
            .report
            .css_analytics
            .as_ref()
            .expect("css analytics")
            .raw_style_values
            .iter()
            .any(|item| item.value == "red"),
        "session-backed health should reuse the parsed CSS walk already built for the session"
    );
}

#[test]
fn run_health_with_session_artifacts_accepts_retained_dead_code_analysis() {
    let project = tempfile::tempdir().expect("temp dir");
    let src = project.path().join("src");
    std::fs::create_dir(&src).expect("src dir");
    std::fs::write(
        src.join("index.ts"),
        "export function score(value: boolean) { if (value) { return 1; } return 0; }\n",
    )
    .expect("source file");

    let options = ComplexityOptions {
        analysis: analysis_at(project.path()),
        file_scores: true,
        ..ComplexityOptions::default()
    };
    let resolved = resolve_programmatic_analysis_context(&options.analysis)
        .expect("programmatic context resolves");
    let session =
        fallow_engine::session::AnalysisSession::load(project.path(), None).expect("session loads");
    let artifacts = session
        .analyze_dead_code_with_artifacts(true, true)
        .expect("dead-code artifacts");
    assert!(artifacts.graph.is_some());

    let output = resolved
        .install(|| {
            run_health_with_session_artifacts(
                &options,
                &resolved,
                &session,
                None,
                Some(artifacts),
                None,
            )
        })
        .expect("health succeeds");

    assert!(!output.report.file_scores.is_empty());
}

#[test]
fn serialize_health_report_json_tags_meta_and_strips_paths() {
    let root = Path::new("/repo");
    let json = serialize_health_report_json(HealthJsonReportInput {
        report: HealthReport::default(),
        root,
        elapsed: std::time::Duration::ZERO,
        explain: true,
        grouped_by: None,
        groups: None,
        workspace_diagnostics: vec![WorkspaceDiagnostic::new(
            Path::new("/repo"),
            PathBuf::from("/repo/package.json"),
            fallow_types::workspace::WorkspaceDiagnosticKind::UndeclaredWorkspace,
        )],
        next_steps: vec![NextStep {
            id: "inspect-health".to_string(),
            command: "fallow health --format json".to_string(),
            reason: "inspect health details".to_string(),
        }],
        envelope_mode: RootEnvelopeMode::Tagged,
        telemetry_analysis_run_id: Some("run-api-health"),
    })
    .expect("health JSON serializes");

    assert_eq!(json["kind"], "health");
    assert_eq!(json["schema_version"], HEALTH_SCHEMA_VERSION);
    assert!(json["_meta"].is_object());
    assert_eq!(
        json["_meta"]["telemetry"]["analysis_run_id"],
        "run-api-health"
    );
    assert_eq!(json["workspace_diagnostics"][0]["path"], "package.json");
    assert_eq!(json["next_steps"][0]["id"], "inspect-health");
}

#[test]
fn programmatic_health_runner_serializes_api_owned_output() {
    let project = tempfile::tempdir().expect("temp dir");
    let root = project.path().to_path_buf();
    let json = health_json_with_runner(
        &ComplexityOptions {
            analysis: AnalysisOptions {
                explain: true,
                ..AnalysisOptions::default()
            },
            ..ComplexityOptions::default()
        },
        &FakeHealthRunner {
            root,
            telemetry_analysis_run_id: Some("run-123".to_string()),
        },
    )
    .expect("programmatic health should serialize");

    assert_eq!(json["kind"], "health");
    assert_eq!(json["workspace_diagnostics"][0]["path"], "package.json");
    assert_eq!(json["next_steps"][0]["id"], "impact-report");
    assert_eq!(
        json["_meta"]["telemetry"]["analysis_run_id"],
        serde_json::Value::from("run-123")
    );
}

#[test]
fn serialized_duplication_returns_dupes_envelope() {
    let project = tempfile::tempdir().expect("temp dir");
    let root = project.path();
    std::fs::create_dir(root.join("src")).expect("src dir");
    let code = "export function repeated() {\n  return ['a', 'b', 'c'].join(',');\n}\n";
    std::fs::write(root.join("src/a.ts"), code).expect("file");
    std::fs::write(root.join("src/b.ts"), code).expect("file");

    let json = duplication_json(&DuplicationOptions {
        analysis: analysis_at(root),
        min_tokens: Some(1),
        min_lines: Some(1),
        ..DuplicationOptions::default()
    })
    .expect("duplication succeeds");

    assert_eq!(json["kind"], "dupes");
    assert!(json["clone_groups"].is_array());
    assert!(json["stats"].is_object());
}

/// A monorepo whose `workspaces` glob matches a directory with no
/// `package.json` produces a `GlobMatchedNoPackageJson` workspace
/// diagnostic that the CLI surfaces on `workspace_diagnostics[]`, plus
/// unused exports + a clone that drive `next_steps[]`. The api / napi
/// surface must carry the same enrichment the CLI emits.
fn enriched_project() -> tempfile::TempDir {
    let project = tempfile::tempdir().expect("temp dir");
    let root = project.path();
    // `packages/empty` matches the glob but has no package.json -> diagnostic.
    std::fs::create_dir_all(root.join("packages/empty")).expect("empty pkg dir");
    std::fs::write(
        root.join("packages/empty/note.txt"),
        "no package.json here\n",
    )
    .expect("note");
    write_json(
        root.join("package.json"),
        r#"{"name":"api-enriched","main":"src/index.ts","workspaces":["packages/*"]}"#,
    );
    std::fs::create_dir(root.join("src")).expect("src dir");
    std::fs::write(
        root.join("src/index.ts"),
        "import './a';\nimport './b';\nexport const entry = 1;\nconsole.log(entry);\n",
    )
    .expect("entry");
    // Identical bodies so dupes detection (and the trace-clone next step)
    // has a clone to report, plus an unused export per file.
    let clone = "export function repeated() {\n  return ['x', 'y', 'z'].join(',');\n}\n";
    std::fs::write(root.join("src/a.ts"), clone).expect("a");
    std::fs::write(root.join("src/b.ts"), clone).expect("b");
    project
}

fn has_glob_no_package_json(diagnostics: &serde_json::Value) -> bool {
    diagnostics
        .as_array()
        .into_iter()
        .flatten()
        .any(|diag| diag["kind"] == "glob-matched-no-package-json")
}

/// Regression guard: the napi/api dead-code path must populate
/// `workspace_diagnostics` and `next_steps` exactly like the CLI's
/// `serialize_check_json` route does. The pre-fix code hardcoded both to
/// empty, silently dropping the enrichment for `fallow/types` embedders.
#[test]
fn serialized_dead_code_carries_workspace_diagnostics_and_next_steps() {
    let project = enriched_project();
    let root = project.path();

    let json = dead_code_json(&DeadCodeOptions {
        analysis: analysis_at(root),
        filters: DeadCodeFilters {
            unused_exports: true,
            ..DeadCodeFilters::default()
        },
        ..DeadCodeOptions::default()
    })
    .expect("dead-code succeeds");

    // Findings exist, so the enrichment must be present (not the dropped
    // empties the crate-split regression produced).
    assert!(
        !json["unused_exports"].as_array().expect("array").is_empty(),
        "fixture must produce unused exports to drive next_steps"
    );
    assert!(
        has_glob_no_package_json(&json["workspace_diagnostics"]),
        "workspace_diagnostics must carry the glob-no-package-json diagnostic, got {:?}",
        json["workspace_diagnostics"]
    );
    assert!(
        json["next_steps"]
            .as_array()
            .is_some_and(|steps| !steps.is_empty()),
        "next_steps must be populated for a run with findings, got {:?}",
        json["next_steps"]
    );
}

/// Companion regression guard for the duplication path: the napi/api dupes
/// JSON must carry `workspace_diagnostics`, `next_steps`, and (under
/// `explain`) the `_meta` block, matching the CLI's `build_duplication_json`
/// route. The pre-fix code hardcoded `meta: None` and both vecs empty.
#[test]
fn serialized_duplication_carries_meta_diagnostics_and_next_steps() {
    let project = enriched_project();
    let root = project.path();

    let json = duplication_json(&DuplicationOptions {
        analysis: AnalysisOptions {
            explain: true,
            ..analysis_at(root)
        },
        min_tokens: Some(1),
        min_lines: Some(1),
        ..DuplicationOptions::default()
    })
    .expect("duplication succeeds");

    assert!(
        !json["clone_groups"].as_array().expect("array").is_empty(),
        "fixture must produce a clone to drive trace-clone next step"
    );
    assert!(
        json["_meta"].is_object(),
        "explain mode must emit the dupes _meta block, got {:?}",
        json["_meta"]
    );
    assert!(
        has_glob_no_package_json(&json["workspace_diagnostics"]),
        "workspace_diagnostics must carry the glob-no-package-json diagnostic, got {:?}",
        json["workspace_diagnostics"]
    );
    assert!(
        json["next_steps"]
            .as_array()
            .is_some_and(|steps| !steps.is_empty()),
        "next_steps must be populated for a run with clones, got {:?}",
        json["next_steps"]
    );
}

#[test]
fn run_duplication_returns_typed_output_before_json() {
    let project = tempfile::tempdir().expect("temp dir");
    let root = project.path();
    std::fs::create_dir(root.join("src")).expect("src dir");
    std::fs::write(root.join("src/a.ts"), "export const a = 1;\n").expect("file");

    let run = run_duplication(&DuplicationOptions {
        analysis: analysis_at(root),
        ..DuplicationOptions::default()
    })
    .expect("duplication succeeds");

    let _: &crate::DuplicationOutput = &run.output;
    assert_eq!(run.output.schema_version.0, duplication::SCHEMA_VERSION);
    assert!(run.clone_groups().is_empty());
    assert!(run.clone_families().is_empty());
    assert!(run.groups().is_none());
    assert_eq!(run.report().stats.clone_groups, 0);
    assert_eq!(run.root, canonical_root(root));
    assert_eq!(run.envelope_mode, RootEnvelopeMode::Tagged);

    let json =
        serialize_duplication_programmatic_json(run).expect("typed duplication output serializes");
    assert_eq!(json["kind"], "dupes");
}

#[test]
fn run_feature_flags_returns_typed_output_before_json() {
    let project = tempfile::tempdir().expect("temp dir");
    let root = project.path();
    write_json(
        root.join("package.json"),
        r#"{"name":"api-flags","main":"src/index.ts"}"#,
    );
    std::fs::create_dir(root.join("src")).expect("src dir");
    std::fs::write(
        root.join("src/index.ts"),
        "if (process.env.FEATURE_ALPHA) {\n  console.log('on');\n}\n",
    )
    .expect("source");

    let run = run_feature_flags(&FeatureFlagsOptions {
        analysis: AnalysisOptions {
            explain: true,
            ..analysis_at(root)
        },
        top: None,
    })
    .expect("feature flags succeeds");

    assert_eq!(run.output.schema_version.0, CHECK_SCHEMA_VERSION);
    assert_eq!(run.output.total_flags, 1);
    assert_eq!(run.output.feature_flags[0].flag_name, "FEATURE_ALPHA");
    assert_eq!(run.envelope_mode, RootEnvelopeMode::Tagged);

    let json = serialize_feature_flags_programmatic_json(run)
        .expect("typed feature-flags output serializes");
    assert_eq!(json["kind"], "feature-flags");
    assert!(json["_meta"]["feature_flags"].is_object());
}

#[test]
fn serialized_feature_flags_returns_json_adapter_output() {
    let project = tempfile::tempdir().expect("temp dir");
    let root = project.path();
    write_json(
        root.join("package.json"),
        r#"{"name":"api-flags-json","main":"src/index.ts"}"#,
    );
    std::fs::create_dir(root.join("src")).expect("src dir");
    std::fs::write(
        root.join("src/index.ts"),
        "if (process.env.FEATURE_ALPHA) {}\nif (process.env.FEATURE_BETA) {}\n",
    )
    .expect("source");

    let json = feature_flags_json(&FeatureFlagsOptions {
        analysis: analysis_at(root),
        top: Some(1),
    })
    .expect("feature flags succeeds");

    assert_eq!(json["kind"], "feature-flags");
    assert_eq!(json["feature_flags"].as_array().expect("flags").len(), 1);
}

#[test]
fn serialized_dead_code_returns_dead_code_envelope() {
    let project = dead_code_project();
    let root = project.path();

    let json = dead_code_json(&DeadCodeOptions {
        analysis: analysis_at(root),
        filters: DeadCodeFilters {
            unused_exports: true,
            ..DeadCodeFilters::default()
        },
        ..DeadCodeOptions::default()
    })
    .expect("dead-code succeeds");

    assert_eq!(json["kind"], "dead-code");
    assert_eq!(json["schema_version"], CHECK_SCHEMA_VERSION);
    assert_eq!(unused_export_names(&json), vec!["deadA", "deadB"]);
}

#[test]
fn run_dead_code_returns_typed_output_before_json() {
    let project = dead_code_project();
    let root = project.path();

    let run = run_dead_code(&DeadCodeOptions {
        analysis: analysis_at(root),
        filters: DeadCodeFilters {
            unused_exports: true,
            ..DeadCodeFilters::default()
        },
        ..DeadCodeOptions::default()
    })
    .expect("dead-code succeeds");

    let _: &crate::DeadCodeOutput = &run.output;
    assert_eq!(run.output.schema_version.0, CHECK_SCHEMA_VERSION);
    assert_eq!(run.results().unused_exports.len(), 2);
    assert_eq!(run.root(), canonical_root(root));
    assert_eq!(run.envelope_mode, RootEnvelopeMode::Tagged);

    let json =
        serialize_dead_code_programmatic_json(run).expect("typed dead-code output serializes");
    assert_eq!(unused_export_names(&json), vec!["deadA", "deadB"]);
}

#[test]
fn run_dead_code_family_helpers_return_typed_filtered_output() {
    let project = dead_code_project();
    let root = project.path();
    let options = DeadCodeOptions {
        analysis: analysis_at(root),
        ..DeadCodeOptions::default()
    };

    let circular = run_circular_dependencies(&options).expect("circular helper");
    let boundary = run_boundary_violations(&options).expect("boundary helper");

    let _: &crate::CircularDependenciesOutput = &circular.output;
    let _: &crate::BoundaryViolationsOutput = &boundary.output;
    assert!(circular.results().unused_exports.is_empty());
    assert!(boundary.results().unused_exports.is_empty());
    assert!(circular.circular_dependencies().is_empty());
    assert!(boundary.boundary_violations().is_empty());
    assert!(boundary.boundary_coverage_violations().is_empty());
    assert!(boundary.boundary_call_violations().is_empty());
    assert_eq!(circular.output.total_issues, 0);
    assert_eq!(boundary.output.total_issues, 0);
}

#[test]
fn serialized_dead_code_explain_includes_output_owned_meta() {
    let project = dead_code_project();
    let root = project.path();

    let json = dead_code_json(&DeadCodeOptions {
        analysis: AnalysisOptions {
            explain: true,
            ..analysis_at(root)
        },
        filters: DeadCodeFilters {
            unused_exports: true,
            ..DeadCodeFilters::default()
        },
        ..DeadCodeOptions::default()
    })
    .expect("dead-code succeeds");

    assert_eq!(json["kind"], "dead-code");
    assert_eq!(
        json["_meta"]["docs"].as_str(),
        Some(fallow_output::CHECK_DOCS)
    );
    assert!(json["_meta"]["rules"]["unused-export"].is_object());
}

#[test]
fn serialized_dead_code_marks_duplicate_export_config_action_fixable() {
    let project = duplicate_export_project();
    let root = project.path();

    let json = dead_code_json(&DeadCodeOptions {
        analysis: analysis_at(root),
        filters: DeadCodeFilters {
            duplicate_exports: true,
            ..DeadCodeFilters::default()
        },
        ..DeadCodeOptions::default()
    })
    .expect("dead-code succeeds");

    let action = &json["duplicate_exports"][0]["actions"][0];
    assert_eq!(action["type"], "add-to-config");
    assert_eq!(action["auto_fixable"], true);
}

#[test]
fn serialized_combined_marks_duplicate_export_config_action_fixable() {
    let project = duplicate_export_project();
    let root = project.path();

    let output = run_combined(&CombinedOptions {
        analysis: analysis_at(root),
        duplication: false,
        health: false,
        ..CombinedOptions::default()
    })
    .expect("combined succeeds");
    let json = serialize_combined_programmatic_json(output).expect("combined json");

    let action = &json["check"]["duplicate_exports"][0]["actions"][0];
    assert_eq!(action["type"], "add-to-config");
    assert_eq!(action["auto_fixable"], true);
}

#[test]
fn serialized_dead_code_keeps_duplicate_export_config_action_blocked_in_subpackage() {
    let workspace = tempfile::tempdir().expect("temp dir");
    std::fs::write(
        workspace.path().join("pnpm-workspace.yaml"),
        "packages:\n  - packages/*\n",
    )
    .expect("workspace");
    let root = workspace.path().join("packages/app");
    duplicate_export_project_at(&root);

    let json = dead_code_json(&DeadCodeOptions {
        analysis: analysis_at(&root),
        filters: DeadCodeFilters {
            duplicate_exports: true,
            ..DeadCodeFilters::default()
        },
        ..DeadCodeOptions::default()
    })
    .expect("dead-code succeeds");

    let action = &json["duplicate_exports"][0]["actions"][0];
    assert_eq!(action["type"], "add-to-config");
    assert_eq!(action["auto_fixable"], false);
}

#[test]
fn serialized_dead_code_file_filter_scopes_source_findings() {
    let project = dead_code_project();
    let root = project.path();

    let json = dead_code_json(&DeadCodeOptions {
        analysis: analysis_at(root),
        filters: DeadCodeFilters {
            unused_exports: true,
            ..DeadCodeFilters::default()
        },
        files: vec![PathBuf::from("src/a.ts")],
        ..DeadCodeOptions::default()
    })
    .expect("dead-code succeeds");

    assert_eq!(unused_export_names(&json), vec!["deadA"]);
}

#[test]
fn serialized_dead_code_diff_file_filters_source_findings() {
    let project = dead_code_project();
    let root = project.path();
    std::fs::write(
        root.join("a.diff"),
        "diff --git a/src/a.ts b/src/a.ts\n+++ b/src/a.ts\n@@ -1 +1 @@\n+export const deadA = 1;\n",
    )
    .expect("diff");

    let json = dead_code_json(&DeadCodeOptions {
        analysis: AnalysisOptions {
            diff_file: Some(PathBuf::from("a.diff")),
            ..analysis_at(root)
        },
        filters: DeadCodeFilters {
            unused_exports: true,
            ..DeadCodeFilters::default()
        },
        ..DeadCodeOptions::default()
    })
    .expect("dead-code succeeds");

    assert_eq!(unused_export_names(&json), vec!["deadA"]);
}

#[test]
fn serialized_circular_dependencies_keeps_dead_code_envelope_but_filters_other_findings() {
    let project = dead_code_project();
    let root = project.path();

    let json = circular_dependencies_json(&DeadCodeOptions {
        analysis: analysis_at(root),
        ..DeadCodeOptions::default()
    })
    .expect("circular helper succeeds");

    assert_eq!(json["kind"], "dead-code");
    assert_eq!(json["total_issues"], 0);
    assert!(json["circular_dependencies"].as_array().is_some());
    assert!(json["unused_exports"].as_array().is_none_or(Vec::is_empty));
}

#[test]
fn serialized_boundary_violations_keeps_only_boundary_family() {
    let project = dead_code_project();
    let root = project.path();

    let json = boundary_violations_json(&DeadCodeOptions {
        analysis: analysis_at(root),
        ..DeadCodeOptions::default()
    })
    .expect("boundary helper succeeds");

    assert_eq!(json["kind"], "dead-code");
    assert_eq!(json["total_issues"], 0);
    assert!(json["boundary_violations"].as_array().is_some());
    assert!(json["unused_exports"].as_array().is_none_or(Vec::is_empty));
}

#[test]
fn diff_file_filters_clone_groups() {
    let root = PathBuf::from("/repo");
    let mut report = DuplicationReport {
        clone_groups: vec![
            group(vec![
                instance("/repo/src/a.ts", 1, 3),
                instance("/repo/src/b.ts", 1, 3),
            ]),
            group(vec![
                instance("/repo/src/c.ts", 10, 12),
                instance("/repo/src/d.ts", 1, 3),
            ]),
        ],
        stats: DuplicationStats {
            total_files: 4,
            total_lines: 100,
            total_tokens: 100,
            clone_groups: 2,
            clone_instances: 4,
            ..DuplicationStats::default()
        },
        ..DuplicationReport::default()
    };
    let diff = DiffIndex::from_unified_diff(
        "diff --git a/src/a.ts b/src/a.ts\n+++ b/src/a.ts\n@@ -1,3 +1,3 @@\n+added\n context\n",
    );

    filter_by_diff(&mut report, &diff, &root);

    assert_eq!(report.clone_groups.len(), 1);
    assert_eq!(
        report.clone_groups[0].instances[0].file,
        root.join("src/a.ts")
    );
}

#[test]
fn workspace_scope_filters_clone_groups() {
    let root = PathBuf::from("/repo");
    let mut report = DuplicationReport {
        clone_groups: vec![
            group(vec![
                instance("/repo/packages/app/a.ts", 1, 3),
                instance("/repo/packages/shared/b.ts", 1, 3),
            ]),
            group(vec![
                instance("/repo/packages/docs/c.ts", 1, 3),
                instance("/repo/packages/docs/d.ts", 1, 3),
            ]),
        ],
        stats: DuplicationStats {
            total_files: 4,
            total_lines: 100,
            total_tokens: 100,
            clone_groups: 2,
            clone_instances: 4,
            ..DuplicationStats::default()
        },
        ..DuplicationReport::default()
    };

    filter_by_workspaces(&mut report, &[root.join("packages/app")], &root);

    assert_eq!(report.clone_groups.len(), 1);
    assert_eq!(
        report.clone_groups[0].instances[0].file,
        root.join("packages/app/a.ts")
    );
}

#[test]
fn workspace_patterns_match_names_paths_and_negation() {
    let project = tempfile::tempdir().expect("temp dir");
    let root = project.path();
    write_json(
        root.join("package.json"),
        r#"{"workspaces":["packages/*"]}"#,
    );
    write_workspace(root, "packages/app", "@scope/app");
    write_workspace(root, "packages/docs", "docs");

    let roots = resolve_workspace_filters(root, &["packages/*".to_string(), "!docs".to_string()])
        .expect("workspace filters resolve");

    assert_eq!(roots, vec![root.join("packages/app")]);
}

fn instance(path: &str, start_line: usize, end_line: usize) -> CloneInstance {
    CloneInstance {
        file: PathBuf::from(path),
        start_line,
        end_line,
        start_col: 0,
        end_col: 0,
        fragment: String::new(),
    }
}

fn group(instances: Vec<CloneInstance>) -> fallow_types::duplicates::CloneGroup {
    fallow_types::duplicates::CloneGroup {
        instances,
        token_count: 10,
        line_count: 3,
    }
}

fn dead_code_project() -> tempfile::TempDir {
    let project = tempfile::tempdir().expect("temp dir");
    let root = project.path();
    std::fs::create_dir(root.join("src")).expect("src dir");
    write_json(
        root.join("package.json"),
        r#"{"name":"api-dead-code","main":"src/index.ts"}"#,
    );
    std::fs::write(
        root.join("src/index.ts"),
        "import './a';\nimport './b';\nexport const entry = 1;\nconsole.log(entry);\n",
    )
    .expect("entry");
    std::fs::write(root.join("src/a.ts"), "export const deadA = 1;\n").expect("a");
    std::fs::write(root.join("src/b.ts"), "export const deadB = 1;\n").expect("b");
    project
}

fn duplicate_export_project() -> tempfile::TempDir {
    let project = tempfile::tempdir().expect("temp dir");
    duplicate_export_project_at(project.path());
    project
}

fn duplicate_export_project_at(root: &Path) {
    std::fs::create_dir_all(root.join("src")).expect("src dir");
    write_json(
        root.join("package.json"),
        r#"{"name":"api-duplicate-export","main":"src/index.ts"}"#,
    );
    std::fs::write(root.join("src/index.ts"), "import './a';\nimport './b';\n").expect("entry");
    std::fs::write(root.join("src/a.ts"), "export const Button = 1;\n").expect("a");
    std::fs::write(root.join("src/b.ts"), "export const Button = 2;\n").expect("b");
}

fn unused_export_names(json: &serde_json::Value) -> Vec<&str> {
    json["unused_exports"]
        .as_array()
        .expect("unused exports array")
        .iter()
        .map(|item| {
            item["name"]
                .as_str()
                .or_else(|| item["export_name"].as_str())
                .expect("unused export name")
        })
        .collect()
}

fn write_workspace(root: &Path, relative: &str, name: &str) {
    let dir = root.join(relative);
    std::fs::create_dir_all(&dir).expect("workspace dir");
    write_json(dir.join("package.json"), &format!(r#"{{"name":"{name}"}}"#));
}

fn write_json(path: PathBuf, json: &str) {
    std::fs::write(path, json).expect("json file");
}
