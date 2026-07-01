use std::path::{Path, PathBuf};

use toml::{Table, Value};

#[test]
fn api_consumers_depend_on_api_not_engine_cli_or_core() {
    for manifest in [
        "crates/lsp/Cargo.toml",
        "crates/mcp/Cargo.toml",
        "crates/napi/Cargo.toml",
    ] {
        assert_no_deps(manifest, &["fallow-engine", "fallow-cli", "fallow-core"]);
    }
}

#[test]
fn cli_does_not_depend_on_core() {
    let manifest = read_manifest("crates/cli/Cargo.toml");
    assert!(
        !section_has_dep(&manifest, "dependencies", "fallow-core"),
        "fallow-cli must not depend on fallow-core in production dependencies"
    );
    assert!(
        !section_has_dep(&manifest, "dev-dependencies", "fallow-core"),
        "fallow-cli tests must use public contract crates instead of fallow-core"
    );
}

#[test]
fn root_envelope_compatibility_debt_stays_removed() {
    let root_envelopes =
        std::fs::read_to_string(workspace_root().join("crates/output/src/root_envelopes.rs"))
            .expect("read root envelopes");
    assert!(
        !root_envelopes.contains("RootEnvelopeMode::Legacy"),
        "legacy root envelope mode must not be reintroduced"
    );
    assert!(
        !root_envelopes.contains("remove_root_kind"),
        "root kind stripping must not be reintroduced"
    );
    let compat_docs =
        std::fs::read_to_string(workspace_root().join("docs/backwards-compatibility.md"))
            .expect("read compatibility docs");
    for required in ["top-level `kind` discriminator", "Tagged root envelopes"] {
        assert!(
            compat_docs.contains(required),
            "compatibility docs must keep tagged-envelope guidance: {required}"
        );
    }
}

#[test]
fn lower_contract_crates_do_not_depend_upward() {
    assert_no_deps(
        "crates/types/Cargo.toml",
        &[
            "fallow-config",
            "fallow-output",
            "fallow-api",
            "fallow-engine",
            "fallow-cli",
            "fallow-core",
        ],
    );
    assert_no_deps(
        "crates/config/Cargo.toml",
        &[
            "fallow-output",
            "fallow-api",
            "fallow-engine",
            "fallow-cli",
            "fallow-core",
        ],
    );
    assert_no_deps(
        "crates/output/Cargo.toml",
        &["fallow-api", "fallow-engine", "fallow-cli", "fallow-core"],
    );
}

#[test]
fn api_and_engine_do_not_depend_on_cli() {
    assert_no_deps("crates/api/Cargo.toml", &["fallow-cli"]);
    assert_no_deps("crates/engine/Cargo.toml", &["fallow-api", "fallow-cli"]);
}

#[test]
fn api_does_not_depend_on_core_or_cli() {
    assert_no_deps("crates/api/Cargo.toml", &["fallow-core", "fallow-cli"]);
    for source_path in rust_sources_under(["crates/api/src"]) {
        let source = read_source_without_line_comments(&source_path)
            .unwrap_or_else(|error| panic!("read {source_path}: {error}"));
        for forbidden in [
            "fallow_core::",
            "use fallow_core",
            "fallow_cli::",
            "use fallow_cli",
        ] {
            assert!(
                !source.contains(forbidden),
                "{source_path} must consume fallow-engine or API-owned helpers instead of {forbidden}"
            );
        }
    }
}

#[test]
fn public_boundaries_do_not_wildcard_reexport_internal_type_crates() {
    for source_path in [
        "crates/engine/src/source.rs",
        "crates/engine/src/results.rs",
        "crates/api/src/editor.rs",
    ] {
        let source =
            std::fs::read_to_string(workspace_root().join(source_path)).expect("read source");
        for forbidden in [
            concat!("pub use fallow_types::extract::", "*"),
            concat!("pub use fallow_types::results::", "*"),
            concat!("pub use fallow_types::output_dead_code::", "*"),
        ] {
            assert!(
                !source.contains(forbidden),
                "{source_path} must keep public boundary reexports explicit"
            );
        }
    }
}

