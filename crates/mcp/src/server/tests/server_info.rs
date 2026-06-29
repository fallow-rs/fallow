use std::collections::BTreeMap;
use std::sync::LazyLock;

use regex::Regex;
use rmcp::ServerHandler;

use super::super::FallowMcp;

#[test]
fn server_info_is_correct() {
    let server = FallowMcp::new();
    let info = ServerHandler::get_info(&server);
    assert_eq!(info.server_info.name, "fallow-mcp");
    assert_eq!(info.server_info.version, env!("CARGO_PKG_VERSION"));
    assert!(info.capabilities.tools.is_some());
    assert!(info.instructions.is_some());
}

#[test]
fn all_tools_registered() {
    let server = FallowMcp::new();
    let tools = server.tool_router.list_all();
    let names: Vec<String> = tools.iter().map(|t| t.name.to_string()).collect();
    assert!(names.contains(&"code_execute".to_string()));
    assert!(names.contains(&"analyze".to_string()));
    assert!(names.contains(&"check_changed".to_string()));
    assert!(names.contains(&"security_candidates".to_string()));
    assert!(names.contains(&"inspect_target".to_string()));
    assert!(names.contains(&"find_dupes".to_string()));
    assert!(names.contains(&"fix_preview".to_string()));
    assert!(names.contains(&"fix_apply".to_string()));
    assert!(names.contains(&"project_info".to_string()));
    assert!(names.contains(&"trace_export".to_string()));
    assert!(names.contains(&"trace_file".to_string()));
    assert!(names.contains(&"trace_dependency".to_string()));
    assert!(names.contains(&"trace_clone".to_string()));
    assert!(names.contains(&"check_health".to_string()));
    assert!(names.contains(&"audit".to_string()));
    assert!(names.contains(&"fallow_explain".to_string()));
    assert!(names.contains(&"list_boundaries".to_string()));
    assert!(names.contains(&"feature_flags".to_string()));
    assert!(names.contains(&"check_runtime_coverage".to_string()));
    assert!(names.contains(&"get_hot_paths".to_string()));
    assert!(names.contains(&"get_blast_radius".to_string()));
    assert!(names.contains(&"get_importance".to_string()));
    assert!(names.contains(&"get_cleanup_candidates".to_string()));
    assert!(names.contains(&"get_token_blast_radius".to_string()));
    assert!(names.contains(&"impact".to_string()));
    assert!(names.contains(&"impact_all".to_string()));
    assert!(names.contains(&"decision_surface".to_string()));
    assert_eq!(tools.len(), 27);
}

#[test]
fn read_only_tools_have_annotations() {
    let server = FallowMcp::new();
    let tools = server.tool_router.list_all();
    let read_only = [
        "code_execute",
        "analyze",
        "check_changed",
        "security_candidates",
        "inspect_target",
        "find_dupes",
        "fix_preview",
        "project_info",
        "trace_export",
        "trace_file",
        "trace_dependency",
        "trace_clone",
        "check_health",
        "audit",
        "decision_surface",
        "fallow_explain",
        "list_boundaries",
        "feature_flags",
        "check_runtime_coverage",
        "get_hot_paths",
        "get_blast_radius",
        "get_importance",
        "get_cleanup_candidates",
        "get_token_blast_radius",
        "impact",
        "impact_all",
    ];
    for tool in &tools {
        let name = tool.name.to_string();
        if read_only.contains(&name.as_str()) {
            let ann = tool.annotations.as_ref().expect("annotations");
            assert_eq!(ann.read_only_hint, Some(true), "{name} should be read-only");
        }
    }
}

#[test]
fn fix_apply_is_destructive() {
    let server = FallowMcp::new();
    let tools = server.tool_router.list_all();
    let fix = tools.iter().find(|t| t.name == "fix_apply").unwrap();
    let ann = fix.annotations.as_ref().unwrap();
    assert_eq!(ann.destructive_hint, Some(true));
    assert_eq!(ann.read_only_hint, Some(false));
}

#[test]
fn all_tools_have_descriptions() {
    let server = FallowMcp::new();
    let tools = server.tool_router.list_all();
    for tool in &tools {
        let name = tool.name.to_string();
        let desc = tool.description.as_deref().unwrap_or("");
        assert!(
            !desc.is_empty(),
            "tool '{name}' should have a non-empty description"
        );
    }
}

#[test]
fn all_tools_have_annotations() {
    let server = FallowMcp::new();
    let tools = server.tool_router.list_all();
    for tool in &tools {
        let name = tool.name.to_string();
        assert!(
            tool.annotations.is_some(),
            "tool '{name}' should have annotations"
        );
    }
}

#[test]
fn open_world_hint_on_analysis_tools() {
    let server = FallowMcp::new();
    let tools = server.tool_router.list_all();
    let open_world = [
        "code_execute",
        "analyze",
        "check_changed",
        "security_candidates",
        "inspect_target",
        "find_dupes",
        "fix_preview",
        "project_info",
        "trace_export",
        "trace_file",
        "trace_dependency",
        "trace_clone",
        "check_health",
        "audit",
        "decision_surface",
        "list_boundaries",
        "feature_flags",
        "check_runtime_coverage",
        "impact_all",
    ];
    for tool in &tools {
        let name = tool.name.to_string();
        if open_world.contains(&name.as_str()) {
            let ann = tool.annotations.as_ref().unwrap();
            assert_eq!(
                ann.open_world_hint,
                Some(true),
                "{name} should have open_world_hint=true"
            );
        }
    }
}

