#![expect(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "benches use unwrap and expect to keep fixture setup concise"
)]
#![allow(
    clippy::significant_drop_tightening,
    reason = "the external Criterion macro owns the benchmark lifecycle"
)]

use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

use criterion::{BatchSize, Criterion, criterion_group, criterion_main};
use fallow_config::{
    DuplicatesConfig, EffectKind, FallowConfig, RulePackDef, RulePackRule, RulePackRuleKind,
    RulesConfig, Severity,
};
use fallow_engine::{
    guard::build_guard_report,
    health::{
        HealthCoverageInputs, HealthExecutionOptions, HealthGateOptions, HealthSort,
        HealthThresholdOverrides, run_ungrouped_health_with_session,
    },
    project_analysis::ProjectAnalysisArtifactOptions,
    session::AnalysisSession,
    trace_chain::trace_symbol_chain_with_session,
};
use fallow_output::{
    RootEnvelopeMode, SuppressionInventoryOutputInput, build_suppression_inventory_output,
    serialize_suppression_inventory_json_output,
};
use fallow_types::output_format::OutputFormat;
use fallow_types::trace_chain::{SymbolChainQuery, TraceDirections};
use tempfile::TempDir;

const FILE_COUNT: usize = 32;
const WARM_FILE_COUNT: usize = 256;
const WARM_CSS_FILE_COUNT: usize = 96;
const CSS_REFERENCE_PATTERNS_PER_CONSUMER: usize = 4;
const WARM_CSS_CONSUMER_FILE_COUNT: usize =
    WARM_CSS_FILE_COUNT / CSS_REFERENCE_PATTERNS_PER_CONSUMER;
const WARM_CSS_TOKENS_PER_FILE: usize = 32;
const CSS_DEEP_THEME_FILE_COUNT: usize = 16;
const CSS_DEEP_COLORS_PER_FILE: usize = 12;
const CSS_DEEP_CVA_FILE_COUNT: usize = 32;
const GUARD_FILE_COUNT: usize = 32;
const GUARD_RULE_COUNT: usize = 8;
const TRACE_CALLER_COUNT: usize = 128;
const SUPPRESSION_FILE_COUNT: usize = 128;
const SUPPRESSIONS_PER_FILE: usize = 4;

struct EngineFixture {
    _temp_dir: TempDir,
    root: PathBuf,
}

struct WarmEngineFixture {
    fixture: EngineFixture,
    session: AnalysisSession,
    config_path: Option<PathBuf>,
}

struct CssDeepEngineFixture {
    fixture: WarmEngineFixture,
    changed_files: Vec<PathBuf>,
}