#[test]
fn api_editor_contracts_do_not_route_type_contracts_through_engine_facade() {
    let source_path = "crates/api/src/editor.rs";
    let source = std::fs::read_to_string(workspace_root().join(source_path)).expect("read source");
    for forbidden in [
        "pub use fallow_engine::",
        "pub use fallow_engine::source::",
        "pub use fallow_engine::results::",
        "pub type EditorCloneFamily = fallow_engine::",
        "pub type EditorCloneGroup = fallow_engine::",
        "pub type EditorCloneInstance = fallow_engine::",
        "pub type EditorDuplicationReport = fallow_engine::",
        "pub type EditorDuplicationStats = fallow_engine::",
        "pub type EditorMirroredDirectory = fallow_engine::",
        "pub type EditorRefactoringKind = fallow_engine::",
        "pub type EditorRefactoringSuggestion = fallow_engine::",
        "pub type EditorDeadCodeAnalysisOutput = fallow_engine::",
        "pub type EditorProjectAnalysisOutput = fallow_engine::",
    ] {
        assert!(
            !source.contains(forbidden),
            "{source_path} must re-export editor type contracts from fallow-types directly"
        );
    }
}

#[test]
fn api_programmatic_health_runner_does_not_expose_engine_results() {
    let source_path = "crates/api/src/runtime/mod.rs";
    let source = std::fs::read_to_string(workspace_root().join(source_path)).expect("read source");
    for forbidden in [
        "pub analysis: fallow_engine::results::HealthAnalysisResult",
        "pub type ProgrammaticHealthAnalysis = fallow_engine::",
        "pub type ProgrammaticHealthRun = fallow_engine::",
        "pub fn derive_programmatic_health_execution_options",
    ] {
        assert!(
            !source.contains(forbidden),
            "{source_path} must expose API-owned programmatic health runner contracts"
        );
    }

    let lib_path = "crates/api/src/lib.rs";
    let lib = std::fs::read_to_string(workspace_root().join(lib_path)).expect("read source");
    for forbidden in [
        "pub use fallow_engine::{",
        "ComplexityRunOptions, ComplexitySectionOptions, DerivedComplexityOptions",
        "DerivedHealthSections, HealthSectionOptions, derive_complexity_sections",
        "derive_programmatic_health_execution_options",
    ] {
        assert!(
            !lib.contains(forbidden),
            "{lib_path} must expose API-owned health option contracts"
        );
    }
}

#[test]
fn engine_does_not_publish_legacy_graph_cache_resolve_modules() {
    let lib = std::fs::read_to_string(workspace_root().join("crates/engine/src/lib.rs"))
        .expect("read engine lib");
    for forbidden in ["pub mod cache;", "pub mod graph;", "pub mod resolve;"] {
        assert!(
            !lib.contains(forbidden),
            "fallow-engine must keep legacy {forbidden} wrapper modules private or removed"
        );
    }

    for removed in [
        "crates/engine/src/cache.rs",
        "crates/engine/src/graph.rs",
        "crates/engine/src/resolve.rs",
    ] {
        assert!(
            !workspace_root().join(removed).exists(),
            "{removed} must not return as a compatibility wrapper"
        );
    }
}

