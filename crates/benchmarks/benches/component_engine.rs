#![expect(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "benches use unwrap and expect to keep fixture setup concise"
)]

use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

use criterion::{BatchSize, Criterion, criterion_group, criterion_main};
use fallow_config::DuplicatesConfig;
use fallow_engine::{
    health::{
        HealthCoverageInputs, HealthExecutionOptions, HealthGateOptions, HealthSort,
        HealthThresholdOverrides, run_ungrouped_health_with_session,
    },
    project_analysis::ProjectAnalysisArtifactOptions,
    session::AnalysisSession,
};
use fallow_types::output_format::OutputFormat;
use tempfile::TempDir;

const FILE_COUNT: usize = 32;
const WARM_FILE_COUNT: usize = 256;
const WARM_CSS_FILE_COUNT: usize = 96;
const WARM_CSS_TOKENS_PER_FILE: usize = 32;

struct EngineFixture {
    _temp_dir: TempDir,
    root: PathBuf,
}

struct WarmEngineFixture {
    fixture: EngineFixture,
    session: AnalysisSession,
    config_path: Option<PathBuf>,
}

fn write_file(root: &Path, path: &str, source: impl AsRef<str>) {
    let path = root.join(path);
    fs::create_dir_all(path.parent().expect("fixture file has parent")).unwrap();
    fs::write(path, source.as_ref()).unwrap();
}

fn create_engine_fixture() -> EngineFixture {
    create_engine_fixture_with_file_count(FILE_COUNT)
}

fn create_engine_fixture_with_file_count(file_count: usize) -> EngineFixture {
    let temp_dir = TempDir::new().unwrap();
    let root = temp_dir.path().to_path_buf();
    write_file(
        &root,
        "package.json",
        r#"{"name":"bench-engine","private":true,"type":"module","main":"src/index.ts","dependencies":{}}"#,
    );

    let mut imports = String::new();
    let mut uses = String::new();
    for index in 0..file_count {
        write_file(
            &root,
            &format!("src/module-{index}.ts"),
            format!(
                r"
export const live{index} = {index};
export const unused{index} = live{index} + 1;
export function compute{index}(input: number): number {{
  let value = input;
  value += live{index};
  value += {index};
  return value;
}}
"
            ),
        );
        if index % 2 == 0 {
            writeln!(
                &mut imports,
                "import {{ live{index} }} from './module-{index}';"
            )
            .unwrap();
            writeln!(&mut uses, "console.log(live{index});").unwrap();
        }
    }
    write_file(&root, "src/index.ts", format!("{imports}\n{uses}\n"));
    write_file(
        &root,
        "src/styles.css",
        ":root { --color-accent: #06c; }\n.button { color: var(--color-accent); padding: 0.5rem 1rem; }\n",
    );

    EngineFixture {
        _temp_dir: temp_dir,
        root,
    }
}

fn create_warm_engine_fixture() -> WarmEngineFixture {
    let fixture = create_engine_fixture_with_file_count(WARM_FILE_COUNT);
    let session = AnalysisSession::load_default(&fixture.root);
    session
        .analyze_dead_code_with_complexity()
        .expect("warm-up analysis succeeds");
    WarmEngineFixture {
        fixture,
        session,
        config_path: None,
    }
}

fn create_warm_css_engine_fixture() -> WarmEngineFixture {
    let fixture = create_engine_fixture_with_file_count(WARM_FILE_COUNT);
    for file_index in 0..WARM_CSS_FILE_COUNT {
        let mut css = String::new();
        for token_index in 0..WARM_CSS_TOKENS_PER_FILE {
            writeln!(
                &mut css,
                ".component-{file_index}-{token_index} {{ color: var(--color-{file_index}-{token_index}); --color-{file_index}-{token_index}: #{file_index:02x}{token_index:02x}aa; padding: {token_index}px; }}"
            )
            .unwrap();
        }
        write_file(
            &fixture.root,
            &format!("src/styles/theme-{file_index}.css"),
            css,
        );
    }
    let session = AnalysisSession::load_default(&fixture.root);
    session
        .analyze_dead_code_with_complexity()
        .expect("warm-up analysis succeeds");
    WarmEngineFixture {
        fixture,
        session,
        config_path: None,
    }
}

fn warm_health_options(fixture: &WarmEngineFixture) -> HealthExecutionOptions<'_> {
    HealthExecutionOptions {
        root: &fixture.fixture.root,
        config_path: &fixture.config_path,
        output: OutputFormat::Human,
        no_cache: false,
        threads: 1,
        quiet: true,
        complexity_breakdown: false,
        thresholds: HealthThresholdOverrides {
            max_crap: Some(0.0),
            ..HealthThresholdOverrides::default()
        },
        top: None,
        sort: HealthSort::Cyclomatic,
        production: false,
        production_override: None,
        allow_remote_extends: false,
        changed_since: None,
        diff_index: None,
        use_shared_diff_index: false,
        workspace: None,
        changed_workspaces: None,
        baseline: None,
        save_baseline: None,
        complexity: true,
        file_scores: false,
        coverage_gaps: false,
        config_activates_coverage_gaps: false,
        hotspots: false,
        ownership: false,
        ownership_emails: None,
        targets: false,
        css: false,
        css_deep: false,
        force_full: false,
        score_only_output: false,
        enforce_coverage_gap_gate: true,
        effort: None,
        score: false,
        gates: HealthGateOptions::default(),
        since: None,
        min_commits: None,
        explain: false,
        summary: false,
        save_snapshot: None,
        trend: false,
        coverage_inputs: HealthCoverageInputs::default(),
        performance: false,
        runtime_coverage: None,
        churn_file: None,
        group_by: None,
    }
}