#[test]
fn impact_is_read_only_closed_world() {
    let server = FallowMcp::new();
    let tools = server.tool_router.list_all();
    let impact = tools.iter().find(|t| t.name == "impact").unwrap();
    let ann = impact.annotations.as_ref().unwrap();
    assert_eq!(ann.read_only_hint, Some(true));
    assert_eq!(ann.open_world_hint, Some(false));
    assert_eq!(ann.idempotent_hint, Some(true));
}

#[test]
fn impact_all_is_read_only_open_world_idempotent() {
    // The load-bearing distinction from single-repo `impact`: the cross-repo
    // roll-up's result set varies with the machine's tracked repos, so it is
    // OPEN-world while staying read-only and idempotent. A regression that
    // dropped any of these hints would otherwise pass CI.
    let server = FallowMcp::new();
    let tools = server.tool_router.list_all();
    let impact_all = tools.iter().find(|t| t.name == "impact_all").unwrap();
    let ann = impact_all.annotations.as_ref().unwrap();
    assert_eq!(ann.read_only_hint, Some(true));
    assert_eq!(ann.open_world_hint, Some(true));
    assert_eq!(ann.idempotent_hint, Some(true));
}

#[test]
fn fix_preview_is_not_destructive() {
    let server = FallowMcp::new();
    let tools = server.tool_router.list_all();
    let preview = tools.iter().find(|t| t.name == "fix_preview").unwrap();
    let ann = preview.annotations.as_ref().unwrap();
    assert_eq!(ann.read_only_hint, Some(true));
    assert_ne!(ann.destructive_hint, Some(true));
}

#[test]
fn server_info_has_description() {
    let server = FallowMcp::new();
    let info = ServerHandler::get_info(&server);
    assert!(
        info.server_info
            .description
            .as_ref()
            .is_some_and(|d| !d.is_empty()),
        "server info should have a description"
    );
}

#[test]
fn server_instructions_mention_all_tools() {
    let server = FallowMcp::new();
    let info = ServerHandler::get_info(&server);
    let instructions = info.instructions.as_deref().unwrap();
    assert!(instructions.contains("code_execute"));
    assert!(instructions.contains("analyze"));
    assert!(instructions.contains("check_changed"));
    assert!(instructions.contains("security_candidates"));
    assert!(instructions.contains("inspect_target"));
    assert!(instructions.contains("find_dupes"));
    assert!(instructions.contains("fix_preview"));
    assert!(instructions.contains("fix_apply"));
    assert!(instructions.contains("project_info"));
    assert!(instructions.contains("trace_export"));
    assert!(instructions.contains("trace_file"));
    assert!(instructions.contains("trace_dependency"));
    assert!(instructions.contains("trace_clone"));
    assert!(instructions.contains("check_health"));
    assert!(instructions.contains("audit"));
    assert!(instructions.contains("decision_surface"));
    assert!(instructions.contains("fallow_explain"));
    assert!(instructions.contains("list_boundaries"));
    assert!(instructions.contains("feature_flags"));
    assert!(instructions.contains("check_runtime_coverage"));
    assert!(instructions.contains("get_hot_paths"));
    assert!(instructions.contains("get_blast_radius"));
    assert!(instructions.contains("get_importance"));
    assert!(instructions.contains("get_cleanup_candidates"));
}

#[test]
fn all_tools_have_input_schema() {
    let server = FallowMcp::new();
    let tools = server.tool_router.list_all();
    for tool in &tools {
        let name = tool.name.to_string();
        assert!(
            !tool.input_schema.is_empty(),
            "tool '{name}' should have a non-empty input_schema"
        );
    }
}

#[test]
fn code_execute_schema_contains_expected_properties() {
    let server = FallowMcp::new();
    let tools = server.tool_router.list_all();
    let tool = tools.iter().find(|t| t.name == "code_execute").unwrap();
    let schema = serde_json::to_string(&tool.input_schema).unwrap();
    for prop in ["code", "root", "timeout_ms", "max_output_bytes"] {
        assert!(
            schema.contains(prop),
            "code_execute schema should contain property '{prop}'"
        );
    }
    let schema: serde_json::Value = serde_json::to_value(&tool.input_schema).unwrap();
    assert_required_fields(&schema, &["code"]);
}

#[test]
fn analyze_schema_contains_expected_properties() {
    let server = FallowMcp::new();
    let tools = server.tool_router.list_all();
    let tool = tools.iter().find(|t| t.name == "analyze").unwrap();
    let schema = serde_json::to_string(&tool.input_schema).unwrap();
    for prop in [
        "root",
        "config",
        "production",
        "workspace",
        "issue_types",
        "boundary_violations",
        "baseline",
        "save_baseline",
        "no_cache",
        "threads",
    ] {
        assert!(
            schema.contains(prop),
            "analyze schema should contain property '{prop}'"
        );
    }
}

#[test]
fn check_changed_schema_contains_expected_properties() {
    let server = FallowMcp::new();
    let tools = server.tool_router.list_all();
    let tool = tools.iter().find(|t| t.name == "check_changed").unwrap();
    let schema = serde_json::to_string(&tool.input_schema).unwrap();
    for prop in [
        "root",
        "since",
        "config",
        "production",
        "workspace",
        "baseline",
        "save_baseline",
        "no_cache",
        "threads",
    ] {
        assert!(
            schema.contains(prop),
            "check_changed schema should contain property '{prop}'"
        );
    }
}