#[test]
fn api_and_cli_use_duplicate_output_contracts_from_types() {
    let duplicate_contract_types = [
        "CloneFamily",
        "CloneGroup",
        "CloneInstance",
        "DefaultIgnoreSkips",
        "DuplicationReport",
        "DuplicationStats",
        "MirroredDirectory",
        "RefactoringKind",
        "RefactoringSuggestion",
    ];
    for source_path in rust_sources_under(["crates/api/src", "crates/cli/src"]) {
        if source_path == "crates/cli/src/architecture_boundaries.rs" {
            continue;
        }
        let source = read_source_without_line_comments(&source_path)
            .unwrap_or_else(|error| panic!("read {source_path}: {error}"));
        for ty in duplicate_contract_types {
            let forbidden = format!("fallow_engine::{ty}");
            assert!(
                !source.contains(&forbidden),
                "{source_path} must import duplicate output contracts from fallow-types, not fallow-engine"
            );
        }
    }
}

#[test]
fn api_and_cli_use_trace_output_contracts_from_types() {
    let trace_contract_types = [
        "CloneTrace",
        "DependencyTrace",
        "ExportReference",
        "ExportTrace",
        "FileTrace",
        "ImpactClosureGap",
        "ImpactClosureTrace",
        "PipelineTimings",
        "ReExportChain",
        "TracedCloneGroup",
        "TracedExport",
        "TracedReExport",
    ];
    for source_path in rust_sources_under(["crates/api/src", "crates/cli/src"]) {
        if source_path == "crates/cli/src/architecture_boundaries.rs" {
            continue;
        }
        let source = read_source_without_line_comments(&source_path)
            .unwrap_or_else(|error| panic!("read {source_path}: {error}"));
        for ty in trace_contract_types {
            let forbidden = format!("fallow_engine::{ty}");
            assert!(
                !source.contains(&forbidden),
                "{source_path} must import trace output contracts from fallow-types, not fallow-engine"
            );
        }
    }
}

#[test]
fn engine_adapter_modules_are_explicit_public_boundaries() {
    let engine_lib = std::fs::read_to_string(workspace_root().join("crates/engine/src/lib.rs"))
        .expect("read engine lib");
    for required in [
        "pub mod changed_files;",
        "pub mod churn;",
        "pub mod cross_reference;",
        "pub mod dead_code;",
        "pub mod discover;",
        "pub mod duplicates;",
        "pub mod health;",
        "pub mod module_graph;",
        "pub mod plugins;",
        "pub mod project_analysis;",
        "pub mod project_config;",
        "pub mod session;",
        "pub mod source;",
        "pub mod trace;",
        "pub mod trace_chain;",
    ] {
        assert!(
            engine_lib.contains(required),
            "engine module boundary must stay explicit: {required}"
        );
    }

    for private in [
        "pub mod core_backend;",
        "pub mod error;",
        "pub mod git_env;",
        "pub mod public_api;",
        "pub mod results;",
        "pub mod security;",
    ] {
        assert!(
            !engine_lib.contains(private),
            "engine private adapter module must not become a public catch-all boundary: {private}"
        );
    }
}

#[test]
fn api_and_cli_do_not_use_removed_engine_root_adapter_exports() {
    for source_path in rust_sources_under(["crates/api/src", "crates/cli/src"]) {
        if source_path == "crates/cli/src/architecture_boundaries.rs" {
            continue;
        }
        let source = read_source_without_line_comments(&source_path)
            .unwrap_or_else(|error| panic!("read {source_path}: {error}"));
        for forbidden in [
            "fallow_engine::AnalysisSession",
            "fallow_engine::AnalysisSessionArtifacts",
            "fallow_engine::ProjectAnalysisArtifactOptions",
            "fallow_engine::ProjectAnalysisOutput",
            "fallow_engine::ProjectAnalysisArtifacts",
            "fallow_engine::ProjectConfig",
            "fallow_engine::ProjectConfigOptions",
            "fallow_engine::results::",
            "fallow_engine::ChangedFilesError",
            "fallow_engine::changed_files(",
            "fallow_engine::config_for_project(",
            "fallow_engine::config_for_project_analysis(",
            "fallow_engine::discover_entry_points(",
            "fallow_engine::discover_files",
            "fallow_engine::filter_results_by_changed_files",
            "fallow_engine::get_changed_files(",
            "fallow_engine::resolve_cache_max_size_bytes(",
            "fallow_engine::try_get_changed_files",
            "fallow_engine::ChurnResult",
            "fallow_engine::ChurnTrend",
            "fallow_engine::FileChurn",
            "fallow_engine::SinceDuration",
            "fallow_engine::analyze_churn",
            "fallow_engine::is_git_repo(",
            "fallow_engine::parse_since(",
            "fallow_engine::RetainedModuleGraph",
            "fallow_engine::ImpactClosurePaths",
            "fallow_engine::PartitionOrderPaths",
            "fallow_engine::FocusFileFactsPaths",
            "fallow_engine::CoordinationGapPaths",
            "fallow_engine::module_value_exports(",
            "fallow_engine::CrossReferenceResult",
            "fallow_engine::cross_reference(",
            "fallow_engine::trace_clone(",
            "fallow_engine::trace_dependency(",
            "fallow_engine::trace_export(",
            "fallow_engine::trace_file(",
            "fallow_engine::trace_symbol_chain(",
        ] {
            assert!(
                !source.contains(forbidden),
                "{source_path} must use the typed fallow-engine module path instead of removed root export {forbidden}"
            );
        }
    }
}

