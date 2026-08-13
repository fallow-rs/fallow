#![expect(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "benches use unwrap and expect to keep fixture setup concise"
)]
#![allow(
    clippy::significant_drop_tightening,
    reason = "the external Criterion macro owns the benchmark lifecycle"
)]

use std::fs;
use std::path::{Path, PathBuf};

use criterion::{BatchSize, Criterion, criterion_group, criterion_main};
use fallow_config::{
    FallowConfig, OutputFormat, discover_workspaces, discover_workspaces_with_diagnostics,
};
use globset::GlobSet;
use tempfile::TempDir;

const BENCH_THREADS: usize = 4;
const WORKSPACE_COUNT: usize = 16;

struct ConfigFixture {
    _temp_dir: TempDir,
    root: PathBuf,
    config_path: PathBuf,
}

fn write_file(root: &Path, path: &str, source: impl AsRef<str>) {
    let path = root.join(path);
    fs::create_dir_all(path.parent().expect("fixture file has parent")).unwrap();
    fs::write(path, source.as_ref()).unwrap();
}

fn create_config_fixture() -> ConfigFixture {
    let temp_dir = TempDir::new().unwrap();
    let root = temp_dir.path().to_path_buf();
    write_file(
        &root,
        "package.json",
        r#"{"name":"bench-config","private":true,"workspaces":["packages/*","apps/*"],"dependencies":{"react":"19.0.0"}}"#,
    );
    write_file(
        &root,
        "pnpm-workspace.yaml",
        r#"
packages:
  - "packages/*"
  - "apps/*"
"#,
    );
    for index in 0..WORKSPACE_COUNT {
        let kind = if index % 3 == 0 { "apps" } else { "packages" };
        write_file(
            &root,
            &format!("{kind}/pkg-{index}/package.json"),
            format!(
                r#"{{"name":"@bench/pkg-{index}","main":"src/index.ts","dependencies":{{"@bench/shared":"workspace:*"}}}}"#
            ),
        );
        write_file(
            &root,
            &format!("{kind}/pkg-{index}/src/index.ts"),
            format!("export const value{index} = {index};\n"),
        );
    }
    let config_path = root.join(".fallowrc.json");
    fs::write(
        &config_path,
        r#"{
  "entry": ["apps/*/src/index.ts"],
  "ignorePatterns": ["**/*.generated.ts"],
  "health": {
    "maxCyclomatic": 12,
    "thresholdOverrides": [
      { "files": ["**/*.test.ts"], "maxCognitive": 40, "maxUnitSize": 500 }
    ]
  },
  "duplicates": { "mode": "mild", "minTokens": 40, "minLines": 5 }
}"#,
    )
    .unwrap();
    ConfigFixture {
        _temp_dir: temp_dir,
        root,
        config_path,
    }
}

fn component_config_load_and_resolve(c: &mut Criterion) {
    c.bench_function("component_config_load_and_resolve", |bencher| {
        bencher.iter_batched_ref(
            create_config_fixture,
            |fixture| {
                let config = FallowConfig::load(&fixture.config_path).unwrap();
                config.resolve(
                    fixture.root.clone(),
                    OutputFormat::Json,
                    BENCH_THREADS,
                    true,
                    true,
                    None,
                )
            },
            BatchSize::LargeInput,
        );
    });
}

fn component_config_workspace_discovery(c: &mut Criterion) {
    c.bench_function("component_config_workspace_discovery", |bencher| {
        bencher.iter_batched_ref(
            create_config_fixture,
            |fixture| discover_workspaces(&fixture.root),
            BatchSize::LargeInput,
        );
    });
}

fn component_config_workspace_diagnostics(c: &mut Criterion) {
    c.bench_function("component_config_workspace_diagnostics", |bencher| {
        bencher.iter_batched_ref(
            create_config_fixture,
            |fixture| {
                discover_workspaces_with_diagnostics(&fixture.root, &GlobSet::empty()).unwrap()
            },
            BatchSize::LargeInput,
        );
    });
}

criterion_group!(
    benches,
    component_config_load_and_resolve,
    component_config_workspace_discovery,
    component_config_workspace_diagnostics
);
criterion_main!(benches);