#[test]
fn check_changed_schema_requires_since() {
    let server = FallowMcp::new();
    let tools = server.tool_router.list_all();
    let tool = tools.iter().find(|t| t.name == "check_changed").unwrap();
    let schema = serde_json::to_string(&tool.input_schema).unwrap();
    assert!(
        schema.contains("\"required\""),
        "check_changed schema should have a required array"
    );
    let schema_value: serde_json::Value = serde_json::from_str(&schema).unwrap();
    if let Some(required) = schema_value.get("required").and_then(|r| r.as_array()) {
        assert!(
            required.iter().any(|v| v.as_str() == Some("since")),
            "check_changed schema should require 'since'"
        );
    }
}

#[test]
fn security_candidates_schema_contains_expected_properties() {
    let server = FallowMcp::new();
    let tools = server.tool_router.list_all();
    let tool = tools
        .iter()
        .find(|t| t.name == "security_candidates")
        .unwrap();
    let schema = serde_json::to_string(&tool.input_schema).unwrap();
    for prop in [
        "root",
        "config",
        "workspace",
        "changed_since",
        "paths",
        "changed_workspaces",
        "surface",
        "gate",
        "no_cache",
        "threads",
    ] {
        assert!(
            schema.contains(prop),
            "security_candidates schema should contain property '{prop}'"
        );
    }
    for inert in [
        "ci",
        "fail_on_issues",
        "sarif_file",
        "summary",
        "baseline",
        "save_baseline",
    ] {
        assert!(
            !schema.contains(inert),
            "security_candidates must not expose inert or mutating property '{inert}'"
        );
    }
}

#[test]
fn security_candidates_description_frames_candidates_and_scope() {
    let server = FallowMcp::new();
    let tools = server.tool_router.list_all();
    let tool = tools
        .iter()
        .find(|t| t.name == "security_candidates")
        .unwrap();
    let desc = tool.description.as_deref().unwrap();
    assert!(
        desc.starts_with("Returns unverified security candidates, not confirmed vulnerabilities."),
        "security_candidates description must lead with candidate framing, got {desc}"
    );
    for expected in [
        "fallow security --format json",
        "kind: \"security\"",
        "security_findings",
        "category",
        "CWE",
        "severity",
        "evidence",
        "structural trace",
        "taint_confidence",
        "Verify trace, reachability context, severity, and evidence",
        "paths",
        "changed_since",
        "changed_workspaces",
        "gate",
        "newly-reachable",
        "attack_surface",
        "FALLOW_DIFF_FILE",
        "FALLOW_TIMEOUT_SECS",
    ] {
        assert!(
            desc.contains(expected),
            "security_candidates description should mention {expected}"
        );
    }
}

#[test]
fn inspect_target_schema_contains_expected_properties() {
    let server = FallowMcp::new();
    let tools = server.tool_router.list_all();
    let tool = tools.iter().find(|t| t.name == "inspect_target").unwrap();
    let schema = serde_json::to_string(&tool.input_schema).unwrap();
    for prop in [
        "target",
        "type",
        "file",
        "export_name",
        "root",
        "config",
        "production",
        "workspace",
        "no_cache",
        "threads",
    ] {
        assert!(
            schema.contains(prop),
            "inspect_target schema should contain property '{prop}'"
        );
    }
    let schema: serde_json::Value = serde_json::to_value(&tool.input_schema).unwrap();
    assert_required_fields(&schema, &["target"]);
}

#[test]
fn inspect_target_description_frames_scope_and_timeout() {
    let server = FallowMcp::new();
    let tools = server.tool_router.list_all();
    let tool = tools.iter().find(|t| t.name == "inspect_target").unwrap();
    let desc = tool.description.as_deref().unwrap();
    for expected in [
        "one typed evidence bundle",
        "target={type:\"file\"",
        "target={type:\"symbol\"",
        "trace_file",
        "trace_export",
        "file-scoped",
        "FALLOW_TIMEOUT_SECS",
    ] {
        assert!(
            desc.contains(expected),
            "inspect_target description should mention {expected}"
        );
    }
}

#[test]
fn find_dupes_schema_contains_expected_properties() {
    let server = FallowMcp::new();
    let tools = server.tool_router.list_all();
    let tool = tools.iter().find(|t| t.name == "find_dupes").unwrap();
    let schema = serde_json::to_string(&tool.input_schema).unwrap();
    for prop in [
        "root",
        "config",
        "workspace",
        "mode",
        "min_tokens",
        "min_lines",
        "threshold",
        "skip_local",
        "cross_language",
        "ignore_imports",
        "explain_skipped",
        "top",
        "baseline",
        "save_baseline",
        "no_cache",
        "threads",
        "changed_since",
    ] {
        assert!(
            schema.contains(prop),
            "find_dupes schema should contain property '{prop}'"
        );
    }
}

#[test]
fn fix_preview_schema_contains_expected_properties() {
    let server = FallowMcp::new();
    let tools = server.tool_router.list_all();
    let tool = tools.iter().find(|t| t.name == "fix_preview").unwrap();
    let schema = serde_json::to_string(&tool.input_schema).unwrap();
    for prop in [
        "root",
        "config",
        "production",
        "workspace",
        "no_cache",
        "threads",
    ] {
        assert!(
            schema.contains(prop),
            "fix_preview schema should contain property '{prop}'"
        );
    }
}