#[test]
fn cli_json_root_outputs_use_runtime_envelope_mode() {
    let allowed = [
        "crates/cli/src/architecture_boundaries.rs",
        "crates/cli/src/output_runtime.rs",
        "crates/cli/src/output_envelope.rs",
    ];
    for source_path in rust_sources_under(["crates/cli/src"]) {
        if allowed.contains(&source_path.as_str()) {
            continue;
        }
        let source = read_source_without_line_comments(&source_path)
            .unwrap_or_else(|error| panic!("read {source_path}: {error}"));
        let forbidden = "RootEnvelopeMode::Tagged";
        assert!(
            !source.contains(forbidden),
            "{source_path} must use output_runtime::current_root_envelope_mode() for root JSON output"
        );
    }
}

#[test]
fn engine_session_and_dead_code_route_core_calls_through_backend_adapter() {
    for source_path in [
        "crates/engine/src/session.rs",
        "crates/engine/src/dead_code.rs",
        "crates/engine/src/trace.rs",
        "crates/engine/src/trace_chain.rs",
    ] {
        let source =
            std::fs::read_to_string(workspace_root().join(source_path)).expect("read source");
        assert!(
            !source.contains("fallow_core::"),
            "{source_path} must use engine::core_backend instead of direct fallow_core calls"
        );
    }

    let session = read_source_without_line_comments("crates/engine/src/session.rs")
        .expect("read engine session source");
    for forbidden in [
        "analyze_with_usages_from_discovery",
        "analyze_with_usages_and_complexity_from_discovery",
        "analyze_retaining_modules_from_discovery",
    ] {
        assert!(
            !session.contains(forbidden),
            "engine session must own dead-code parse orchestration instead of calling {forbidden}"
        );
    }

    let core_backend = read_source_without_line_comments("crates/engine/src/core_backend.rs")
        .expect("read engine core backend source");
    assert!(
        !core_backend.contains("fallow_core::analyze_with_parse_result"),
        "engine reused-parse analysis must use the engine-owned dead-code phase pipeline"
    );

    let dead_code =
        read_source_without_line_comments("crates/engine/src/dead_code.rs").expect("read source");
    assert!(
        !dead_code.contains("core_backend::analyze_with_parse_result"),
        "engine dead-code facade must not delegate reused-parse analysis to the old core monolith"
    );
}

#[test]
fn api_consumers_do_not_reference_engine_core_or_cli_sources() {
    for source_path in rust_sources_under(["crates/lsp/src", "crates/mcp/src", "crates/napi/src"]) {
        let source = read_source_without_line_comments(&source_path)
            .unwrap_or_else(|error| panic!("read {source_path}: {error}"));
        for forbidden in [
            "fallow_engine::",
            "use fallow_engine",
            "fallow_core::",
            "use fallow_core",
            "fallow_cli::",
            "use fallow_cli",
        ] {
            assert!(
                !source.contains(forbidden),
                "{source_path} must consume fallow-api instead of {forbidden}"
            );
        }
    }
}

