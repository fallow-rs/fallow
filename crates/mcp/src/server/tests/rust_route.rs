//! The server handlers pick the Rust route from the root, and only from it.

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;

use super::super::FallowMcp;
use crate::params::{
    AnalyzeParams, CodeExecuteParams, FindDupesParams, MutationInspectParams, MutationPlanParams,
    ProjectInfoParams,
};

/// A stub binary that writes its arguments to `args.txt` and prints `stdout`.
fn stub(dir: &Path, name: &str, stdout: &str) -> PathBuf {
    let report = dir.join(format!("{name}.json"));
    std::fs::write(&report, stdout).expect("write report");
    let binary = dir.join(name);
    std::fs::write(
        &binary,
        format!(
            "#!/bin/sh\necho \"$@\" > '{}'\ncat '{}'\n",
            dir.join(format!("{name}.args")).display(),
            report.display()
        ),
    )
    .expect("write stub");
    std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o755)).expect("chmod stub");
    binary
}

struct Fixture {
    dir: tempfile::TempDir,
    server: FallowMcp,
}

impl Fixture {
    fn new(manifests: &[&str]) -> Self {
        let dir = tempfile::tempdir().expect("temp dir");
        for manifest in manifests {
            let content = if *manifest == "package.json" {
                "{}"
            } else {
                ""
            };
            std::fs::write(dir.path().join(manifest), content).expect("write manifest");
        }
        let tools = dir.path().join("bin");
        std::fs::create_dir(&tools).expect("bin dir");
        let fallow = stub(&tools, "fallow", r#"{"kind":"js"}"#);
        let cargo_fallow = stub(
            &tools,
            "cargo-fallow",
            r#"{"kind":"rust","language":"rust"}"#,
        );
        let server = FallowMcp {
            binary: fallow.to_string_lossy().into_owned(),
            cargo_fallow: cargo_fallow.to_string_lossy().into_owned(),
            tool_router: FallowMcp::tool_router(),
        };
        Self { dir, server }
    }

    fn root(&self) -> String {
        self.dir.path().to_string_lossy().into_owned()
    }

    /// The arguments of the last run of `binary`, or `None` when it never ran.
    fn args_of(&self, binary: &str) -> Option<String> {
        std::fs::read_to_string(self.dir.path().join("bin").join(format!("{binary}.args"))).ok()
    }
}

fn json_of(result: &CallToolResult) -> serde_json::Value {
    let text: String = result
        .content
        .iter()
        .filter_map(|block| block.as_text().map(|text| text.text.clone()))
        .collect();
    serde_json::from_str(&text).expect("json result")
}

#[tokio::test]
async fn analyze_on_a_rust_root_runs_the_combined_check() {
    let fixture = Fixture::new(&["Cargo.toml"]);
    let params = AnalyzeParams {
        root: Some(fixture.root()),
        ..AnalyzeParams::default()
    };
    let result = fixture
        .server
        .analyze(Parameters(params))
        .await
        .expect("result");
    assert_eq!(json_of(&result)["language"], "rust");
    let args = fixture.args_of("cargo-fallow").expect("cargo-fallow ran");
    assert!(args.starts_with("check --manifest-path"), "{args}");
    assert!(args.ends_with("--only deps --only visibility\n"), "{args}");
}

#[tokio::test]
async fn project_info_on_a_rust_root_is_a_typed_error_that_runs_nothing() {
    let fixture = Fixture::new(&["Cargo.toml"]);
    let params = ProjectInfoParams {
        root: Some(fixture.root()),
        ..ProjectInfoParams::default()
    };
    let result = fixture
        .server
        .project_info(Parameters(params))
        .await
        .expect("result");
    assert_eq!(result.is_error, Some(true));
    let value = json_of(&result);
    assert_eq!(value["code"], "FALLOW_RUST_UNSUPPORTED_TOOL");
    assert!(
        value["help"]
            .as_str()
            .is_some_and(|help| help.contains("analyze"))
    );
    assert_eq!(fixture.args_of("cargo-fallow"), None);
    assert_eq!(fixture.args_of("fallow"), None);
}

#[tokio::test]
async fn mutation_plan_on_a_rust_root_lists_the_mutants() {
    let fixture = Fixture::new(&["Cargo.toml"]);
    let params = MutationPlanParams {
        root: Some(fixture.root()),
        package: Some("core".into()),
        ..MutationPlanParams::default()
    };
    let result = fixture
        .server
        .mutation_plan(Parameters(params))
        .await
        .expect("result");
    assert_ne!(result.is_error, Some(true), "{result:?}");
    let args = fixture.args_of("cargo-fallow").expect("cargo-fallow ran");
    assert!(args.starts_with("mutants --manifest-path"), "{args}");
    assert!(args.ends_with("--list --package core\n"), "{args}");
}

#[tokio::test]
async fn a_mixed_root_keeps_the_javascript_route_and_gets_a_warning() {
    let fixture = Fixture::new(&["Cargo.toml", "package.json"]);
    let params = FindDupesParams {
        root: Some(fixture.root()),
        ..FindDupesParams::default()
    };
    let result = fixture
        .server
        .find_dupes(Parameters(params))
        .await
        .expect("result");
    assert_ne!(result.is_error, Some(true), "{result:?}");
    let value = json_of(&result);
    let warnings = value["warnings"].as_array().expect("warnings array");
    assert!(
        warnings
            .iter()
            .any(|warning| warning.as_str().is_some_and(|w| w.contains("cargo fallow"))),
        "{value}"
    );
    assert_eq!(fixture.args_of("cargo-fallow"), None);
}

#[tokio::test]
async fn mutation_inspect_on_a_rust_root_reads_the_cache_only() {
    let fixture = Fixture::new(&["Cargo.toml"]);
    let params = MutationInspectParams {
        mutant_id: "m-1".into(),
        report: None,
        root: Some(fixture.root()),
        config: None,
        allow_remote_extends: None,
        package: Some("core".into()),
    };
    let result = fixture
        .server
        .mutation_inspect(Parameters(params))
        .await
        .expect("result");
    assert_eq!(json_of(&result)["language"], "rust");
    let args = fixture.args_of("cargo-fallow").expect("cargo-fallow ran");
    assert!(
        args.ends_with("--package core --inspect m-1 --no-build\n"),
        "{args}"
    );
}

/// The test process runs in `crates/mcp`, a Cargo package with no
/// `package.json`. A call without a root so takes the route of the server
/// folder, as a server that an agent starts in a Rust workspace does.
#[tokio::test]
async fn a_call_without_a_root_takes_the_route_of_the_server_folder() {
    let fixture = Fixture::new(&[]);
    let result = fixture
        .server
        .find_dupes(Parameters(FindDupesParams::default()))
        .await
        .expect("result");
    assert_eq!(json_of(&result)["language"], "rust");
    let args = fixture.args_of("cargo-fallow").expect("cargo-fallow ran");
    assert!(
        args.starts_with("dupes --manifest-path ./Cargo.toml"),
        "{args}"
    );

    let params = CodeExecuteParams {
        code: "return 1;".into(),
        root: None,
        timeout_ms: Some(5_000),
        max_output_bytes: None,
    };
    let result = fixture
        .server
        .code_execute(Parameters(params))
        .await
        .expect("result");
    assert_eq!(result.is_error, Some(true));
    assert_eq!(json_of(&result)["code"], "FALLOW_RUST_UNSUPPORTED_TOOL");
}
