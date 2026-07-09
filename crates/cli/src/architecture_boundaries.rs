use std::path::{Path, PathBuf};

use toml::{Table, Value};

#[test]
fn repo_architecture_north_star_stays_documented() {
    let migration_doc =
        std::fs::read_to_string(workspace_root().join("docs/fallow-core-migration.md"))
            .expect("read core migration doc");
    for required in [
        "Architecture north star",
        "deterministic repo-intelligence engine",
        "Engine-first",
        "Contracts-first",
        "Session reuse before broad persistence",
        "Repo-policy as code",
        "Core stays backend-only",
        "adapter boundary over the internal `fallow-core` backend",
    ] {
        assert!(
            migration_doc.contains(required),
            "core migration doc must keep the architecture north star: {required}"
        );
    }
    assert!(
        !migration_doc.contains("ADR-008"),
        "public core migration doc must stay self-contained instead of requiring private ADR context"
    );
    for forbidden in [
        "while engine migration is in progress",
        "while `fallow-engine` still builds on it",
        "owns the migration boundary",
        "current exceptions",
        "current migration exceptions",
    ] {
        assert!(
            !migration_doc.contains(forbidden),
            "core migration doc must describe final backend boundaries, not migration leftovers: {forbidden}"
        );
    }
}

#[test]
fn architecture_invariants_doc_tracks_guarded_boundaries() {
    let doc = std::fs::read_to_string(workspace_root().join("docs/architecture-invariants.md"))
        .expect("read architecture invariants doc");
    for required in [
        "backend adapter containment",
        "shared output-helper ownership",
        "manifest/docs drift",
        "drift-tested against `docs/output-schema.json`",
        "SARIF builders",
        "Boundary Policy",
        "`fallow-core` is a backend implementation crate",
    ] {
        assert!(
            doc.contains(required),
            "architecture invariants doc must describe guarded boundary: {required}"
        );
    }
    assert!(
        !doc.contains("Broader layering rules still need human review"),
        "architecture invariants doc must not describe guarded boundaries as manual-only review"
    );
    for forbidden in [
        "Current Exceptions",
        "current exceptions",
        "current migration exceptions",
        "migration exceptions",
        "while engine migration continues",
        "known migration states",
        "still owns some CI",
        "move toward `fallow-output`",
    ] {
        assert!(
            !doc.contains(forbidden),
            "architecture invariants doc must describe final boundaries, not known leftovers: {forbidden}"
        );
    }
}