fn warm_css_health_options(fixture: &WarmEngineFixture) -> HealthExecutionOptions<'_> {
    HealthExecutionOptions {
        css: true,
        ..warm_health_options(fixture)
    }
}

fn component_engine_session_load(c: &mut Criterion) {
    c.bench_function("component_engine_session_load", |bencher| {
        bencher.iter_batched_ref(
            create_engine_fixture,
            |fixture| AnalysisSession::load_default(&fixture.root),
            BatchSize::LargeInput,
        );
    });
}

fn component_engine_parsed_parts(c: &mut Criterion) {
    c.bench_function("component_engine_parsed_parts", |bencher| {
        bencher.iter_batched_ref(
            create_engine_fixture,
            |fixture| {
                let session = AnalysisSession::load_default(&fixture.root);
                session.parsed_parts(false)
            },
            BatchSize::LargeInput,
        );
    });
}

fn component_engine_project_analysis_artifacts(c: &mut Criterion) {
    c.bench_function("component_engine_project_analysis_artifacts", |bencher| {
        bencher.iter_batched_ref(
            create_engine_fixture,
            |fixture| {
                let session = AnalysisSession::load_default(&fixture.root);
                session
                    .analyze_project_with_artifacts(
                        &DuplicatesConfig::default(),
                        ProjectAnalysisArtifactOptions {
                            retain_complexity_artifacts: true,
                            retain_graph: true,
                            collect_source_fingerprints: true,
                            ..ProjectAnalysisArtifactOptions::default()
                        },
                    )
                    .unwrap()
            },
            BatchSize::LargeInput,
        );
    });
}

fn component_engine_warm_session_dead_code_large(c: &mut Criterion) {
    let fixture = create_warm_engine_fixture();
    c.bench_function("component_engine_warm_session_dead_code_large", |bencher| {
        bencher.iter(|| fixture.session.analyze_dead_code());
    });
}

fn component_engine_warm_session_complexity_owned(c: &mut Criterion) {
    let fixture = create_warm_engine_fixture();
    c.bench_function(
        "component_engine_warm_session_complexity_owned",
        |bencher| bencher.iter(|| fixture.session.analyze_dead_code_with_complexity()),
    );
}

fn component_engine_warm_session_complexity_shared(c: &mut Criterion) {
    let fixture = create_warm_engine_fixture();
    c.bench_function(
        "component_engine_warm_session_complexity_shared",
        |bencher| {
            bencher.iter(|| {
                fixture
                    .session
                    .analyze_dead_code_with_shared_artifacts(true, false)
            });
        },
    );
}

fn component_engine_warm_session_health(c: &mut Criterion) {
    let fixture = create_warm_engine_fixture();
    let options = warm_health_options(&fixture);
    c.bench_function("component_engine_warm_session_health", |bencher| {
        bencher.iter(|| {
            run_ungrouped_health_with_session(&options, None, &fixture.session, None)
                .expect("warm health analysis succeeds")
        });
    });
}

fn component_engine_warm_session_css_health(c: &mut Criterion) {
    let fixture = create_warm_engine_fixture();
    let options = warm_css_health_options(&fixture);
    run_ungrouped_health_with_session(&options, None, &fixture.session, None)
        .expect("CSS health warm-up succeeds");
    c.bench_function("component_engine_warm_session_css_health", |bencher| {
        bencher.iter(|| {
            run_ungrouped_health_with_session(&options, None, &fixture.session, None)
                .expect("warm CSS health analysis succeeds")
        });
    });
}

fn component_engine_warm_session_css_health_many_files(c: &mut Criterion) {
    let fixture = create_warm_css_engine_fixture();
    let options = warm_css_health_options(&fixture);
    run_ungrouped_health_with_session(&options, None, &fixture.session, None)
        .expect("many-file CSS health warm-up succeeds");
    c.bench_function(
        "component_engine_warm_session_css_health_many_files",
        |bencher| {
            bencher.iter(|| {
                run_ungrouped_health_with_session(&options, None, &fixture.session, None)
                    .expect("warm many-file CSS health analysis succeeds")
            });
        },
    );
}

criterion_group!(
    benches,
    component_engine_session_load,
    component_engine_parsed_parts,
    component_engine_project_analysis_artifacts,
    component_engine_warm_session_dead_code_large,
    component_engine_warm_session_complexity_owned,
    component_engine_warm_session_complexity_shared,
    component_engine_warm_session_health,
    component_engine_warm_session_css_health,
    component_engine_warm_session_css_health_many_files
);
criterion_main!(benches);