#[test]
fn fix_apply_schema_contains_expected_properties() {
    let server = FallowMcp::new();
    let tools = server.tool_router.list_all();
    let tool = tools.iter().find(|t| t.name == "fix_apply").unwrap();
    let schema = serde_json::to_string(&tool.input_schema).unwrap();
    for prop in [
        "root",
        "config",
        "production",
        "workspace",
        "no_cache",
        "threads",
    ] {
        assert!(
            schema.contains(prop),
            "fix_apply schema should contain property '{prop}'"
        );
    }
}

#[test]
fn project_info_schema_contains_expected_properties() {
    let server = FallowMcp::new();
    let tools = server.tool_router.list_all();
    let tool = tools.iter().find(|t| t.name == "project_info").unwrap();
    let schema = serde_json::to_string(&tool.input_schema).unwrap();
    for prop in [
        "root",
        "config",
        "entry_points",
        "files",
        "plugins",
        "boundaries",
        "no_cache",
        "threads",
    ] {
        assert!(
            schema.contains(prop),
            "project_info schema should contain property '{prop}'"
        );
    }
}

#[test]
fn trace_export_schema_contains_expected_properties() {
    let server = FallowMcp::new();
    let tools = server.tool_router.list_all();
    let tool = tools.iter().find(|t| t.name == "trace_export").unwrap();
    let schema = serde_json::to_string(&tool.input_schema).unwrap();
    for prop in [
        "file",
        "export_name",
        "root",
        "config",
        "production",
        "workspace",
        "no_cache",
        "threads",
    ] {
        assert!(
            schema.contains(prop),
            "trace_export schema should contain property '{prop}'"
        );
    }
    let schema: serde_json::Value = serde_json::to_value(&tool.input_schema).unwrap();
    assert_required_fields(&schema, &["file", "export_name"]);
    assert_eq!(
        schema
            .pointer("/properties/file/minLength")
            .and_then(|v| v.as_u64()),
        Some(1)
    );
    assert_eq!(
        schema
            .pointer("/properties/export_name/minLength")
            .and_then(|v| v.as_u64()),
        Some(1)
    );
}

#[test]
fn trace_file_schema_contains_expected_properties() {
    let server = FallowMcp::new();
    let tools = server.tool_router.list_all();
    let tool = tools.iter().find(|t| t.name == "trace_file").unwrap();
    let schema = serde_json::to_string(&tool.input_schema).unwrap();
    for prop in [
        "file",
        "root",
        "config",
        "production",
        "workspace",
        "no_cache",
        "threads",
    ] {
        assert!(
            schema.contains(prop),
            "trace_file schema should contain property '{prop}'"
        );
    }
    let schema: serde_json::Value = serde_json::to_value(&tool.input_schema).unwrap();
    assert_required_fields(&schema, &["file"]);
    assert_eq!(
        schema
            .pointer("/properties/file/minLength")
            .and_then(|v| v.as_u64()),
        Some(1)
    );
}

#[test]
fn trace_dependency_schema_contains_expected_properties() {
    let server = FallowMcp::new();
    let tools = server.tool_router.list_all();
    let tool = tools.iter().find(|t| t.name == "trace_dependency").unwrap();
    let schema = serde_json::to_string(&tool.input_schema).unwrap();
    for prop in [
        "package_name",
        "root",
        "config",
        "production",
        "workspace",
        "no_cache",
        "threads",
    ] {
        assert!(
            schema.contains(prop),
            "trace_dependency schema should contain property '{prop}'"
        );
    }
    let schema: serde_json::Value = serde_json::to_value(&tool.input_schema).unwrap();
    assert_required_fields(&schema, &["package_name"]);
    assert_eq!(
        schema
            .pointer("/properties/package_name/minLength")
            .and_then(|v| v.as_u64()),
        Some(1)
    );
}

#[test]
fn trace_clone_schema_contains_expected_properties() {
    let server = FallowMcp::new();
    let tools = server.tool_router.list_all();
    let tool = tools.iter().find(|t| t.name == "trace_clone").unwrap();
    let schema = serde_json::to_string(&tool.input_schema).unwrap();
    for prop in [
        "file",
        "line",
        "fingerprint",
        "root",
        "config",
        "workspace",
        "mode",
        "min_tokens",
        "min_lines",
        "threshold",
        "skip_local",
        "cross_language",
        "ignore_imports",
        "no_cache",
        "threads",
    ] {
        assert!(
            schema.contains(prop),
            "trace_clone schema should contain property '{prop}'"
        );
    }
    let schema: serde_json::Value = serde_json::to_value(&tool.input_schema).unwrap();
    let required: Vec<&str> = schema
        .get("required")
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().filter_map(|v| v.as_str()).collect())
        .unwrap_or_default();
    assert!(
        !required.contains(&"file") && !required.contains(&"line"),
        "file/line must be optional now, got required: {required:?}"
    );
}

fn assert_required_fields(schema: &serde_json::Value, expected: &[&str]) {
    let required = schema
        .get("required")
        .and_then(|v| v.as_array())
        .expect("schema should have required fields");
    for field in expected {
        assert!(
            required.iter().any(|v| v.as_str() == Some(field)),
            "schema should require {field}, got {required:?}"
        );
    }
}