struct GuardFixture {
    _temp_dir: TempDir,
    config: fallow_config::ResolvedConfig,
    files: Vec<String>,
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

fn create_guard_fixture() -> GuardFixture {
    let temp_dir = TempDir::new().unwrap();
    let root = temp_dir.path().to_path_buf();
    let mut rules = Vec::with_capacity(GUARD_RULE_COUNT);
    for rule_index in 0..GUARD_RULE_COUNT {
        rules.push(RulePackRule {
            id: format!("team-{rule_index}-network"),
            kind: RulePackRuleKind::BannedEffect,
            callees: Vec::new(),
            specifiers: Vec::new(),
            effects: vec![EffectKind::Network],
            exports: Vec::new(),
            ignore_type_only: false,
            files: vec![format!("src/team-{rule_index}/**")],
            exclude: vec![format!("src/team-{rule_index}/generated/**")],
            zones: Vec::new(),
            message: None,
            severity: None,
        });
    }
    let mut config = FallowConfig {
        rules: RulesConfig {
            policy_violation: Severity::Warn,
            ..RulesConfig::default()
        },
        ..FallowConfig::default()
    }
    .resolve(root.clone(), OutputFormat::Json, 1, true, true, None);
    config.rule_packs = vec![RulePackDef {
        schema: None,
        version: 1,
        name: "guard-benchmark".to_string(),
        description: None,
        rules,
    }];

    let mut files = Vec::with_capacity(GUARD_FILE_COUNT);
    for file_index in 0..GUARD_FILE_COUNT {
        let file = format!(
            "src/team-{}/feature-{file_index}.ts",
            file_index % GUARD_RULE_COUNT
        );
        write_file(&root, &file, "export const enabled = true;\n");
        files.push(file);
    }

    GuardFixture {
        _temp_dir: temp_dir,
        config,
        files,
    }
}

fn create_trace_engine_fixture() -> WarmEngineFixture {
    let temp_dir = TempDir::new().unwrap();
    let root = temp_dir.path().to_path_buf();
    write_file(
        &root,
        "package.json",
        r#"{"name":"bench-trace","private":true,"type":"module","main":"src/index.ts","dependencies":{}}"#,
    );
    write_file(
        &root,
        "src/leaf.ts",
        "export function leaf(value: number): number { return value + 1; }\n",
    );
    write_file(
        &root,
        "src/shared.ts",
        "import { leaf } from './leaf';\nexport function target(value: number): number { return leaf(value); }\n",
    );

    let mut entry_imports = String::new();
    let mut entry_uses = String::from("console.log(");
    for index in 0..TRACE_CALLER_COUNT {
        write_file(
            &root,
            &format!("src/consumer-{index}.ts"),
            format!(
                "import {{ target }} from './shared';\nexport function consumer{index}(): number {{ return target({index}); }}\n"
            ),
        );
        writeln!(
            &mut entry_imports,
            "import {{ consumer{index} }} from './consumer-{index}';"
        )
        .unwrap();
        if index > 0 {
            entry_uses.push_str(", ");
        }
        write!(&mut entry_uses, "consumer{index}()").unwrap();
    }
    entry_uses.push_str(");\n");
    write_file(
        &root,
        "src/index.ts",
        format!("{entry_imports}\n{entry_uses}"),
    );

    let fixture = EngineFixture {
        _temp_dir: temp_dir,
        root,
    };
    let session = AnalysisSession::load_default(&fixture.root);
    WarmEngineFixture {
        fixture,
        session,
        config_path: None,
    }
}

fn create_suppression_inventory_fixture() -> WarmEngineFixture {
    let temp_dir = TempDir::new().unwrap();
    let root = temp_dir.path().to_path_buf();
    write_file(
        &root,
        "package.json",
        r#"{"name":"bench-suppressions","private":true,"type":"module","main":"src/index.ts","dependencies":{}}"#,
    );
    write_file(&root, "src/index.ts", "export const entry = true;\n");

    for file_index in 0..SUPPRESSION_FILE_COUNT {
        let mut source = String::new();
        for marker_index in 0..SUPPRESSIONS_PER_FILE {
            writeln!(
                &mut source,
                "// fallow-ignore-next-line unused-export -- benchmark migration\nexport const unused_{file_index}_{marker_index} = {marker_index};"
            )
            .unwrap();
        }
        write_file(&root, &format!("src/legacy/module-{file_index}.ts"), source);
    }

    let fixture = EngineFixture {
        _temp_dir: temp_dir,
        root,
    };
    let session = AnalysisSession::load_default(&fixture.root);
    session
        .analyze_dead_code_with_artifacts(false, false)
        .expect("suppression inventory warm-up succeeds");
    WarmEngineFixture {
        fixture,
        session,
        config_path: None,
    }
}

fn suppression_inventory_json(fixture: &WarmEngineFixture) -> serde_json::Value {
    let analysis = fixture
        .session
        .analyze_dead_code_with_artifacts(false, false)
        .expect("suppression inventory analysis succeeds");
    let output = build_suppression_inventory_output(SuppressionInventoryOutputInput {
        active: &analysis.results.active_suppressions,
        stale: &analysis.results.stale_suppressions,
        root: &fixture.fixture.root,
    });
    serialize_suppression_inventory_json_output(output, RootEnvelopeMode::Tagged, None)
        .expect("suppression inventory JSON serialization succeeds")
}

fn trace_symbol_chain(fixture: &WarmEngineFixture) -> fallow_types::trace_chain::SymbolChainTrace {
    trace_symbol_chain_with_session(
        &fixture.session,
        SymbolChainQuery {
            file: "src/shared.ts",
            symbol: "target",
            depth: 2,
            directions: TraceDirections {
                callers: true,
                callees: true,
            },
        },
    )
    .expect("trace analysis succeeds")
    .expect("trace target exists")
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

fn create_css_reference_engine_fixture() -> WarmEngineFixture {
    let mut fixture = create_warm_css_engine_fixture();
    for consumer_index in 0..WARM_CSS_CONSUMER_FILE_COUNT {
        let static_file_index = consumer_index * CSS_REFERENCE_PATTERNS_PER_CONSUMER;
        let interpolated_file_index = static_file_index + 1;
        let dot_file_index = static_file_index + 2;
        let bracket_file_index = static_file_index + 3;
        write_file(
            &fixture.fixture.root,
            &format!("src/components/consumer-{consumer_index}.tsx"),
            format!(
                r#"
import dotStyles from '../styles/theme-{dot_file_index}.css';
import bracketStyles from '../styles/theme-{bracket_file_index}.css';

const tone = 1;

export function Consumer{consumer_index}() {{
  return (
    <section className="component-{static_file_index}-0">
      <span className={{`component-{interpolated_file_index}-${{tone}}`}} />
      <span className={{dotStyles.component{dot_file_index}2}} />
      <span className={{bracketStyles['component{bracket_file_index}3']}} />
    </section>
  );
}}
"#
            ),
        );
    }
    fixture.session = AnalysisSession::load_default(&fixture.fixture.root);
    fixture
        .session
        .analyze_dead_code_with_complexity()
        .expect("warm-up analysis succeeds");
    fixture
}

fn fixture_color(file_index: usize, color_index: usize) -> String {
    if file_index == 0 && color_index < 2 {
        return format!("#f05a2{}", 8 + color_index);
    }
    let red = (file_index * 17 + color_index * 3) % 256;
    let green = (file_index * 11 + color_index * 5) % 256;
    let blue = (file_index * 7 + color_index * 13) % 256;
    format!("#{red:02x}{green:02x}{blue:02x}")
}

fn create_css_deep_color_candidate_fixture() -> CssDeepEngineFixture {
    let fixture = create_engine_fixture_with_file_count(FILE_COUNT);
    let mut changed_files = Vec::new();
    write_file(
        &fixture.root,
        "package.json",
        r#"{"name":"bench-engine","private":true,"type":"module","main":"src/index.ts","dependencies":{"class-variance-authority":"0.7.0","tailwindcss":"4.0.0"}}"#,
    );

    for file_index in 0..CSS_DEEP_THEME_FILE_COUNT {
        let mut theme = String::from("@theme {\n");
        for color_index in 0..CSS_DEEP_COLORS_PER_FILE {
            let color = fixture_color(file_index, color_index);
            let name = match (file_index, color_index) {
                (0, 0) => "brand".to_owned(),
                (0, 1) => "signal".to_owned(),
                _ => format!("palette-{file_index}-{color_index}"),
            };
            writeln!(&mut theme, "  --color-{name}: {color};").unwrap();
        }
        theme.push_str("}\n");
        let path = format!("src/styles/palette-{file_index}.css");
        write_file(&fixture.root, &path, theme);
        changed_files.push(fixture.root.join(path));
    }

    for file_index in 0..CSS_DEEP_CVA_FILE_COUNT {
        let palette_index = file_index % CSS_DEEP_THEME_FILE_COUNT;
        let mut source = String::from(
            "import { cva } from 'class-variance-authority';\n\
             export const palette = cva('inline-flex', { variants: { tone: {\n",
        );
        for color_index in 0..CSS_DEEP_COLORS_PER_FILE {
            let color = fixture_color(palette_index, color_index);
            writeln!(
                &mut source,
                "  tone{color_index}: 'bg-[{color}] border-[{color}]',"
            )
            .unwrap();
        }
        source.push_str("} } });\n");
        let path = format!("src/components/palette-{file_index}.ts");
        write_file(&fixture.root, &path, source);
        changed_files.push(fixture.root.join(path));
    }

    let session = AnalysisSession::load_default(&fixture.root);
    session
        .analyze_dead_code_with_complexity()
        .expect("warm-up analysis succeeds");
    CssDeepEngineFixture {
        fixture: WarmEngineFixture {
            fixture,
            session,
            config_path: None,
        },
        changed_files,
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
        baseline_mode: fallow_engine::baseline::HealthBaselineMode::Count,
        baseline_mode_explicit: false,
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
        analysis_identity: fallow_types::semantic::SemanticAnalysisIdentity::default(),
        group_by: None,
    }
}

fn warm_css_health_options(fixture: &WarmEngineFixture) -> HealthExecutionOptions<'_> {
    HealthExecutionOptions {
        css: true,
        ..warm_health_options(fixture)
    }
}

fn warm_css_deep_health_options(fixture: &WarmEngineFixture) -> HealthExecutionOptions<'_> {
    HealthExecutionOptions {
        css_deep: true,
        ..warm_css_health_options(fixture)
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

fn component_engine_warm_session_css_deep_color_candidates(c: &mut Criterion) {
    let fixture = create_css_deep_color_candidate_fixture();
    let options = warm_css_deep_health_options(&fixture.fixture);
    let warmup = run_ungrouped_health_with_session(
        &options,
        None,
        &fixture.fixture.session,
        Some(fixture.changed_files.clone()),
    )
    .expect("CSS deep color candidate warm-up succeeds");
    assert!(
        warmup.report.css_analytics.is_some_and(|report| report
            .near_duplicate_theme_tokens
            .iter()
            .any(|candidate| {
                candidate.token == "--color-signal"
                    && candidate.nearest_token.name == "--color-brand"
            })),
        "CSS deep analysis produces near-duplicate color candidates"
    );
    c.bench_function(
        "component_engine_warm_session_css_deep_color_candidates",
        |bencher| {
            bencher.iter(|| {
                run_ungrouped_health_with_session(
                    &options,
                    None,
                    &fixture.fixture.session,
                    Some(fixture.changed_files.clone()),
                )
                .expect("warm CSS deep color candidate analysis succeeds")
            });
        },
    );
}

fn component_engine_first_session_css_health_references_many_files(c: &mut Criterion) {
    let fixture = create_css_reference_engine_fixture();
    let options = warm_css_health_options(&fixture);
    run_ungrouped_health_with_session(&options, None, &fixture.session, None)
        .expect("many-file CSS reference health pre-warm succeeds");
    c.bench_function(
        "component_engine_first_session_css_health_references_many_files",
        |bencher| {
            bencher.iter_batched(
                || AnalysisSession::load_default(&fixture.fixture.root),
                |session| {
                    run_ungrouped_health_with_session(&options, None, &session, None)
                        .expect("first-session many-file CSS reference health analysis succeeds")
                },
                BatchSize::LargeInput,
            );
        },
    );
}

fn component_engine_guard_rule_scope_many_files(c: &mut Criterion) {
    let fixture = create_guard_fixture();
    let warmup = build_guard_report(&fixture.config, &fixture.files)
        .expect("guard rule scope warm-up succeeds");
    assert_eq!(warmup.files.len(), GUARD_FILE_COUNT);
    assert!(
        warmup.files.iter().all(|file| file.policy_rules.len() == 1),
        "each guard target matches its team rule"
    );
    c.bench_function("component_engine_guard_rule_scope_many_files", |bencher| {
        bencher.iter(|| {
            build_guard_report(&fixture.config, &fixture.files)
                .expect("guard rule scope analysis succeeds")
        });
    });
}

fn component_engine_warm_session_trace_symbol_chain(c: &mut Criterion) {
    let fixture = create_trace_engine_fixture();
    let warmup = trace_symbol_chain(&fixture);
    assert!(warmup.symbol_found, "trace target is exported");
    assert_eq!(
        warmup.callers.as_ref().map(Vec::len),
        Some(TRACE_CALLER_COUNT),
        "trace walks every direct caller"
    );
    assert_eq!(
        warmup.callees.as_ref().map(Vec::len),
        Some(1),
        "trace resolves the imported leaf callee"
    );
    c.bench_function(
        "component_engine_warm_session_trace_symbol_chain",
        |bencher| {
            bencher.iter(|| trace_symbol_chain(&fixture));
        },
    );
}

fn component_engine_warm_session_suppression_inventory_json(c: &mut Criterion) {
    let fixture = create_suppression_inventory_fixture();
    let warmup = suppression_inventory_json(&fixture);
    assert_eq!(
        warmup["summary"]["total"].as_u64(),
        Some((SUPPRESSION_FILE_COUNT * SUPPRESSIONS_PER_FILE) as u64),
        "inventory retains every suppression marker"
    );
    assert_eq!(
        warmup["summary"]["files"].as_u64(),
        Some(SUPPRESSION_FILE_COUNT as u64),
        "inventory groups markers by source file"
    );
    c.bench_function(
        "component_engine_warm_session_suppression_inventory_json",
        |bencher| {
            bencher.iter(|| suppression_inventory_json(&fixture));
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
    component_engine_warm_session_css_health_many_files,
    component_engine_warm_session_css_deep_color_candidates,
    component_engine_first_session_css_health_references_many_files,
    component_engine_guard_rule_scope_many_files,
    component_engine_warm_session_trace_symbol_chain,
    component_engine_warm_session_suppression_inventory_json
);
criterion_main!(benches);