#[test]
fn contributor_architecture_map_and_roadmap_track_current_ownership() {
    let contributing = std::fs::read_to_string(workspace_root().join("CONTRIBUTING.md"))
        .expect("read contributing guide");
    let project_structure = contributing
        .split_once("## Project structure")
        .and_then(|(_, rest)| rest.split_once("\n## ").map(|(section, _)| section))
        .expect("find project structure section");

    for manifest_path in workspace_crate_manifests() {
        let crate_path = manifest_path
            .strip_suffix("Cargo.toml")
            .expect("workspace manifest path ends in Cargo.toml");
        let documented_path = format!("`{crate_path}`");
        assert!(
            project_structure.contains(&documented_path),
            "CONTRIBUTING project map must include workspace crate {documented_path}"
        );
    }

    for required in [
        "Internal detector backend used by `fallow-engine`",
        "Analysis sessions, discovery, parsing, graph construction",
        "Shared output contracts",
        "Supported Rust facade",
        "CLI protocol adapter",
        "LSP adapter",
        "MCP adapter",
        "NAPI adapter",
        "`editors/vscode/`",
        "`editors/zed/`",
        "`editors/nvim/`",
        "`npm/fallow/`",
        "`npm/<platform>/`",
    ] {
        assert!(
            project_structure.contains(required),
            "CONTRIBUTING project map must preserve current ownership: {required}"
        );
    }
    assert!(
        !project_structure
            .contains("Analysis engine: plugins, discovery, parsing, resolution, graph, detection"),
        "CONTRIBUTING must not assign engine orchestration to fallow-core"
    );

    let roadmap =
        std::fs::read_to_string(workspace_root().join("ROADMAP.md")).expect("read roadmap");
    assert!(
        !roadmap.contains("### Codebase health grade"),
        "ROADMAP must not present the shipped health grade as planned work"
    );
    for required in [
        "### Health score calibration and adoption",
        "Shipped today",
        "0-100 score",
        "A-F letter grade",
        "badge output",
        "saved vital-sign snapshots",
        "trend comparisons",
        "multi-signal explainability",
    ] {
        assert!(
            roadmap.contains(required),
            "ROADMAP must distinguish shipped health scoring from planned calibration: {required}"
        );
    }
}

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
fn only_engine_depends_on_core_as_backend_adapter() {
    let allowed = ["crates/engine/Cargo.toml"];
    for manifest_path in workspace_crate_manifests() {
        if manifest_path == "crates/core/Cargo.toml" || allowed.contains(&manifest_path.as_str()) {
            continue;
        }
        assert_no_deps(&manifest_path, &["fallow-core"]);
    }
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
        "crates/security/Cargo.toml",
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
fn core_publish_status_matches_engine_dependency() {
    let manifest = read_manifest("crates/core/Cargo.toml");
    let engine = read_manifest("crates/engine/Cargo.toml");
    let engine_depends_on_core = section_has_dep(&engine, "dependencies", "fallow-core");
    let core_publish_disabled = manifest
        .get("package")
        .and_then(Value::as_table)
        .and_then(|package| package.get("publish"))
        == Some(&Value::Boolean(false));
    assert!(
        !engine_depends_on_core || !core_publish_disabled,
        "fallow-core cannot be publish=false while fallow-engine has a normal dependency on it"
    );

    let release_workflow =
        std::fs::read_to_string(workspace_root().join(".github/workflows/release.yml"))
            .expect("read release workflow");
    assert!(
        !engine_depends_on_core || release_workflow.contains("fallow-core fallow-engine"),
        "release workflow must publish fallow-core before fallow-engine until engine no longer depends on it"
    );
}

#[test]
fn engine_owns_parse_cache_size_policy() {
    let project_config = read_source_without_line_comments("crates/engine/src/project_config.rs")
        .expect("read engine project config");
    let core_backend = read_source_without_line_comments("crates/engine/src/core_backend.rs")
        .expect("read core backend adapter");
    assert!(
        project_config.contains("fallow_extract::cache::DEFAULT_CACHE_MAX_SIZE"),
        "engine project config must own parse-cache max-size fallback policy"
    );
    assert!(
        !core_backend.contains("resolve_cache_max_size_bytes"),
        "parse-cache size policy must not round-trip through the fallow-core adapter"
    );
    assert!(
        !core_backend.contains("collect_file_hashes"),
        "session-owned artifact metadata must not live in the fallow-core adapter"
    );
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
fn core_does_not_publish_engine_owned_copy_modules() {
    let core_lib = std::fs::read_to_string(workspace_root().join("crates/core/src/lib.rs"))
        .expect("read core lib");
    for forbidden in [
        "pub mod churn;",
        "pub mod cross_reference;",
        "pub mod trace;",
        "pub mod trace_chain;",
    ] {
        assert!(
            !core_lib.contains(forbidden),
            "fallow-core must not republish engine-owned copy module: {forbidden}"
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
fn cli_audit_styling_rendering_uses_output_contract_helpers() {
    let source_path = "crates/cli/src/audit_output.rs";
    let source = read_source_without_line_comments(source_path)
        .unwrap_or_else(|error| panic!("read {source_path}: {error}"));
    for required in [
        "fallow_output::styling_candidate_count",
        "fallow_output::styling_audit_context_label",
    ] {
        assert!(
            source.contains(required),
            "{source_path} must use shared fallow-output audit styling render helpers"
        );
    }
    for forbidden in ["fn styling_candidate_count", "let severity_label"] {
        assert!(
            !source.contains(forbidden),
            "{source_path} must not re-own shared audit styling render fact `{forbidden}`"
        );
    }
}

#[test]
fn api_does_not_own_sarif_family_assembly() {
    for source_path in [
        "crates/api/src/dead_code_sarif.rs",
        "crates/api/src/sarif_output.rs",
    ] {
        let source = read_source_without_line_comments(source_path)
            .unwrap_or_else(|error| panic!("read {source_path}: {error}"));
        for forbidden in [
            "SarifFindingFields",
            "SarifSourceSnippetCache",
            "SarifDocumentInput",
            "build_sarif_document",
            "build_sarif_result_with_snippet",
            "append_sarif_findings",
            "struct SarifCtx",
            "type SarifRuleBuilder",
            "fn sarif_",
            "fn push_",
        ] {
            assert!(
                !source.contains(forbidden),
                "{source_path} must delegate SARIF-family assembly to fallow-output instead of owning `{forbidden}`"
            );
        }
    }
}

#[test]
fn output_owns_sarif_family_assembly() {
    let dead_code = read_source_without_line_comments("crates/output/src/dead_code_sarif.rs")
        .expect("read output dead-code SARIF module");
    for required in [
        "pub fn build_dead_code_sarif",
        "SarifFindingFields",
        "SarifSourceSnippetCache",
        "append_sarif_findings",
        "build_sarif_document",
    ] {
        assert!(
            dead_code.contains(required),
            "fallow-output must own dead-code SARIF assembly: {required}"
        );
    }

    let analysis = read_source_without_line_comments("crates/output/src/analysis_sarif.rs")
        .expect("read output analysis SARIF module");
    for required in [
        "pub fn build_duplication_sarif",
        "pub fn build_grouped_duplication_sarif",
        "pub fn build_health_sarif",
        "pub fn annotate_sarif_results",
    ] {
        assert!(
            analysis.contains(required),
            "fallow-output must own analysis SARIF assembly: {required}"
        );
    }
}

#[test]
fn include_dupes_reuses_dead_code_discovery_artifacts() {
    let check = read_source_without_line_comments("crates/cli/src/check/mod.rs")
        .expect("read check module");
    let output = read_source_without_line_comments("crates/cli/src/check/output.rs")
        .expect("read check output module");
    assert!(
        check.contains("opts.include_dupes")
            && check.contains("AnalysisSession::from_resolved_config")
            && check.contains(".analyze_dead_code_retaining_files"),
        "check --include-dupes must retain discovered files from the shared AnalysisSession dead-code run"
    );
    assert!(
        check.contains("result.retained_files.as_deref()")
            || check.contains("result\n            .retained_files\n            .as_deref()"),
        "check --include-dupes must pass retained discovered files into cross-reference"
    );
    assert!(
        !output.contains("discover_files_with_plugin_scopes"),
        "check cross-reference output must not rediscover files after dead-code analysis"
    );
}

#[test]
fn check_command_dead_code_routes_through_analysis_session() {
    let check = read_source_without_line_comments("crates/cli/src/check/mod.rs")
        .expect("read check module");
    assert!(
        check.contains("AnalysisSession::from_resolved_config"),
        "check must build an AnalysisSession before dead-code variants"
    );
    for forbidden in [
        "fallow_engine::dead_code::analyze(",
        "fallow_engine::dead_code::analyze_with_trace",
        "fallow_engine::dead_code::analyze_retaining_files",
        "fallow_engine::dead_code::analyze_retaining_modules",
    ] {
        assert!(
            !check.contains(forbidden),
            "check must route dead-code analysis through AnalysisSession instead of {forbidden}"
        );
    }
}

#[test]
fn security_command_dead_code_routes_through_analysis_session() {
    let security = read_source_without_line_comments("crates/cli/src/security.rs")
        .expect("read security module");
    assert!(
        security.contains("AnalysisSession::from_resolved_config"),
        "security must build an AnalysisSession before dead-code variants"
    );
    for forbidden in [
        "fallow_engine::dead_code::analyze(",
        "fallow_engine::dead_code::analyze_retaining_modules",
    ] {
        assert!(
            !security.contains(forbidden),
            "security must route dead-code analysis through AnalysisSession instead of {forbidden}"
        );
    }
}

#[test]
fn fix_command_dead_code_routes_through_analysis_session() {
    let fix =
        read_source_without_line_comments("crates/cli/src/fix/mod.rs").expect("read fix module");
    assert!(
        fix.contains("AnalysisSession::from_resolved_config"),
        "fix must build an AnalysisSession before dead-code analysis"
    );
    assert!(
        !fix.contains("fallow_engine::dead_code::analyze_with_file_hashes"),
        "fix must collect file hashes through AnalysisSession artifacts"
    );
}

#[test]
fn coverage_upload_dead_code_routes_through_analysis_session() {
    for source_path in [
        "crates/cli/src/coverage/upload_static_findings.rs",
        "crates/cli/src/coverage/upload_inventory.rs",
    ] {
        let source =
            read_source_without_line_comments(source_path).expect("read coverage upload module");
        assert!(
            source.contains("AnalysisSession::from_resolved_config"),
            "{source_path} must build an AnalysisSession before dead-code analysis"
        );
        for forbidden in [
            "fallow_engine::dead_code::analyze(",
            "fallow_engine::dead_code::analyze_retaining_modules",
        ] {
            assert!(
                !source.contains(forbidden),
                "{source_path} must route dead-code analysis through AnalysisSession instead of {forbidden}"
            );
        }
    }
}

#[test]
fn watch_command_dead_code_routes_through_analysis_session() {
    let watch =
        read_source_without_line_comments("crates/cli/src/watch.rs").expect("read watch module");
    assert!(
        watch.contains("AnalysisSession::from_resolved_config"),
        "watch must build an AnalysisSession before dead-code analysis"
    );
    assert!(
        !watch.contains("fallow_engine::dead_code::analyze("),
        "watch must route dead-code analysis through AnalysisSession"
    );
}

#[test]
fn feature_flags_reuse_session_parse_and_discovery() {
    let flags = read_source_without_line_comments("crates/engine/src/flags.rs")
        .expect("read engine flags module");
    assert!(
        flags.contains("analyze_feature_flags_with_session"),
        "feature flags must expose the session-backed runner"
    );
    for forbidden in [
        "discover_files_with_plugin_scopes",
        "parse_files_for_config",
        "analyze_with_parse_result",
    ] {
        assert!(
            !flags.contains(forbidden),
            "feature flags must reuse AnalysisSession parse/discovery instead of {forbidden}"
        );
    }
}

#[test]
fn list_surfaces_reuse_session_discovery() {
    for source_path in ["crates/cli/src/list.rs", "crates/api/src/list_runtime.rs"] {
        let source = read_source_without_line_comments(source_path).expect("read source");
        assert!(
            source.contains("AnalysisSession::from_"),
            "{source_path} must build an AnalysisSession before collecting discovered files"
        );
        assert!(
            !source.contains("discover_files_with_plugin_scopes"),
            "{source_path} must reuse AnalysisSession discovery instead of direct discovery"
        );
        if source_path == "crates/cli/src/list.rs" {
            assert!(
                source.contains("session.workspaces()")
                    && source.contains("session.workspace_diagnostics()"),
                "{source_path} must reuse AnalysisSession workspace metadata when a session already exists"
            );
        }
    }
}

#[test]
fn list_surfaces_delegate_inventory_composition_to_engine() {
    for source_path in ["crates/cli/src/list.rs", "crates/api/src/list_runtime.rs"] {
        let source = read_source_without_line_comments(source_path).expect("read source");
        assert!(
            source.contains("fallow_engine::list_inventory"),
            "{source_path} must use engine-owned list inventory helpers"
        );
        for forbidden in [
            "discover_entry_points(",
            "discover_workspace_entry_points(",
            "discover_plugin_entry_points(",
            "PluginRegistry::new",
            "PackageJson::load",
            "merge_active_plugins_from",
        ] {
            assert!(
                !source.contains(forbidden),
                "{source_path} must not own list inventory composition helper `{forbidden}`"
            );
        }
    }
}

#[test]
fn coverage_inventory_reuses_session_discovery() {
    let source = read_source_without_line_comments("crates/cli/src/coverage/upload_inventory.rs")
        .expect("read coverage upload inventory");
    assert!(
        source.contains("AnalysisSession::from_resolved_config"),
        "coverage upload-inventory must create one AnalysisSession for inventory discovery"
    );
    assert!(
        source.contains("fn collect_inventory(\n    session: &AnalysisSession"),
        "coverage inventory collection must receive the shared AnalysisSession"
    );
    assert!(
        source.contains("fn collect_caller_edges(\n    session: &AnalysisSession"),
        "caller-edge collection must reuse the inventory AnalysisSession"
    );
    assert!(
        !source.contains("discover_files_with_plugin_scopes"),
        "coverage upload-inventory must reuse AnalysisSession discovery instead of direct discovery"
    );
}

#[test]
fn decision_surface_reuses_session_workspace_metadata() {
    let source = read_source_without_line_comments("crates/api/src/runtime/decision_surface.rs")
        .expect("read decision surface runtime");
    assert!(
        source.contains("session.workspaces()"),
        "decision surface must reuse workspace metadata captured by AnalysisSession"
    );
    assert!(
        !source.contains("discover_workspaces("),
        "decision surface must not rediscover workspaces after building an AnalysisSession"
    );
    assert!(
        !source.contains("PackageJson::load"),
        "decision surface must not own root package metadata reads after building an AnalysisSession"
    );

    let review_deltas =
        read_source_without_line_comments("crates/api/src/review_deltas.rs").expect("read deltas");
    assert!(
        review_deltas.contains("fallow_engine::project_analysis::public_export_keys_for_graph"),
        "review deltas must use engine-owned public API graph policy"
    );
    assert!(
        !review_deltas.contains("fallow_config::PackageJson"),
        "review deltas must not own package metadata policy"
    );

    let audit = read_source_without_line_comments("crates/cli/src/audit.rs").expect("read audit");
    assert!(
        audit.contains(
            "review_deltas::public_export_keys_for(graph, &check.config, &check.workspaces, root)"
        ),
        "audit brief must route public API key selection through review deltas"
    );
    assert!(
        !audit.contains("PackageJson::load"),
        "audit brief must not own public API package metadata reads"
    );
}

#[test]
fn project_info_reuses_session_workspace_metadata() {
    let source = read_source_without_line_comments("crates/api/src/list_runtime.rs")
        .expect("read list runtime");
    assert!(
        source.contains("let workspaces = session.workspaces();"),
        "project info must read workspace metadata from the shared AnalysisSession"
    );
    assert!(
        !source.contains("discover_workspaces(")
            && !source.contains("discover_workspaces_with_diagnostics("),
        "project info must not rediscover workspaces after building an AnalysisSession"
    );
}

#[test]
fn session_backed_api_runtimes_defer_workspace_scope_to_session() {
    for source_path in [
        "crates/api/src/runtime/combined.rs",
        "crates/api/src/runtime/dead_code.rs",
        "crates/api/src/runtime/duplication.rs",
        "crates/api/src/runtime/feature_flags.rs",
        "crates/api/src/runtime/decision_surface.rs",
    ] {
        let source = read_source_without_line_comments(source_path).expect("read runtime source");
        assert!(
            source.contains("resolve_programmatic_analysis_context_deferred_workspace"),
            "{source_path} must defer workspace scope until an AnalysisSession has workspace metadata"
        );
    }

    for source_path in [
        "crates/api/src/runtime/dead_code.rs",
        "crates/api/src/runtime/duplication.rs",
        "crates/api/src/runtime/feature_flags.rs",
        "crates/api/src/runtime/decision_surface.rs",
        "crates/api/src/runtime/mod.rs",
    ] {
        let source = read_source_without_line_comments(source_path).expect("read runtime source");
        assert!(
            source.contains("workspace_roots_for_session("),
            "{source_path} must resolve workspace filters from session.workspaces()"
        );
        assert!(
            !source.contains("resolved.workspace_roots.as_ref()"),
            "{source_path} must not apply eager workspace roots in session-backed analysis"
        );
    }
}

#[test]
fn session_backed_api_next_steps_reuse_session_workspaces() {
    let dead_code = read_source_without_line_comments("crates/api/src/runtime/dead_code.rs")
        .expect("read dead-code runtime source");
    assert!(
        dead_code.contains("default_workspace_ref_for_workspaces(root, session.workspaces())"),
        "dead-code next steps must reuse AnalysisSession workspace metadata"
    );
    assert!(
        !dead_code.contains("default_workspace_ref(root)"),
        "dead-code next steps must not rediscover workspaces after building an AnalysisSession"
    );

    let combined = read_source_without_line_comments("crates/api/src/runtime/combined.rs")
        .expect("read combined runtime source");
    assert!(
        combined.contains("workspaces: Some(session.workspaces().to_vec())")
            && combined.contains("default_workspace_ref_for_workspaces(root, workspaces)"),
        "combined next steps must carry session workspace metadata into shared-session output"
    );
}

#[test]
fn next_step_workspace_ref_probing_routes_through_engine() {
    for source_path in [
        "crates/api/src/next_steps.rs",
        "crates/cli/src/report/suggestions.rs",
    ] {
        let source = read_source_without_line_comments(source_path).expect("read source");
        assert!(
            source.contains("fallow_engine::repo_refs::default_workspace_ref"),
            "{source_path} must use engine-owned repo-ref probing"
        );
        for forbidden in [
            "Command::new(\"git\")",
            "std::process::Command::new(\"git\")",
            "fn git_ref_exists",
            "fn run_git",
            "symbolic-ref",
            "origin/master",
        ] {
            assert!(
                !source.contains(forbidden),
                "{source_path} must not own git-ref probing helper `{forbidden}`"
            );
        }
    }
}

#[test]
fn routing_self_identity_probe_routes_through_engine() {
    let source_path = "crates/api/src/routing.rs";
    let source = read_source_without_line_comments(source_path).expect("read routing source");
    assert!(
        source.contains("fallow_engine::repo_refs::current_user_identities"),
        "routing must use engine-owned git identity probing"
    );
    for forbidden in [
        "Command::new(\"git\")",
        "std::process::Command::new(\"git\")",
        "fn current_user_identities",
        "user.email",
        "user.name",
    ] {
        assert!(
            !source.contains(forbidden),
            "{source_path} must not own git identity probing helper `{forbidden}`"
        );
    }
}

#[test]
fn audit_repo_ref_orchestration_routes_through_engine() {
    let source_path = "crates/api/src/runtime/audit.rs";
    let source = read_source_without_line_comments(source_path).expect("read audit source");
    let production_source = source
        .split("\n#[cfg(test)]")
        .next()
        .expect("audit source before tests");
    assert!(
        production_source.contains("repo_refs::{self, ResolvedAuditBase, TemporaryBaseWorktree}"),
        "audit runtime must use engine-owned repo-ref and base-worktree helpers"
    );
    for forbidden in [
        "Command::new(\"git\")",
        "std::process::Command::new(\"git\")",
        "clear_ambient_git_env",
        "fn git_stdout",
        "fn git_ref_exists",
        "fn git_upstream_ref",
        "fn git_merge_base",
        "fn detect_remote_default_ref",
        "fn get_head_sha",
        "struct BaseWorktree",
    ] {
        assert!(
            !production_source.contains(forbidden),
            "{source_path} must not own audit git orchestration helper `{forbidden}`"
        );
    }

    let decision_surface_path = "crates/api/src/runtime/decision_surface.rs";
    let decision_surface =
        read_source_without_line_comments(decision_surface_path).expect("read decision surface");
    assert!(
        decision_surface.contains("fallow_engine::repo_refs::{self, TemporaryBaseWorktree}"),
        "decision surface must use engine-owned base-worktree helpers"
    );
    assert!(
        !decision_surface.contains("super::audit::BaseWorktree")
            && !decision_surface.contains("super::audit::base_analysis_root"),
        "{decision_surface_path} must not depend on audit-internal base-worktree helpers"
    );
}

#[test]
fn combined_and_audit_share_project_analysis_artifacts() {
    for source_path in [
        "crates/api/src/runtime/combined.rs",
        "crates/api/src/runtime/audit.rs",
    ] {
        let source = read_source_without_line_comments(source_path).expect("read runtime source");
        assert!(
            source.contains("analyze_project_with_artifacts"),
            "{source_path} must reuse one engine-owned project artifact run for shared dead-code and duplication paths"
        );
        assert!(
            source.contains("run_dead_code_from_artifacts")
                && source.contains("run_duplication_report_with_session"),
            "{source_path} must build API outputs from retained artifacts instead of rerunning dead-code or duplication"
        );
        assert!(
            source.contains("health_may_consume_duplication_report")
                && source.contains("project.duplication.clone()")
                && (source.contains("pre_computed_duplication")
                    || source.contains("duplication_artifacts")),
            "{source_path} must pass the already computed project duplication report into health when score or targets need it"
        );
    }
}

#[test]
fn grouped_health_reuses_project_duplication_artifacts() {
    let output_build =
        read_source_without_line_comments("crates/engine/src/health/output_build.rs")
            .expect("read health output build");
    let grouping = read_source_without_line_comments("crates/engine/src/health/grouping.rs")
        .expect("read health grouping");
    assert!(
        output_build.contains("derived_sections.dupes_report.as_ref()"),
        "health grouping must receive the already computed project duplication report"
    );
    assert!(
        grouping.contains("dupes_report: Option<&'a DuplicationReport>"),
        "health grouping must model duplication as an artifact input"
    );
    assert!(
        !grouping.contains("find_duplicates(")
            && !grouping.contains("find_duplicates_cached(")
            && !grouping.contains("duplicates::find_duplicates"),
        "health grouping must not run an additional duplicate analysis per group"
    );
}

#[test]
fn standalone_health_parse_precompute_does_not_fill_session_cache() {
    for source_path in [
        "crates/cli/src/health/mod.rs",
        "crates/engine/src/health/runner.rs",
    ] {
        let source = read_source_without_line_comments(source_path).expect("read health source");
        assert!(
            source.contains("parsed_parts_uncached(true)"),
            "{source_path} must avoid retaining an extra full module vector for one-shot health precompute"
        );
    }
}

#[test]
fn framework_health_reuses_pipeline_workspaces() {
    let framework_health =
        read_source_without_line_comments("crates/engine/src/health/framework_health.rs")
            .expect("read framework health source");
    assert!(
        !framework_health.contains("discover_workspaces("),
        "framework health diagnostics must use HealthPipelineInputs workspaces instead of rediscovering"
    );
    let execute = read_source_without_line_comments("crates/engine/src/health/execute.rs")
        .expect("read health execute source");
    assert!(
        execute.contains("workspaces,") && execute.contains("workspaces: &workspaces"),
        "health execute must thread pipeline workspaces into output assembly"
    );
}

#[test]
fn explain_dead_code_aliases_route_through_issue_registry() {
    let source = read_source_without_line_comments("crates/api/src/explain.rs")
        .expect("read explain source");
    assert!(
        source.contains("issue_meta_for_contract_token"),
        "fallow explain must resolve dead-code tokens through IssueKindMeta"
    );
    assert!(
        source.contains("rule_result_meta(rule).map_or(rule.name, |meta| meta.meta_name)")
            && source.contains(
                "rule_result_meta(rule).map_or(rule.short, |meta| meta.sarif_description)"
            )
            && source.contains(
                "rule_result_meta(rule).map_or(rule.docs_path, |meta| meta.meta_docs_path)"
            ),
        "standalone explain output must derive shared contract fields from IssueResultMeta"
    );
    assert!(
        !source.contains("dead_code_alias_id(") && !source.contains("catalog_alias_id("),
        "dead-code and catalog explain aliases must not be mirrored outside IssueKindMeta"
    );
}

#[test]
fn sarif_rule_descriptions_live_in_issue_result_registry() {
    let source = read_source_without_line_comments("crates/types/src/issue_meta.rs")
        .expect("read issue metadata source");
    assert!(
        source.contains("pub sarif_description: &'static str"),
        "IssueResultMeta must own SARIF short descriptions"
    );
    assert!(
        source.contains("issue_result_meta_by_code(code).map(|meta| meta.sarif_description)"),
        "SARIF rule descriptions must resolve from IssueResultMeta"
    );
    assert!(
        !source.contains("\"unused-file\" => Some(")
            && !source.contains("\"unused-export\" => Some(")
            && !source.contains("\"unresolved-import\" => Some("),
        "SARIF descriptions must not be mirrored in a per-issue match"
    );
}

#[test]
fn typescript_alias_policy_routes_through_issue_registry() {
    let source = read_source_without_line_comments("crates/types/src/issue_meta.rs")
        .expect("read issue metadata source");
    assert!(
        source.contains("pub const ISSUE_TS_ALIAS_META"),
        "TypeScript alias policy must live in registry data"
    );
    assert!(
        source.contains("ISSUE_TS_ALIAS_META")
            && source.contains(".find(|meta| meta.code == code)")
            && source.contains(".map(|meta| meta.alias)"),
        "issue_ts_alias must resolve from ISSUE_TS_ALIAS_META"
    );
    assert!(
        !source.contains("\"unused-file\" => TsAliasMeta")
            && !source.contains("\"unused-export\" => TsAliasMeta")
            && !source.contains("\"unresolved-import\" => TsAliasMeta"),
        "TypeScript aliases must not be mirrored in a per-issue match"
    );
}

#[test]
fn vscode_tree_labels_route_through_generated_issue_registry() {
    let source = std::fs::read_to_string(workspace_root().join("editors/vscode/src/labels.ts"))
        .expect("read vscode labels");
    assert!(
        source.contains("DIAGNOSTIC_CATEGORIES"),
        "VS Code tree labels must read labels from the generated IssueKindMeta surface"
    );
    assert!(
        !source.contains("\"Unused Files\"")
            && !source.contains("\"Unused Exports\"")
            && !source.contains("\"Code Duplication\""),
        "VS Code tree labels must not mirror issue labels as a hand-maintained string table"
    );
}

#[test]
fn lsp_changed_since_scopes_editor_project_analysis_before_duplication() {
    let source =
        read_source_without_line_comments("crates/lsp/src/analysis.rs").expect("read lsp analysis");
    assert!(
        source.contains("resolve_changed_since_scope("),
        "LSP must resolve changedSince before per-root analysis so shared project artifacts can receive the scope"
    );
    assert!(
        source.contains("analyze_project_with_changed_files"),
        "LSP editor analysis must pass changed files into the typed editor API before duplication runs"
    );
    assert!(
        source.contains("analysis.filter_by_changed_files"),
        "LSP must keep the existing post-analysis changedSince filter for dead-code and inline complexity semantics"
    );
}

#[test]
fn engine_session_and_dead_code_route_core_calls_through_backend_adapter() {
    assert_engine_modules_do_not_call_core_directly();
    assert_engine_session_owns_parse_orchestration();
    assert_engine_dead_code_facade_has_no_analysis_bypasses();
    assert_engine_discovery_exposes_session_oriented_surface();
    assert_engine_changed_files_owns_git_orchestration();
}

fn assert_engine_modules_do_not_call_core_directly() {
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
}

fn assert_engine_session_owns_parse_orchestration() {
    let session = read_source_without_line_comments("crates/engine/src/session.rs")
        .expect("read engine session source");
    for forbidden in [
        "analyze_with_usages_from_discovery",
        "analyze_with_usages_and_complexity_from_discovery",
        "analyze_retaining_modules_from_discovery",
        "pub fn analyze_dead_code_from_config",
        "pub fn analyze_dead_code_with_complexity_from_config",
        "pub fn analyze_dead_code_with_artifacts_from_config",
        "pub fn analyze_dead_code_retaining_files_from_config",
        "pub fn analyze_dead_code_with_parse_result_from_config",
    ] {
        assert!(
            !session.contains(forbidden),
            "engine session must own dead-code parse orchestration instead of calling {forbidden}"
        );
    }
}

fn assert_engine_dead_code_facade_has_no_analysis_bypasses() {
    let core_backend = read_source_without_line_comments("crates/engine/src/core_backend.rs")
        .expect("read engine core backend source");
    assert!(
        !core_backend.contains("fallow_core::analyze_with_parse_result"),
        "engine reused-parse analysis must use the engine-owned dead-code phase pipeline"
    );
    assert!(
        !core_backend.contains("fallow_core::analyze::derive_security_severity"),
        "engine security severity policy must stay in fallow-engine, not the core analyze namespace"
    );
    assert!(
        !core_backend.contains("fallow_core::analyze::public_api_package_entry_points"),
        "engine public API entry-point selection must stay in fallow-engine, not the core analyze namespace"
    );
    assert!(
        !core_backend.contains("fallow_core::config_for_project"),
        "engine project config resolution must stay owned by fallow-engine, not the old core monolith"
    );

    let dead_code =
        read_source_without_line_comments("crates/engine/src/dead_code.rs").expect("read source");
    assert!(
        !dead_code.contains("core_backend::analyze_with_parse_result"),
        "engine dead-code facade must not delegate reused-parse analysis to the old core monolith"
    );
    for forbidden in [
        "pub fn analyze(",
        "pub fn analyze_with_usages(",
        "pub fn analyze_with_file_hashes(",
        "pub fn analyze_with_trace(",
        "pub fn analyze_retaining_modules(",
        "pub fn analyze_retaining_files(",
        "pub fn analyze_with_parse_result(",
        "pub fn analyze_with_usages_and_complexity(",
    ] {
        assert!(
            !dead_code.contains(forbidden),
            "engine dead-code facade must not expose direct analysis bypass {forbidden}; use AnalysisSession"
        );
    }

    let cli_dupes =
        read_source_without_line_comments("crates/cli/src/dupes.rs").expect("read cli dupes");
    assert!(
        !cli_dupes.contains("discover_files_with_plugin_scopes"),
        "standalone dupes must use AnalysisSession discovery instead of direct discovery"
    );

    let project_config = read_source_without_line_comments("crates/engine/src/project_config.rs")
        .expect("read engine project config source");
    assert!(
        !project_config.contains("core_backend::config_for_project"),
        "engine project config must not route through the core backend adapter"
    );
}

#[test]
fn engine_owns_trace_without_core_adapter() {
    let core_backend = read_source_without_line_comments("crates/engine/src/core_backend.rs")
        .expect("read engine core backend source");
    for forbidden in [
        "fallow_core::trace::",
        "fallow_core::trace_chain::",
        "pub fn trace_export(",
        "pub fn trace_symbol_chain(",
    ] {
        assert!(
            !core_backend.contains(forbidden),
            "engine trace must stay owned by trace modules instead of core_backend adapter: {forbidden}"
        );
    }

    for source_path in [
        "crates/engine/src/trace.rs",
        "crates/engine/src/trace_impl.rs",
        "crates/engine/src/trace_chain.rs",
        "crates/engine/src/trace_chain_impl.rs",
    ] {
        let source = read_source_without_line_comments(source_path)
            .unwrap_or_else(|error| panic!("read {source_path}: {error}"));
        for forbidden in ["core_backend::trace", "fallow_core::trace"] {
            assert!(
                !source.contains(forbidden),
                "{source_path} must not route trace behavior through {forbidden}"
            );
        }
    }
}

#[test]
fn engine_owns_duplication_pure_helpers_without_core_adapter() {
    let core_backend = read_source_without_line_comments("crates/engine/src/core_backend.rs")
        .expect("read engine core backend source");
    // The core detector copy is deleted, so routing through the old core
    // duplicates module can no longer compile; only the adapter-struct
    // guard remains.
    assert!(
        !core_backend.contains("pub struct BackendCloneFingerprintSet"),
        "duplication detector code must stay engine-owned instead of core_backend adapter: BackendCloneFingerprintSet"
    );

    let duplicates =
        read_source_without_line_comments("crates/engine/src/duplicates.rs").expect("read source");
    for forbidden in [
        "core_backend::clone_fingerprint",
        "core_backend::fingerprint_for_fragment",
        "core_backend::dominant_identifier",
        "core_backend::refresh_clone_families",
        "core_backend::source_token_kinds_equivalent",
        "core_backend::find_duplicates",
        "core_backend::find_duplicates_cached",
        "core_backend::find_duplicates_with_defaults",
        "core_backend::find_duplicates_touching_files_with_defaults",
    ] {
        assert!(
            !duplicates.contains(forbidden),
            "engine duplicates module must not route detector behavior through {forbidden}"
        );
    }

    let detector =
        read_source_without_line_comments("crates/engine/src/duplication_detector/mod.rs")
            .expect("read engine duplication detector source");
    assert!(
        detector.contains("fn tokenize_corpus_for_duplicates")
            && detector.contains("fn detect_and_postprocess")
            && detector.contains("pub fn find_duplicates"),
        "engine duplication detector must own tokenization and clone detection entry points"
    );
}

#[test]
fn core_backend_fallow_core_calls_are_explicitly_allowlisted() {
    let core_backend = read_source_without_line_comments("crates/engine/src/core_backend.rs")
        .expect("read engine core backend source");
    let allowed = [
        "fallow_core::AnalysisDiscovery",
        "fallow_core::DeadCodeBackendPrelude",
        "fallow_core::DeadCodeEntryPoints",
        "fallow_core::prepare_dead_code_backend_prelude",
        "fallow_core::discover_dead_code_entry_points",
        "fallow_core::try_load_dead_code_graph_cache",
        "fallow_core::resolve_dead_code_imports",
        "fallow_core::build_dead_code_graph",
        "fallow_core::run_dead_code_detectors",
        "fallow_core::plugins::AggregatedPluginResult",
        "fallow_core::plugins::PluginRegistry",
        "fallow_core::plugins::manifest_entries",
        "fallow_core::plugins::registry::is_external_plugin_active",
    ];

    for line in core_backend.lines() {
        if !line.contains("fallow_core::") {
            continue;
        }
        assert!(
            allowed.iter().any(|allowed| line.contains(allowed)),
            "unexpected fallow-core adapter dependency in core_backend.rs: {line}"
        );
    }
}

#[test]
fn engine_owns_churn_without_core_adapter() {
    let core_backend = read_source_without_line_comments("crates/engine/src/core_backend.rs")
        .expect("read engine core backend source");
    assert!(
        !core_backend.contains("fallow_core::churn"),
        "engine churn must stay owned by crates/engine/src/churn.rs"
    );

    let churn =
        read_source_without_line_comments("crates/engine/src/churn.rs").expect("read churn source");
    for forbidden in ["core_backend::", "crate::spawn::git", "fallow_core::churn"] {
        assert!(
            !churn.contains(forbidden),
            "engine churn must not route through legacy backend path {forbidden}"
        );
    }
    assert!(
        churn.contains("crate::git_env::clear_ambient_git_env"),
        "engine churn git subprocesses must clear ambient git environment"
    );
}

#[test]
fn engine_guard_owns_policy_scope_matching() {
    let core_backend = read_source_without_line_comments("crates/engine/src/core_backend.rs")
        .expect("read engine core backend source");
    assert!(
        !core_backend.contains("fallow_core::analyze::rules_applying_to_path"),
        "guard policy scope matching must stay owned by fallow-engine"
    );

    let guard =
        read_source_without_line_comments("crates/engine/src/guard.rs").expect("read guard source");
    assert!(
        !guard.contains("core_backend::rules_applying_to_path"),
        "guard report assembly must not call core_backend for policy scope matching"
    );
}

fn assert_engine_discovery_exposes_session_oriented_surface() {
    let engine_discover = read_source_without_line_comments("crates/engine/src/discover.rs")
        .expect("read engine discover");
    for forbidden in [
        "pub fn discover_files(",
        "pub fn discover_files_with_additional_hidden_dirs(",
        "pub fn discover_files_with_plugin_scopes(",
    ] {
        assert!(
            !engine_discover.contains(forbidden),
            "engine discovery must expose session-oriented discovery instead of leftover direct helper {forbidden}"
        );
    }
}

fn assert_engine_changed_files_owns_git_orchestration() {
    let changed_files = read_source_without_line_comments("crates/engine/src/changed_files.rs")
        .expect("read source");
    for forbidden in [
        "core_backend::set_changed_files_spawn_hook",
        "core_backend::validate_git_ref",
        "core_backend::resolve_git_toplevel",
        "core_backend::resolve_git_common_dir",
        "core_backend::try_get_changed_files",
        "core_backend::try_get_changed_diff",
        "core_backend::get_changed_files",
    ] {
        assert!(
            !changed_files.contains(forbidden),
            "engine changed-files git orchestration must be owned by changed_files.rs, not {forbidden}"
        );
    }

    let core_backend = read_source_without_line_comments("crates/engine/src/core_backend.rs")
        .expect("read engine core backend source");
    for forbidden in [
        "fallow_core::changed_files::set_spawn_hook",
        "fallow_core::changed_files::validate_git_ref",
        "fallow_core::changed_files::resolve_git_toplevel",
        "fallow_core::changed_files::resolve_git_common_dir",
        "fallow_core::changed_files::try_get_changed_files",
        "fallow_core::changed_files::try_get_changed_diff",
        "fallow_core::changed_files::get_changed_files",
        "fallow_core::changed_files::filter_results_by_changed_files",
        "fallow_core::changed_files::filter_duplication_by_changed_files",
    ] {
        assert!(
            !core_backend.contains(forbidden),
            "engine core_backend must not re-introduce changed-files orchestration through {forbidden}"
        );
    }

    let core_lib =
        read_source_without_line_comments("crates/core/src/lib.rs").expect("read core lib source");
    assert!(
        !core_lib.contains("pub mod changed_files"),
        "fallow-core must not re-publish changed-file orchestration after it moved to fallow-engine"
    );
}

#[test]
fn core_legacy_orchestration_is_hidden_from_public_docs() {
    let source = std::fs::read_to_string(workspace_root().join("crates/core/src/lib.rs"))
        .expect("read core lib");
    for item in [
        "pub struct AnalysisOutput",
        "pub struct AnalysisParseMetrics",
        "pub struct AnalysisDiscovery",
        "pub struct DeadCodePreludeTimings",
        "pub struct DeadCodeBackendPrelude",
        "pub struct DeadCodeEntryPoints",
        "pub struct DeadCodeResolvedModules",
        "pub struct DeadCodeGraphRun",
        "pub struct DeadCodeDetectorRun",
        "pub fn analyze(",
        "pub fn analyze_with_usages(",
        "pub fn analyze_with_trace(",
        "pub fn analyze_retaining_modules(",
    ] {
        assert_doc_hidden_before(&source, item);
    }
    assert!(
        !source.contains("pub fn config_for_project("),
        "fallow-core config_for_project must stay crate-private now that fallow-engine owns project config resolution"
    );
    assert!(
        !source.contains("pub fn analyze_project("),
        "fallow-core analyze_project must stay crate-private now that fallow-api owns the public project analysis surface"
    );
    assert!(
        !source.contains("pub fn analyze_with_usages_and_complexity("),
        "fallow-core analyze_with_usages_and_complexity must stay removed; LSP now composes typed API and health artifacts"
    );
    assert!(
        !source.contains("pub fn analyze_with_file_hashes("),
        "fallow-core analyze_with_file_hashes must stay removed; fix and CLI callers use AnalysisSession artifacts"
    );
    assert!(
        !source.contains("pub fn analyze_with_parse_result("),
        "fallow-core analyze_with_parse_result must stay removed; pre-parsed reuse stays behind fallow-engine AnalysisSession"
    );
    assert!(
        !source.contains("pub fn public_api_package_entry_points("),
        "fallow-core public_api_package_entry_points must stay private; engine owns the public API entrypoint surface"
    );
    assert!(
        !source.contains("pub use entry_points::resolve_entry_path"),
        "fallow-core resolve_entry_path must not be externally re-exported; engine owns public API entrypoint resolution"
    );
}

#[test]
fn core_legacy_orchestration_wrappers_stay_out_of_production_call_paths() {
    for source_path in rust_sources_under([
        "crates/api/src",
        "crates/cli/src",
        "crates/engine/src",
        "crates/lsp/src",
        "crates/mcp/src",
        "crates/napi/src",
    ]) {
        if source_path == "crates/cli/src/architecture_boundaries.rs" {
            continue;
        }
        let source = read_source_without_line_comments(&source_path)
            .unwrap_or_else(|error| panic!("read {source_path}: {error}"));
        for forbidden in [
            "fallow_core::analyze(",
            "fallow_core::analyze_with_usages(",
            "fallow_core::analyze_with_trace(",
            "fallow_core::analyze_retaining_modules(",
            "fallow_core::analyze_with_parse_result(",
        ] {
            assert!(
                !source.contains(forbidden),
                "{source_path} must not call legacy fallow-core orchestration wrapper {forbidden}"
            );
        }
    }
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
fn mcp_api_routes_honor_ambient_changed_since_scope() {
    for source_path in [
        "crates/mcp/src/tools/analyze.rs",
        "crates/mcp/src/tools/audit.rs",
        "crates/mcp/src/tools/code_mode_tools.rs",
        "crates/mcp/src/tools/decision_surface.rs",
        "crates/mcp/src/tools/dupes.rs",
        "crates/mcp/src/tools/flags.rs",
        "crates/mcp/src/tools/health.rs",
        "crates/mcp/src/tools/list_boundaries.rs",
        "crates/mcp/src/tools/project_info.rs",
        "crates/mcp/src/tools/trace.rs",
    ] {
        let source = read_source_without_line_comments(source_path)
            .unwrap_or_else(|error| panic!("read {source_path}: {error}"));
        assert!(
            source.contains("changed_since_from_param("),
            "{source_path} must apply FALLOW_CHANGED_SINCE when tool params omit changed_since"
        );
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
fn public_core_migration_messages_stay_self_contained() {
    for source_path in rust_sources_under(["crates/core/src", "crates/core/benches"]) {
        let source = read_source_without_line_comments(&source_path)
            .unwrap_or_else(|error| panic!("read {source_path}: {error}"));
        assert!(
            !source.contains("ADR-008"),
            "{source_path} must not require private ADR context for public fallow-core migration messaging"
        );
    }
}

#[test]
fn api_and_cli_workspace_discovery_routes_through_engine() {
    for source_path in rust_sources_under(["crates/api/src", "crates/cli/src"]) {
        if source_path == "crates/cli/src/architecture_boundaries.rs" {
            continue;
        }
        let source = read_source_without_line_comments(&source_path)
            .unwrap_or_else(|error| panic!("read {source_path}: {error}"));
        for forbidden in [
            "fallow_config::discover_workspaces(",
            "fallow_config::discover_workspaces_with_diagnostics(",
            "use fallow_config::{discover_workspaces",
            "use fallow_config::discover_workspaces",
        ] {
            assert!(
                !source.contains(forbidden),
                "{source_path} must route workspace discovery through fallow_engine::discover or AnalysisSession"
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

#[test]
fn api_and_cli_workspace_scope_resolution_routes_through_engine() {
    for source_path in [
        "crates/api/src/analysis_context.rs",
        "crates/cli/src/check/filtering.rs",
    ] {
        let source = read_source_without_line_comments(source_path)
            .unwrap_or_else(|error| panic!("read {source_path}: {error}"));
        assert!(
            source.contains("fallow_engine::workspace_scope"),
            "{source_path} must route workspace-scope resolution through fallow-engine"
        );
        if source_path == "crates/api/src/analysis_context.rs" {
            assert!(
                source.contains("resolve_workspace_scope_roots_for_project"),
                "{source_path} must use the engine-owned project workspace-scope helper"
            );
            assert!(
                !source.contains("discover_workspace_packages(root)"),
                "{source_path} must not rediscover workspaces outside the engine workspace-scope helper"
            );
        }
        for forbidden in [
            "globset::Glob",
            "fn split_workspace_patterns",
            "fn split_patterns",
            "fn find_workspace_matches",
            "fn find_matches",
            "fn match_positive_workspace_patterns",
            "fn match_positive_patterns",
            "fn relative_workspace_path",
            "fn format_available_workspaces",
            "fn workspaces_containing_any",
        ] {
            assert!(
                !source.contains(forbidden),
                "{source_path} must not own workspace-scope matching helper `{forbidden}`"
            );
        }
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

fn assert_doc_hidden_before(source: &str, item: &str) {
    let index = source
        .find(item)
        .unwrap_or_else(|| panic!("expected to find {item}"));
    let prefix = &source[..index];
    let recent_attributes = prefix
        .rsplit_once("\n\n")
        .map_or(prefix, |(_, recent)| recent);
    assert!(
        recent_attributes.contains("#[doc(hidden)]"),
        "{item} must stay hidden from fallow-core rustdoc; expose engine/api wrappers instead"
    );
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

fn workspace_crate_manifests() -> Vec<String> {
    let crates_dir = workspace_root().join("crates");
    let mut manifests = Vec::new();
    for entry in std::fs::read_dir(&crates_dir).expect("read crates directory") {
        let entry = entry.expect("read crates directory entry");
        let path = entry.path();
        if !path.is_dir() || !path.join("Cargo.toml").is_file() {
            continue;
        }
        let name = entry.file_name();
        manifests.push(format!("crates/{}/Cargo.toml", name.to_string_lossy()));
    }
    manifests.sort();
    manifests
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