#[test]
fn check_health_schema_contains_expected_properties() {
    let server = FallowMcp::new();
    let tools = server.tool_router.list_all();
    let tool = tools.iter().find(|t| t.name == "check_health").unwrap();
    let schema = serde_json::to_string(&tool.input_schema).unwrap();
    for prop in [
        "root",
        "config",
        "max_cyclomatic",
        "max_cognitive",
        "max_crap",
        "top",
        "sort",
        "changed_since",
        "complexity",
        "file_scores",
        "hotspots",
        "targets",
        "since",
        "min_commits",
        "churn_file",
        "workspace",
        "production",
        "save_snapshot",
        "baseline",
        "save_baseline",
        "no_cache",
        "threads",
        "runtime_coverage",
        "min_invocations_hot",
        "min_observation_volume",
        "low_traffic_threshold",
    ] {
        assert!(
            schema.contains(prop),
            "check_health schema should contain property '{prop}'"
        );
    }
}

#[test]
fn check_health_description_mentions_runtime_coverage() {
    let server = FallowMcp::new();
    let tools = server.tool_router.list_all();
    let tool = tools.iter().find(|t| t.name == "check_health").unwrap();
    let desc = tool.description.as_deref().unwrap();
    assert!(
        desc.contains("runtime_coverage"),
        "check_health description should mention runtime_coverage (paid feature wiring)"
    );
    assert!(
        desc.contains("min_invocations_hot"),
        "check_health description should mention min_invocations_hot tuning knob"
    );
    assert!(
        desc.contains("fallow license"),
        "check_health description should reference `fallow license activate` as the activation path"
    );
}

#[test]
fn audit_schema_contains_expected_properties() {
    let server = FallowMcp::new();
    let tools = server.tool_router.list_all();
    let tool = tools.iter().find(|t| t.name == "audit").unwrap();
    let schema = serde_json::to_string(&tool.input_schema).unwrap();
    for prop in [
        "root",
        "config",
        "base",
        "production",
        "workspace",
        "no_cache",
        "threads",
        "gate",
        "max_crap",
        "coverage",
        "coverage_root",
    ] {
        assert!(
            schema.contains(prop),
            "audit schema should contain property '{prop}'"
        );
    }
}

#[test]
fn decision_surface_schema_contains_expected_properties() {
    let server = FallowMcp::new();
    let tools = server.tool_router.list_all();
    let tool = tools.iter().find(|t| t.name == "decision_surface").unwrap();
    let schema = serde_json::to_string(&tool.input_schema).unwrap();
    for prop in [
        "root",
        "config",
        "base",
        "max_decisions",
        "workspace",
        "no_cache",
        "threads",
    ] {
        assert!(
            schema.contains(prop),
            "decision_surface schema should contain property '{prop}'"
        );
    }
}

#[test]
fn decision_surface_description_frames_solid_3_cap_and_anchoring() {
    let server = FallowMcp::new();
    let tools = server.tool_router.list_all();
    let tool = tools.iter().find(|t| t.name == "decision_surface").unwrap();
    let desc = tool.description.as_deref().unwrap();
    for expected in [
        "decision-surface",
        "signal_id",
        "REJECTED",
        "SOLID-3",
        "coupling-boundary",
        "public-api-contract",
        "dependency",
        "fallow-ignore",
        "exits 0",
    ] {
        assert!(
            desc.contains(expected),
            "decision_surface description should mention {expected}"
        );
    }
}

#[test]
fn decision_surface_is_read_only_open_world() {
    let server = FallowMcp::new();
    let tools = server.tool_router.list_all();
    let tool = tools.iter().find(|t| t.name == "decision_surface").unwrap();
    let ann = tool.annotations.as_ref().unwrap();
    assert_eq!(ann.read_only_hint, Some(true));
    assert_eq!(ann.open_world_hint, Some(true));
}

#[test]
fn impact_schema_contains_root_and_omits_inert_flags() {
    let server = FallowMcp::new();
    let tools = server.tool_router.list_all();
    let tool = tools.iter().find(|t| t.name == "impact").unwrap();
    let schema = serde_json::to_value(&tool.input_schema).unwrap();
    let props = schema
        .get("properties")
        .and_then(|p| p.as_object())
        .expect("impact schema has a properties object");
    assert!(props.contains_key("root"), "impact exposes 'root'");
    for inert in ["config", "no_cache", "threads"] {
        assert!(
            !props.contains_key(inert),
            "impact must NOT expose inert property '{inert}'"
        );
    }
}

#[test]
fn list_boundaries_schema_contains_expected_properties() {
    let server = FallowMcp::new();
    let tools = server.tool_router.list_all();
    let tool = tools.iter().find(|t| t.name == "list_boundaries").unwrap();
    let schema = serde_json::to_string(&tool.input_schema).unwrap();
    for prop in ["root", "config", "no_cache", "threads"] {
        assert!(
            schema.contains(prop),
            "list_boundaries schema should contain property '{prop}'"
        );
    }
}

/// Pins that the fields whose descriptions were migrated from
/// `#[schemars(description = ...)]` to `///` doc comments still surface a
/// non-empty description in the published schema. A future drift here would
/// drop user-visible prose from `tools/list`.
#[test]
fn converted_field_descriptions_render_in_schema() {
    let server = FallowMcp::new();
    let tools = server.tool_router.list_all();

    let cases: &[(&str, &[&str])] = &[
        (
            "project_info",
            &["entry_points", "files", "plugins", "boundaries"],
        ),
        (
            "list_boundaries",
            &["root", "config", "no_cache", "threads"],
        ),
        ("analyze", &["boundary_violations"]),
        ("find_dupes", &["changed_since"]),
    ];

    for (tool_name, fields) in cases {
        let tool = tools.iter().find(|t| t.name == *tool_name).unwrap();
        let schema: serde_json::Value = serde_json::to_value(&tool.input_schema).unwrap();
        for field in *fields {
            let desc = schema
                .pointer(&format!("/properties/{field}/description"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            assert!(
                !desc.is_empty(),
                "{tool_name}.{field} should have a non-empty description in the schema"
            );
        }
    }
}

#[derive(Clone, Copy)]
struct ToolDefaultExpectation {
    tool: &'static str,
    param: &'static str,
}

static CLAP_DEFAULT_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"default_value_t\s*=\s*([0-9]+(?:\.[0-9]+)?)").unwrap());