#[test]
fn engine_root_facade_does_not_reexport_private_adapter_helpers() {
    let source_path = "crates/engine/src/lib.rs";
    let source = read_source_without_line_comments(source_path)
        .unwrap_or_else(|error| panic!("read {source_path}: {error}"));
    for forbidden in [
        "ChangedFilesSpawnHook",
        "ChurnSpawnHook",
        "analyze_churn_from_file",
        "collect_hidden_dir_scopes",
        "compile_glob_set",
        "discover_dynamically_loaded_entry_points",
        "discover_files_and_config_candidates",
        "discover_infrastructure_entry_points",
        "discover_plugin_entry_point_sets",
        "AnalysisSessionParts",
        "pub use health::",
        "health_scoring",
        "health_ownership",
        "pub use dead_code::",
        "analyze_retaining_modules",
        "analyze_with_file_hashes",
        "filter_to_workspaces",
        "pub use duplicates::",
        "pub use changed_files::",
        "pub use churn::",
        "pub use cross_reference::",
        "pub use discover::",
        "pub use module_graph::",
        "pub use plugins::",
        "pub use project_config::",
        "pub use session::",
        "pub use source::inventory",
        "pub use trace::",
        "pub use trace_chain::",
        "InventoryComplexity",
        "InventoryEntry",
        "walk_source_with_complexity",
    ] {
        assert!(
            !source.contains(forbidden),
            "fallow-engine root facade must not re-export private adapter helper {forbidden}"
        );
    }
}

#[test]
fn engine_core_references_stay_inside_adapter_modules() {
    let allowed = ["crates/engine/src/core_backend.rs"];
    for source_path in rust_sources_under(["crates/engine/src"]) {
        let source = read_source_without_line_comments(&source_path)
            .unwrap_or_else(|error| panic!("read {source_path}: {error}"));
        if source.contains("fallow_core::") || source.contains("use fallow_core") {
            assert!(
                allowed.contains(&source_path.as_str()),
                "{source_path} must route fallow_core access through core_backend or an approved typed adapter still awaiting containment"
            );
        }
    }
}

#[test]
fn engine_source_inventory_owns_public_contracts() {
    let source_path = "crates/engine/src/source.rs";
    let source = std::fs::read_to_string(workspace_root().join(source_path)).expect("read source");
    for forbidden in [
        "pub use fallow_extract::cache::CacheStore",
        "pub use fallow_extract::inventory::",
        "pub type InventoryEntry = fallow_extract::",
        "pub type CacheStore = fallow_extract::",
    ] {
        assert!(
            !source.contains(forbidden),
            "{source_path} must wrap extractor inventory output in engine-owned contracts"
        );
    }

    let lib = std::fs::read_to_string(workspace_root().join("crates/engine/src/lib.rs"))
        .expect("read engine lib");
    assert!(
        !lib.contains("pub use source::CacheStore"),
        "engine root must not publish extractor parse-cache internals"
    );
}

