//! The programmatic trace API, which the MCP `trace_file` and
//! `trace_dependency` tools call, names the Module Federation source the same
//! way the CLI does (issue #2796).

#![expect(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests and benches use unwrap and expect to keep fixture setup concise"
)]

use std::fs;

use fallow_api::{
    AnalysisOptions, TraceDependencyOptions, TraceFileOptions, run_trace_dependency, run_trace_file,
};

fn write_fixture() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(
        root.join("package.json"),
        r#"{"name":"host","main":"src/index.ts","devDependencies":{"@module-federation/enhanced":"^0.9.0"}}"#,
    )
    .unwrap();
    fs::write(
        root.join("module-federation.config.ts"),
        "export default { exposes: { './Button': './src/Button.tsx' }, remotes: { checkout: 'checkout@x' } };\n",
    )
    .unwrap();
    fs::write(root.join("src/index.ts"), "export const app = 1;\n").unwrap();
    fs::write(root.join("src/Button.tsx"), "export const Button = 1;\n").unwrap();
    dir
}

fn analysis(dir: &tempfile::TempDir) -> AnalysisOptions {
    AnalysisOptions {
        root: Some(dir.path().to_path_buf()),
        no_cache: true,
        ..AnalysisOptions::default()
    }
}

#[test]
fn trace_file_and_trace_dependency_carry_the_federation_sources() {
    let dir = write_fixture();

    let file = run_trace_file(&TraceFileOptions {
        analysis: analysis(&dir),
        file: "src/Button.tsx".to_string(),
    })
    .expect("trace file");
    assert_eq!(file.output.sources.len(), 1, "{:?}", file.output.sources);
    assert_eq!(file.output.sources[0].key, "exposes");
    assert_eq!(
        file.output.sources[0].config,
        std::path::PathBuf::from("module-federation.config.ts")
    );

    let dependency = run_trace_dependency(&TraceDependencyOptions {
        analysis: analysis(&dir),
        package_name: "checkout".to_string(),
    })
    .expect("trace dependency");
    assert_eq!(
        dependency.output.sources.len(),
        1,
        "{:?}",
        dependency.output.sources
    );
    assert_eq!(dependency.output.sources[0].key, "remotes");

    let plain = run_trace_file(&TraceFileOptions {
        analysis: analysis(&dir),
        file: "src/index.ts".to_string(),
    })
    .expect("trace file");
    assert!(plain.output.sources.is_empty());
}