static DOC_DEFAULT_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\b(?:spec\s+default|default)\s*\(?([0-9]+(?:\.[0-9]+)?)\)?").unwrap()
});

static DESCRIPTION_DEFAULT_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\bdefault\s*\(?([0-9]+(?:\.[0-9]+)?)\)?").unwrap());

const RUNTIME_DEFAULT_EXPECTATIONS: &[ToolDefaultExpectation] = &[
    ToolDefaultExpectation {
        tool: "check_health",
        param: "min_invocations_hot",
    },
    ToolDefaultExpectation {
        tool: "check_health",
        param: "min_observation_volume",
    },
    ToolDefaultExpectation {
        tool: "check_health",
        param: "low_traffic_threshold",
    },
    ToolDefaultExpectation {
        tool: "check_runtime_coverage",
        param: "min_invocations_hot",
    },
    ToolDefaultExpectation {
        tool: "check_runtime_coverage",
        param: "min_observation_volume",
    },
    ToolDefaultExpectation {
        tool: "check_runtime_coverage",
        param: "low_traffic_threshold",
    },
    ToolDefaultExpectation {
        tool: "audit",
        param: "min_invocations_hot",
    },
];

fn mcp_tool_descriptions() -> BTreeMap<String, String> {
    let server = FallowMcp::new();
    server
        .tool_router
        .list_all()
        .iter()
        .map(|tool| {
            (
                tool.name.to_string(),
                tool.description.as_deref().unwrap_or("").to_owned(),
            )
        })
        .collect()
}

fn default_drift_reports(
    descriptions: &BTreeMap<String, String>,
    cli_src: &str,
    expectations: &[ToolDefaultExpectation],
) -> Vec<String> {
    let mut reports = Vec::new();

    for expectation in expectations {
        let Some(cli_default) = cli_default_for_param(cli_src, expectation.param) else {
            reports.push(format!(
                "{}.{}: CLI-side default not found",
                expectation.tool, expectation.param
            ));
            continue;
        };

        match descriptions
            .get(expectation.tool)
            .and_then(|description| description_default_for_param(description, expectation.param))
        {
            Some(description_default) if description_default == cli_default => {}
            Some(description_default) => reports.push(format!(
                "{}.{} default mismatch: MCP description states {}, CLI source states {}",
                expectation.tool, expectation.param, description_default, cli_default
            )),
            None => reports.push(format!(
                "{}.{} default missing from MCP description; CLI source states {}",
                expectation.tool, expectation.param, cli_default
            )),
        }
    }

    reports
}

fn cli_default_for_param(cli_src: &str, param: &str) -> Option<String> {
    let lines: Vec<_> = cli_src.lines().collect();
    let needle = format!("{param}:");

    for (idx, line) in lines.iter().enumerate() {
        if !line.contains(&needle) {
            continue;
        }

        let mut context_lines = Vec::new();
        for candidate in lines[..idx].iter().rev() {
            let trimmed = candidate.trim_start();
            if trimmed.starts_with("///") || trimmed.starts_with("#[") {
                context_lines.push(*candidate);
                continue;
            }
            break;
        }
        context_lines.reverse();
        context_lines.push(*line);
        let context = context_lines.join(" ");
        if let Some(captures) = CLAP_DEFAULT_RE.captures(&context) {
            return Some(captures[1].to_owned());
        }
        if let Some(captures) = DOC_DEFAULT_RE.captures(&context) {
            return Some(captures[1].to_owned());
        }
    }

    None
}

fn description_default_for_param(description: &str, param: &str) -> Option<String> {
    let start = description.find(param)?;
    let window: String = description[start..].chars().take(220).collect();
    DESCRIPTION_DEFAULT_RE
        .captures(&window)
        .map(|captures| captures[1].to_owned())
}

#[test]
fn mcp_tool_description_defaults_match_cli_defaults() {
    let descriptions = mcp_tool_descriptions();
    let reports = default_drift_reports(
        &descriptions,
        include_str!("../../../../cli/src/main.rs"),
        RUNTIME_DEFAULT_EXPECTATIONS,
    );

    assert!(
        reports.is_empty(),
        "MCP tool description default drift:\n{}",
        reports.join("\n")
    );
}

#[test]
fn default_drift_gate_trips_on_changed_or_missing_tool_description_default() {
    let mut descriptions = BTreeMap::new();
    descriptions.insert(
        "check_health".to_string(),
        "Runtime tuning: min_invocations_hot default 101; low_traffic_threshold default 0.001."
            .to_string(),
    );

    let reports = default_drift_reports(
        &descriptions,
        include_str!("../../../../cli/src/main.rs"),
        &[
            ToolDefaultExpectation {
                tool: "check_health",
                param: "min_invocations_hot",
            },
            ToolDefaultExpectation {
                tool: "check_health",
                param: "min_observation_volume",
            },
        ],
    );

    assert!(
        reports.iter().any(|report| {
            report.contains("check_health.min_invocations_hot")
                && report.contains("101")
                && report.contains("100")
        }),
        "changed defaults should produce a diff-friendly report: {reports:?}"
    );
    assert!(
        reports.iter().any(|report| {
            report.contains("check_health.min_observation_volume")
                && report.contains("missing")
                && report.contains("5000")
        }),
        "missing defaults should produce a diff-friendly report: {reports:?}"
    );
}