#[test]
fn engine_root_does_not_publish_graph_node_internals() {
    let lib_path = "crates/engine/src/lib.rs";
    let lib = std::fs::read_to_string(workspace_root().join(lib_path)).expect("read engine lib");
    for forbidden in [
        " ModuleGraph,",
        "ModuleNode",
        "ExportSymbol",
        "ResolvedModule",
        "pub use module_graph::{ ModuleNode",
    ] {
        assert!(
            !lib.contains(forbidden),
            "{lib_path} must expose graph snapshots and query helpers, not graph internals"
        );
    }
    for line in lib.lines() {
        assert!(
            !line.contains("ModuleGraph") || line.contains("RetainedModuleGraph"),
            "{lib_path} must expose RetainedModuleGraph, not concrete ModuleGraph"
        );
    }

    let coverage_path = "crates/cli/src/health/coverage.rs";
    let coverage =
        std::fs::read_to_string(workspace_root().join(coverage_path)).expect("read coverage");
    for forbidden in ["fallow_engine::ModuleNode", ".is_test_reachable"] {
        assert!(
            !coverage.contains(forbidden),
            "{coverage_path} must use engine-owned graph export snapshots"
        );
    }

    let module_graph_path = "crates/engine/src/module_graph.rs";
    let module_graph = std::fs::read_to_string(workspace_root().join(module_graph_path))
        .expect("read engine module graph");
    for forbidden in [
        "pub use fallow_graph::",
        "pub type ModuleGraph = fallow_graph::",
        "pub type ModuleNode = fallow_graph::",
        "pub type ExportSymbol = fallow_graph::",
        "pub type ResolvedModule = fallow_graph::",
    ] {
        assert!(
            !module_graph.contains(forbidden),
            "{module_graph_path} must wrap graph internals in engine-owned contracts"
        );
    }
}

#[test]
fn cli_audit_uses_engine_graph_fact_helpers() {
    let source_path = "crates/cli/src/audit.rs";
    let source = std::fs::read_to_string(workspace_root().join(source_path)).expect("read audit");
    for forbidden in [
        "graph.modules",
        ".impact_closure(&changed_ids)",
        ".partition_order(&changed_ids)",
        ".focus_file_facts(&changed_ids)",
    ] {
        assert!(
            !source.contains(forbidden),
            "{source_path} must ask fallow-engine for path-resolved graph facts"
        );
    }
}

fn read_source_without_line_comments(path: &str) -> std::io::Result<String> {
    let source = std::fs::read_to_string(workspace_root().join(path))?;
    Ok(source
        .lines()
        .filter(|line| !line.trim_start().starts_with("//"))
        .collect::<Vec<_>>()
        .join("\n"))
}

fn assert_no_deps(manifest_path: &str, forbidden: &[&str]) {
    let manifest = read_manifest(manifest_path);
    for section in ["dependencies", "dev-dependencies", "build-dependencies"] {
        for dep in forbidden {
            assert!(
                !section_has_dep(&manifest, section, dep),
                "{manifest_path} must not list {dep} under {section}"
            );
        }
    }
}

fn rust_sources_under<const N: usize>(roots: [&str; N]) -> Vec<String> {
    let mut sources = Vec::new();
    for root in roots {
        collect_rust_sources(&workspace_root().join(root), root, &mut sources);
    }
    sources.sort();
    sources
}

fn collect_rust_sources(dir: &Path, relative_dir: &str, out: &mut Vec<String>) {
    for entry in
        std::fs::read_dir(dir).unwrap_or_else(|error| panic!("read {relative_dir}: {error}"))
    {
        let entry = entry.unwrap_or_else(|error| panic!("read entry in {relative_dir}: {error}"));
        let path = entry.path();
        let file_name = entry.file_name();
        let file_name = file_name.to_string_lossy();
        let relative_path = format!("{relative_dir}/{file_name}");
        if path.is_dir() {
            collect_rust_sources(&path, &relative_path, out);
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            out.push(relative_path);
        }
    }
}

fn section_has_dep(manifest: &Value, section: &str, dep: &str) -> bool {
    manifest
        .get(section)
        .and_then(Value::as_table)
        .is_some_and(|deps| deps.contains_key(dep))
}

fn read_manifest(path: &str) -> Value {
    let text = std::fs::read_to_string(workspace_root().join(path)).expect("read Cargo.toml");
    Value::Table(text.parse::<Table>().expect("parse Cargo.toml"))
}

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("workspace root")
        .to_path_buf()
}
