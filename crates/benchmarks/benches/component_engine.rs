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
use fallow_engine::{project_analysis::ProjectAnalysisArtifactOptions, session::AnalysisSession};
use tempfile::TempDir;

const FILE_COUNT: usize = 32;
const WARM_FILE_COUNT: usize = 256;

struct EngineFixture {
    _temp_dir: TempDir,
    root: PathBuf,
}

struct WarmEngineFixture {
    _fixture: EngineFixture,
    session: AnalysisSession,
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
        _fixture: fixture,
        session,
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

criterion_group!(
    benches,
    component_engine_session_load,
    component_engine_parsed_parts,
    component_engine_project_analysis_artifacts,
    component_engine_warm_session_dead_code_large,
    component_engine_warm_session_complexity_owned,
    component_engine_warm_session_complexity_shared
);
criterion_main!(benches);