#[test]
fn check_runtime_coverage_schema_contains_expected_properties() {
    let server = FallowMcp::new();
    let tools = server.tool_router.list_all();
    let tool = tools
        .iter()
        .find(|t| t.name == "check_runtime_coverage")
        .unwrap();
    let schema = serde_json::to_string(&tool.input_schema).unwrap();
    for prop in [
        "coverage",
        "root",
        "config",
        "production",
        "workspace",
        "min_invocations_hot",
        "min_observation_volume",
        "low_traffic_threshold",
        "no_cache",
        "threads",
        "max_crap",
        "top",
        "group_by",
    ] {
        assert!(
            schema.contains(prop),
            "check_runtime_coverage schema should contain property '{prop}'"
        );
    }
}

#[test]
fn runtime_context_split_tool_schemas_require_coverage() {
    let server = FallowMcp::new();
    let tools = server.tool_router.list_all();
    for name in [
        "get_hot_paths",
        "get_blast_radius",
        "get_importance",
        "get_cleanup_candidates",
    ] {
        let tool = tools.iter().find(|t| t.name == name).unwrap();
        let schema = serde_json::to_string(&tool.input_schema).unwrap();
        assert!(
            schema.contains("coverage"),
            "{name} schema should contain coverage"
        );
        assert!(schema.contains("top"), "{name} schema should contain top");
        let schema_value: serde_json::Value = serde_json::from_str(&schema).unwrap();
        let required = schema_value
            .get("required")
            .and_then(|r| r.as_array())
            .expect("runtime context schema should have a required array");
        assert!(
            required.iter().any(|v| v.as_str() == Some("coverage")),
            "{name} schema should require coverage"
        );
    }
}

#[test]
fn check_runtime_coverage_schema_requires_coverage() {
    let server = FallowMcp::new();
    let tools = server.tool_router.list_all();
    let tool = tools
        .iter()
        .find(|t| t.name == "check_runtime_coverage")
        .unwrap();
    let schema = serde_json::to_string(&tool.input_schema).unwrap();
    let schema_value: serde_json::Value = serde_json::from_str(&schema).unwrap();
    let required = schema_value
        .get("required")
        .and_then(|r| r.as_array())
        .expect("check_runtime_coverage schema should have a required array");
    assert!(
        required.iter().any(|v| v.as_str() == Some("coverage")),
        "check_runtime_coverage schema should require 'coverage'"
    );
}

#[test]
fn feature_flags_schema_contains_expected_properties() {
    let server = FallowMcp::new();
    let tools = server.tool_router.list_all();
    let tool = tools.iter().find(|t| t.name == "feature_flags").unwrap();
    let schema = serde_json::to_string(&tool.input_schema).unwrap();
    for prop in [
        "root",
        "config",
        "production",
        "workspace",
        "flag_type",
        "confidence",
        "no_cache",
        "threads",
    ] {
        assert!(
            schema.contains(prop),
            "feature_flags schema should contain property '{prop}'"
        );
    }
}

#[test]
fn fix_apply_does_not_have_open_world_hint() {
    let server = FallowMcp::new();
    let tools = server.tool_router.list_all();
    let fix = tools.iter().find(|t| t.name == "fix_apply").unwrap();
    let ann = fix.annotations.as_ref().unwrap();
    assert_ne!(
        ann.open_world_hint,
        Some(true),
        "fix_apply should not have open_world_hint=true"
    );
}

#[test]
fn analyze_description_mentions_unused_code() {
    let server = FallowMcp::new();
    let tools = server.tool_router.list_all();
    let tool = tools.iter().find(|t| t.name == "analyze").unwrap();
    let desc = tool.description.as_deref().unwrap();
    assert!(
        desc.contains("unused"),
        "analyze description should mention 'unused'"
    );
}

#[test]
fn find_dupes_description_mentions_duplication() {
    let server = FallowMcp::new();
    let tools = server.tool_router.list_all();
    let tool = tools.iter().find(|t| t.name == "find_dupes").unwrap();
    let desc = tool.description.as_deref().unwrap();
    assert!(
        desc.contains("duplic"),
        "find_dupes description should mention duplication"
    );
}

#[test]
fn check_health_description_mentions_complexity() {
    let server = FallowMcp::new();
    let tools = server.tool_router.list_all();
    let tool = tools.iter().find(|t| t.name == "check_health").unwrap();
    let desc = tool.description.as_deref().unwrap();
    assert!(
        desc.contains("complexity"),
        "check_health description should mention 'complexity'"
    );
}

#[test]
fn check_health_description_mentions_config_activated_coverage_gaps() {
    let server = FallowMcp::new();
    let tools = server.tool_router.list_all();
    let tool = tools.iter().find(|t| t.name == "check_health").unwrap();
    let desc = tool.description.as_deref().unwrap();
    assert!(
        desc.contains("rules.coverage-gaps") || desc.contains("config file may also enable"),
        "check_health description should explain config-activated coverage gaps"
    );
}

#[test]
fn fix_apply_description_warns_about_modification() {
    let server = FallowMcp::new();
    let tools = server.tool_router.list_all();
    let tool = tools.iter().find(|t| t.name == "fix_apply").unwrap();
    let desc = tool.description.as_deref().unwrap();
    assert!(
        desc.contains("modif") || desc.contains("disk") || desc.contains("destructi"),
        "fix_apply description should warn about file modification"
    );
}

#[test]
fn fix_preview_description_mentions_dry_run_or_preview() {
    let server = FallowMcp::new();
    let tools = server.tool_router.list_all();
    let tool = tools.iter().find(|t| t.name == "fix_preview").unwrap();
    let desc = tool.description.as_deref().unwrap();
    assert!(
        desc.contains("preview") || desc.contains("dry") || desc.contains("without modif"),
        "fix_preview description should mention preview/dry-run behavior"
    );
}

#[test]
fn all_tool_schemas_are_json_objects() {
    let server = FallowMcp::new();
    let tools = server.tool_router.list_all();
    for tool in &tools {
        let name = tool.name.to_string();
        let schema_str = serde_json::to_string(&tool.input_schema).unwrap();
        let schema_value: serde_json::Value = serde_json::from_str(&schema_str).unwrap();
        assert!(
            schema_value.is_object(),
            "tool '{name}' schema should be a JSON object"
        );
        assert_eq!(
            schema_value.get("type").and_then(|t| t.as_str()),
            Some("object"),
            "tool '{name}' schema should have type=object"
        );
    }
}

/// Returns the 1-based line numbers of any field that carries BOTH a `///`
/// doc comment AND a `#[schemars(description = ...)]` attribute (single or
/// multi-line). The explicit attribute wins, so when both forms co-occur the
/// doc comment silently fails to reach the schema.
fn fields_with_both_doc_and_schemars_description(src: &str) -> Vec<usize> {
    let lines: Vec<&str> = src.lines().collect();
    let mut offenders: Vec<usize> = Vec::new();

    for (i, line) in lines.iter().enumerate() {
        let trimmed = line.trim_start();
        if !trimmed.starts_with("#[schemars(") {
            continue;
        }
        let mut full = String::new();
        let mut depth: i32 = 0;
        let mut j = i;
        loop {
            full.push_str(lines[j]);
            full.push(' ');
            for c in lines[j].chars() {
                match c {
                    '(' => depth += 1,
                    ')' => depth -= 1,
                    _ => {}
                }
            }
            if depth <= 0 || j + 1 >= lines.len() {
                break;
            }
            j += 1;
        }
        if !full.contains("description") {
            continue;
        }

        let mut has_doc = false;
        let mut k = i;
        while k > 0 {
            k -= 1;
            let prev = lines[k].trim();
            if prev.is_empty() {
                break;
            }
            if prev.starts_with("///") {
                has_doc = true;
                break;
            }
            if prev.starts_with("pub ") || prev.starts_with("pub(") {
                break;
            }
            if prev.starts_with('{') || prev.starts_with('}') {
                break;
            }
        }

        if has_doc {
            offenders.push(i + 1);
        }
    }
    offenders
}

/// Drift gate: every param struct field uses EITHER a `///` doc comment OR a
/// `#[schemars(description = "...")]` attribute, never both.
#[test]
fn params_fields_do_not_carry_both_doc_comment_and_schemars_description() {
    let src = include_str!("../../params.rs");
    let offenders = fields_with_both_doc_and_schemars_description(src);
    assert!(
        offenders.is_empty(),
        "params.rs has fields carrying BOTH a `///` doc comment AND a \
         `#[schemars(description = ...)]` attribute. The explicit attribute \
         wins and rustdoc edits silently fail to reach the schema. Drop one \
         of the two forms. Offending lines: {offenders:?}"
    );
}

/// Positive-case gate test: synthetic source with the bad pattern is flagged.
/// Without this test, a future refactor to the gate logic could silently turn
/// it into a no-op and the CI would happily pass forever.
#[test]
fn gate_trips_on_combined_doc_and_schemars_description() {
    let good = r#"
pub struct A {
    /// Plain doc only.
    pub a: Option<bool>,

    #[schemars(description = "Attr only")]
    pub b: Option<bool>,
}
"#;
    assert!(
        fields_with_both_doc_and_schemars_description(good).is_empty(),
        "good source should not trip the gate"
    );

    let bad_single_line = r#"
pub struct A {
    /// Doc says X.
    #[schemars(description = "Attr says Y")]
    pub a: Option<bool>,
}
"#;
    assert!(
        !fields_with_both_doc_and_schemars_description(bad_single_line).is_empty(),
        "bad source (single-line attr) should trip the gate"
    );

    let bad_multi_line = r#"
pub struct A {
    /// Doc says X.
    #[schemars(
        description = "Attr says Y"
    )]
    pub a: Option<bool>,
}
"#;
    assert!(
        !fields_with_both_doc_and_schemars_description(bad_multi_line).is_empty(),
        "bad source (multi-line attr) should trip the gate"
    );

    let benign_non_description = r"
pub struct A {
    /// Doc says X.
    #[schemars(length(min = 1))]
    pub a: String,
}
";
    assert!(
        fields_with_both_doc_and_schemars_description(benign_non_description).is_empty(),
        "schemars(length/range/...) without description should not trip the gate"
    );
}

#[test]
fn server_is_cloneable() {
    let server = FallowMcp::new();
    let cloned = server.clone();
    let tools_original = server.tool_router.list_all();
    let tools_cloned = cloned.tool_router.list_all();
    assert_eq!(tools_original.len(), tools_cloned.len());
}
