use crate::params::*;
use crate::tools::ISSUE_TYPE_FLAGS;

#[test]
fn issue_type_flags_are_complete() {
    assert_eq!(ISSUE_TYPE_FLAGS.len(), 17);
    for &(name, flag) in ISSUE_TYPE_FLAGS {
        assert!(
            flag.starts_with("--"),
            "flag for {name} should start with --"
        );
    }
}

#[test]
fn analyze_params_deserialize() {
    let json = r#"{"root":"/tmp/project","production":true,"issue_types":["unused-files"]}"#;
    let params: AnalyzeParams = serde_json::from_str(json).unwrap();
    assert_eq!(params.root.as_deref(), Some("/tmp/project"));
    assert_eq!(params.production, Some(true));
    assert_eq!(params.issue_types.unwrap(), vec!["unused-files"]);
}

#[test]
fn analyze_params_minimal() {
    let params: AnalyzeParams = serde_json::from_str("{}").unwrap();
    assert!(params.root.is_none());
    assert!(params.production.is_none());
    assert!(params.issue_types.is_none());
    assert!(params.baseline.is_none());
    assert!(params.save_baseline.is_none());
    assert!(params.no_cache.is_none());
    assert!(params.threads.is_none());
}

#[test]
fn analyze_params_with_global_flags() {
    let json = r#"{
        "baseline": "base.json",
        "save_baseline": "new.json",
        "no_cache": true,
        "threads": 4
    }"#;
    let params: AnalyzeParams = serde_json::from_str(json).unwrap();
    assert_eq!(params.baseline.as_deref(), Some("base.json"));
    assert_eq!(params.save_baseline.as_deref(), Some("new.json"));
    assert_eq!(params.no_cache, Some(true));
    assert_eq!(params.threads, Some(4));
}

#[test]
fn check_changed_params_require_since() {
    let json = "{}";
    let result: Result<CheckChangedParams, _> = serde_json::from_str(json);
    assert!(result.is_err());

    let json = r#"{"since":"main"}"#;
    let params: CheckChangedParams = serde_json::from_str(json).unwrap();
    assert_eq!(params.since, "main");
}

#[test]
fn check_runtime_coverage_params_require_coverage() {
    let json = "{}";
    let result: Result<CheckRuntimeCoverageParams, _> = serde_json::from_str(json);
    assert!(result.is_err());

    let json = r#"{"coverage":"./coverage"}"#;
    let params: CheckRuntimeCoverageParams = serde_json::from_str(json).unwrap();
    assert_eq!(params.coverage, "./coverage");
    assert!(params.min_invocations_hot.is_none());
    assert!(params.min_observation_volume.is_none());
    assert!(params.low_traffic_threshold.is_none());
    assert!(params.top.is_none());
    assert!(params.group_by.is_none());
}

#[test]
fn check_runtime_coverage_params_all_fields_deserialize() {
    let json = r#"{
        "coverage": "./coverage/coverage-final.json",
        "root": "/project",
        "config": "fallow.toml",
        "production": true,
        "workspace": "apps/web",
        "min_invocations_hot": 250,
        "min_observation_volume": 7500,
        "low_traffic_threshold": 0.002,
        "no_cache": true,
        "threads": 4,
        "max_crap": 35.0,
        "top": 10,
        "group_by": "owner"
    }"#;
    let params: CheckRuntimeCoverageParams = serde_json::from_str(json).unwrap();
    assert_eq!(params.coverage, "./coverage/coverage-final.json");
    assert_eq!(params.root.as_deref(), Some("/project"));
    assert_eq!(params.config.as_deref(), Some("fallow.toml"));
    assert_eq!(params.production, Some(true));
    assert_eq!(params.workspace.as_deref(), Some("apps/web"));
    assert_eq!(params.min_invocations_hot, Some(250));
    assert_eq!(params.min_observation_volume, Some(7500));
    assert_eq!(params.low_traffic_threshold, Some(0.002));
    assert_eq!(params.no_cache, Some(true));
    assert_eq!(params.threads, Some(4));
    assert_eq!(params.max_crap, Some(35.0));
    assert_eq!(params.top, Some(10));
    assert_eq!(params.group_by.as_deref(), Some("owner"));
}

#[test]
fn find_dupes_params_defaults() {
    let params: FindDupesParams = serde_json::from_str("{}").unwrap();
    assert!(params.mode.is_none());
    assert!(params.min_tokens.is_none());
    assert!(params.skip_local.is_none());
    assert!(params.config.is_none());
    assert!(params.workspace.is_none());
    assert!(params.baseline.is_none());
    assert!(params.no_cache.is_none());
    assert!(params.threads.is_none());
}

#[test]
fn fix_params_with_production() {
    let json = r#"{"root":"/tmp","production":true}"#;
    let params: FixParams = serde_json::from_str(json).unwrap();
    assert_eq!(params.production, Some(true));
}

#[test]
fn fix_params_with_global_flags() {
    let json = r#"{"workspace":"frontend","no_cache":true,"threads":2}"#;
    let params: FixParams = serde_json::from_str(json).unwrap();
    assert_eq!(params.workspace.as_deref(), Some("frontend"));
    assert_eq!(params.no_cache, Some(true));
    assert_eq!(params.threads, Some(2));
}

#[test]
fn health_params_all_fields_deserialize() {
    let json = r#"{
        "root": "/project",
        "config": "fallow.toml",
        "max_cyclomatic": 25,
        "max_cognitive": 30,
        "max_crap": 45.5,
        "top": 10,
        "sort": "cognitive",
        "changed_since": "HEAD~3",
        "baseline": "base.json",
        "save_baseline": "new.json",
        "no_cache": true,
        "threads": 8
    }"#;
    let params: HealthParams = serde_json::from_str(json).unwrap();
    assert_eq!(params.root.as_deref(), Some("/project"));
    assert_eq!(params.config.as_deref(), Some("fallow.toml"));
    assert_eq!(params.max_cyclomatic, Some(25));
    assert_eq!(params.max_cognitive, Some(30));
    assert_eq!(params.max_crap, Some(45.5));
    assert_eq!(params.top, Some(10));
    assert_eq!(params.sort.as_deref(), Some("cognitive"));
    assert_eq!(params.changed_since.as_deref(), Some("HEAD~3"));
    assert_eq!(params.baseline.as_deref(), Some("base.json"));
    assert_eq!(params.save_baseline.as_deref(), Some("new.json"));
    assert_eq!(params.no_cache, Some(true));
    assert_eq!(params.threads, Some(8));
}

#[test]
fn health_params_minimal() {
    let params: HealthParams = serde_json::from_str("{}").unwrap();
    assert!(params.root.is_none());
    assert!(params.config.is_none());
    assert!(params.max_cyclomatic.is_none());
    assert!(params.max_cognitive.is_none());
    assert!(params.max_crap.is_none());
    assert!(params.top.is_none());
    assert!(params.sort.is_none());
    assert!(params.changed_since.is_none());
    assert!(params.baseline.is_none());
    assert!(params.no_cache.is_none());
    assert!(params.threads.is_none());
}

#[test]
fn project_info_params_deserialize() {
    let json = r#"{"root": "/app", "config": ".fallowrc.json"}"#;
    let params: ProjectInfoParams = serde_json::from_str(json).unwrap();
    assert_eq!(params.root.as_deref(), Some("/app"));
    assert_eq!(params.config.as_deref(), Some(".fallowrc.json"));
}

#[test]
fn project_info_params_with_global_flags() {
    let json = r#"{"no_cache": true, "threads": 4}"#;
    let params: ProjectInfoParams = serde_json::from_str(json).unwrap();
    assert_eq!(params.no_cache, Some(true));
    assert_eq!(params.threads, Some(4));
}

#[test]
fn trace_export_params_require_file_and_export_name() {
    let json = "{}";
    let result: Result<TraceExportParams, _> = serde_json::from_str(json);
    assert!(result.is_err());

    let json = r#"{"file":"src/utils.ts","export_name":"usedFunction"}"#;
    let params: TraceExportParams = serde_json::from_str(json).unwrap();
    assert_eq!(params.file, "src/utils.ts");
    assert_eq!(params.export_name, "usedFunction");
    assert!(params.root.is_none());
}

#[test]
fn trace_file_params_require_file() {
    let json = "{}";
    let result: Result<TraceFileParams, _> = serde_json::from_str(json);
    assert!(result.is_err());

    let json = r#"{"file":"src/utils.ts","production":true,"workspace":"apps/web"}"#;
    let params: TraceFileParams = serde_json::from_str(json).unwrap();
    assert_eq!(params.file, "src/utils.ts");
    assert_eq!(params.production, Some(true));
    assert_eq!(params.workspace.as_deref(), Some("apps/web"));
}

#[test]
fn trace_dependency_params_require_package_name() {
    let json = "{}";
    let result: Result<TraceDependencyParams, _> = serde_json::from_str(json);
    assert!(result.is_err());

    let json = r#"{"package_name":"react","root":"/repo"}"#;
    let params: TraceDependencyParams = serde_json::from_str(json).unwrap();
    assert_eq!(params.package_name, "react");
    assert_eq!(params.root.as_deref(), Some("/repo"));
}

#[test]
fn trace_clone_params_require_file_and_line() {
    let json = "{}";
    let result: Result<TraceCloneParams, _> = serde_json::from_str(json);
    assert!(result.is_err());

    let json = r#"{
        "file": "src/original.ts",
        "line": 2,
        "mode": "semantic",
        "min_tokens": 80,
        "skip_local": true
    }"#;
    let params: TraceCloneParams = serde_json::from_str(json).unwrap();
    assert_eq!(params.file, "src/original.ts");
    assert_eq!(params.line, 2);
    assert_eq!(params.mode.as_deref(), Some("semantic"));
    assert_eq!(params.min_tokens, Some(80));
    assert_eq!(params.skip_local, Some(true));
}

#[test]
fn find_dupes_params_all_fields_deserialize() {
    let json = r#"{
        "root": "/project",
        "config": "fallow.toml",
        "workspace": "@my/lib",
        "mode": "strict",
        "min_tokens": 100,
        "min_lines": 10,
        "threshold": 5.5,
        "skip_local": true,
        "top": 5,
        "baseline": "base.json",
        "save_baseline": "new.json",
        "no_cache": true,
        "threads": 4
    }"#;
    let params: FindDupesParams = serde_json::from_str(json).unwrap();
    assert_eq!(params.root.as_deref(), Some("/project"));
    assert_eq!(params.config.as_deref(), Some("fallow.toml"));
    assert_eq!(params.workspace.as_deref(), Some("@my/lib"));
    assert_eq!(params.mode.as_deref(), Some("strict"));
    assert_eq!(params.min_tokens, Some(100));
    assert_eq!(params.min_lines, Some(10));
    assert_eq!(params.threshold, Some(5.5));
    assert_eq!(params.skip_local, Some(true));
    assert_eq!(params.top, Some(5));
    assert_eq!(params.baseline.as_deref(), Some("base.json"));
    assert_eq!(params.save_baseline.as_deref(), Some("new.json"));
    assert_eq!(params.no_cache, Some(true));
    assert_eq!(params.threads, Some(4));
}

#[test]
fn check_changed_params_all_fields_deserialize() {
    let json = r#"{
        "root": "/app",
        "since": "develop",
        "config": "custom.toml",
        "production": true,
        "workspace": "frontend",
        "baseline": "base.json",
        "save_baseline": "new.json",
        "no_cache": true,
        "threads": 2
    }"#;
    let params: CheckChangedParams = serde_json::from_str(json).unwrap();
    assert_eq!(params.root.as_deref(), Some("/app"));
    assert_eq!(params.since, "develop");
    assert_eq!(params.config.as_deref(), Some("custom.toml"));
    assert_eq!(params.production, Some(true));
    assert_eq!(params.workspace.as_deref(), Some("frontend"));
    assert_eq!(params.baseline.as_deref(), Some("base.json"));
    assert_eq!(params.save_baseline.as_deref(), Some("new.json"));
    assert_eq!(params.no_cache, Some(true));
    assert_eq!(params.threads, Some(2));
}

#[test]
fn fix_params_minimal_deserialize() {
    let params: FixParams = serde_json::from_str("{}").unwrap();
    assert!(params.root.is_none());
    assert!(params.config.is_none());
    assert!(params.production.is_none());
    assert!(params.workspace.is_none());
    assert!(params.no_cache.is_none());
    assert!(params.threads.is_none());
}

#[test]
fn project_info_params_minimal_deserialize() {
    let params: ProjectInfoParams = serde_json::from_str("{}").unwrap();
    assert!(params.root.is_none());
    assert!(params.config.is_none());
    assert!(params.no_cache.is_none());
    assert!(params.threads.is_none());
}

#[test]
fn find_dupes_params_with_cross_language_deserialize() {
    let json = r#"{"cross_language": true}"#;
    let params: FindDupesParams = serde_json::from_str(json).unwrap();
    assert_eq!(params.cross_language, Some(true));
}

#[test]
fn health_params_all_boolean_section_flags_deserialize() {
    let json = r#"{
        "complexity": true,
        "file_scores": true,
        "hotspots": true,
        "since": "6m",
        "min_commits": 3,
        "workspace": "ui",
        "production": true
    }"#;
    let params: HealthParams = serde_json::from_str(json).unwrap();
    assert_eq!(params.complexity, Some(true));
    assert_eq!(params.file_scores, Some(true));
    assert_eq!(params.hotspots, Some(true));
    assert_eq!(params.since.as_deref(), Some("6m"));
    assert_eq!(params.min_commits, Some(3));
    assert_eq!(params.workspace.as_deref(), Some("ui"));
    assert_eq!(params.production, Some(true));
}

// ── HealthParams: targets and save_snapshot deserialization ────────

#[test]
fn health_params_targets_deserialize() {
    let json = r#"{"targets": true}"#;
    let params: HealthParams = serde_json::from_str(json).unwrap();
    assert_eq!(params.targets, Some(true));
}

#[test]
fn health_params_targets_false_deserialize() {
    let json = r#"{"targets": false}"#;
    let params: HealthParams = serde_json::from_str(json).unwrap();
    assert_eq!(params.targets, Some(false));
}

#[test]
fn health_params_save_snapshot_with_path_deserialize() {
    let json = r#"{"save_snapshot": "snapshots/v1.json"}"#;
    let params: HealthParams = serde_json::from_str(json).unwrap();
    assert_eq!(params.save_snapshot.as_deref(), Some("snapshots/v1.json"));
}

#[test]
fn health_params_save_snapshot_empty_string_deserialize() {
    let json = r#"{"save_snapshot": ""}"#;
    let params: HealthParams = serde_json::from_str(json).unwrap();
    assert_eq!(params.save_snapshot.as_deref(), Some(""));
}

#[test]
fn health_params_missing_save_snapshot_is_none() {
    let params: HealthParams = serde_json::from_str("{}").unwrap();
    assert!(params.save_snapshot.is_none());
    assert!(params.targets.is_none());
}

// ── AnalyzeParams: unknown fields are ignored ─────────────────────

#[test]
fn analyze_params_ignores_unknown_fields() {
    let json = r#"{"root": "/app", "unknown_field": 42}"#;
    // serde default behavior: unknown fields are ignored (no deny_unknown_fields)
    let params: AnalyzeParams = serde_json::from_str(json).unwrap();
    assert_eq!(params.root.as_deref(), Some("/app"));
}

// ── CheckChangedParams: empty since string is accepted ────────────

#[test]
fn check_changed_params_empty_since_string() {
    let json = r#"{"since": ""}"#;
    let params: CheckChangedParams = serde_json::from_str(json).unwrap();
    assert_eq!(params.since, "");
}

// ── FindDupesParams: cross_language deserialization ────────────────

#[test]
fn find_dupes_params_cross_language_false_deserialize() {
    let json = r#"{"cross_language": false}"#;
    let params: FindDupesParams = serde_json::from_str(json).unwrap();
    assert_eq!(params.cross_language, Some(false));
}

// ── FindDupesParams: ignore_imports deserialization ──────────────

#[test]
fn find_dupes_params_ignore_imports_true_deserialize() {
    let json = r#"{"ignore_imports": true}"#;
    let params: FindDupesParams = serde_json::from_str(json).unwrap();
    assert_eq!(params.ignore_imports, Some(true));
}

#[test]
fn find_dupes_params_ignore_imports_false_deserialize() {
    let json = r#"{"ignore_imports": false}"#;
    let params: FindDupesParams = serde_json::from_str(json).unwrap();
    assert_eq!(params.ignore_imports, Some(false));
}

// ── FixParams: all fields deserialize ─────────────────────────────

#[test]
fn fix_params_all_fields_deserialize() {
    let json = r#"{
        "root": "/project",
        "config": "custom.toml",
        "production": true,
        "workspace": "@scope/pkg",
        "no_cache": true,
        "threads": 8
    }"#;
    let params: FixParams = serde_json::from_str(json).unwrap();
    assert_eq!(params.root.as_deref(), Some("/project"));
    assert_eq!(params.config.as_deref(), Some("custom.toml"));
    assert_eq!(params.production, Some(true));
    assert_eq!(params.workspace.as_deref(), Some("@scope/pkg"));
    assert_eq!(params.no_cache, Some(true));
    assert_eq!(params.threads, Some(8));
}

// ── HealthParams: full deserialization including new fields ────────

#[test]
fn health_params_all_fields_including_new_deserialize() {
    let json = r#"{
        "root": "/project",
        "config": "fallow.toml",
        "max_cyclomatic": 25,
        "max_cognitive": 30,
        "top": 10,
        "sort": "cognitive",
        "changed_since": "HEAD~3",
        "complexity": true,
        "file_scores": true,
        "hotspots": true,
        "targets": true,
        "since": "6m",
        "min_commits": 5,
        "workspace": "ui",
        "production": true,
        "save_snapshot": "snap.json",
        "baseline": "base.json",
        "save_baseline": "new.json",
        "no_cache": true,
        "threads": 8
    }"#;
    let params: HealthParams = serde_json::from_str(json).unwrap();
    assert_eq!(params.targets, Some(true));
    assert_eq!(params.save_snapshot.as_deref(), Some("snap.json"));
    assert_eq!(params.complexity, Some(true));
    assert_eq!(params.file_scores, Some(true));
    assert_eq!(params.hotspots, Some(true));
    assert_eq!(params.since.as_deref(), Some("6m"));
    assert_eq!(params.min_commits, Some(5));
}

// ── AnalyzeParams: issue_types with unicode values ────────────────

#[test]
fn analyze_params_unicode_values_deserialize() {
    let json = r#"{"root": "/home/ユーザー", "workspace": "パッケージ"}"#;
    let params: AnalyzeParams = serde_json::from_str(json).unwrap();
    assert_eq!(params.root.as_deref(), Some("/home/ユーザー"));
    assert_eq!(params.workspace.as_deref(), Some("パッケージ"));
}

// ── FindDupesParams: threshold edge values ────────────────────────

#[test]
fn find_dupes_params_threshold_zero_deserialize() {
    let json = r#"{"threshold": 0.0}"#;
    let params: FindDupesParams = serde_json::from_str(json).unwrap();
    assert_eq!(params.threshold, Some(0.0));
}

#[test]
fn find_dupes_params_threshold_negative_deserialize() {
    let json = r#"{"threshold": -1.5}"#;
    let params: FindDupesParams = serde_json::from_str(json).unwrap();
    assert_eq!(params.threshold, Some(-1.5));
}

#[test]
fn find_dupes_params_threshold_large_deserialize() {
    let json = r#"{"threshold": 100.0}"#;
    let params: FindDupesParams = serde_json::from_str(json).unwrap();
    assert_eq!(params.threshold, Some(100.0));
}

// ── HealthParams: threads boundary values ─────────────────────────

#[test]
fn health_params_threads_zero_deserialize() {
    let json = r#"{"threads": 0}"#;
    let params: HealthParams = serde_json::from_str(json).unwrap();
    assert_eq!(params.threads, Some(0));
}

#[test]
fn health_params_threads_large_deserialize() {
    let json = r#"{"threads": 1024}"#;
    let params: HealthParams = serde_json::from_str(json).unwrap();
    assert_eq!(params.threads, Some(1024));
}

// ── CheckChangedParams: unicode in since ref ──────────────────────

#[test]
fn check_changed_params_unicode_since() {
    let json = r#"{"since": "feature/日本語-branch"}"#;
    let params: CheckChangedParams = serde_json::from_str(json).unwrap();
    assert_eq!(params.since, "feature/日本語-branch");
}

// ── HealthParams: save_snapshot with unicode path ─────────────────

#[test]
fn health_params_save_snapshot_unicode_path_deserialize() {
    let json = r#"{"save_snapshot": "/home/ユーザー/スナップ.json"}"#;
    let params: HealthParams = serde_json::from_str(json).unwrap();
    assert_eq!(
        params.save_snapshot.as_deref(),
        Some("/home/ユーザー/スナップ.json")
    );
}

// ── FindDupesParams: min_tokens/min_lines boundary values ─────────

#[test]
fn find_dupes_params_min_tokens_zero_deserialize() {
    let json = r#"{"min_tokens": 0}"#;
    let params: FindDupesParams = serde_json::from_str(json).unwrap();
    assert_eq!(params.min_tokens, Some(0));
}

#[test]
fn find_dupes_params_min_lines_max_deserialize() {
    let json = r#"{"min_lines": 4294967295}"#;
    let params: FindDupesParams = serde_json::from_str(json).unwrap();
    assert_eq!(params.min_lines, Some(u32::MAX));
}

// ── HealthParams: max_cyclomatic/max_cognitive boundary values ─────

#[test]
fn health_params_complexity_thresholds_boundary_deserialize() {
    let json = r#"{"max_cyclomatic": 0, "max_cognitive": 0}"#;
    let params: HealthParams = serde_json::from_str(json).unwrap();
    assert_eq!(params.max_cyclomatic, Some(0));
    assert_eq!(params.max_cognitive, Some(0));
}

// ── FixParams: ignores unknown fields ─────────────────────────────

#[test]
fn fix_params_ignores_unknown_fields() {
    let json = r#"{"root": "/app", "extra_field": true}"#;
    let params: FixParams = serde_json::from_str(json).unwrap();
    assert_eq!(params.root.as_deref(), Some("/app"));
}

// ── ProjectInfoParams: ignores unknown fields ─────────────────────

#[test]
fn project_info_params_ignores_unknown_fields() {
    let json = r#"{"root": "/app", "verbose": true}"#;
    let params: ProjectInfoParams = serde_json::from_str(json).unwrap();
    assert_eq!(params.root.as_deref(), Some("/app"));
}

// ── AuditParams ─────────────────────────────────────────────────

#[test]
fn audit_params_deserialize() {
    let json =
        r#"{"root":"/tmp/project","base":"main","production":true,"production_health":true}"#;
    let params: AuditParams = serde_json::from_str(json).unwrap();
    assert_eq!(params.root.as_deref(), Some("/tmp/project"));
    assert_eq!(params.base.as_deref(), Some("main"));
    assert_eq!(params.production, Some(true));
    assert_eq!(params.production_health, Some(true));
}

#[test]
fn audit_params_minimal() {
    let params: AuditParams = serde_json::from_str("{}").unwrap();
    assert!(params.root.is_none());
    assert!(params.base.is_none());
    assert!(params.production.is_none());
    assert!(params.production_dead_code.is_none());
    assert!(params.production_health.is_none());
    assert!(params.production_dupes.is_none());
    assert!(params.workspace.is_none());
    assert!(params.no_cache.is_none());
    assert!(params.threads.is_none());
}

#[test]
fn audit_params_with_all_fields() {
    let json = r#"{
        "root": "/project",
        "config": ".fallowrc.json",
        "base": "develop",
        "production": true,
        "production_dead_code": true,
        "production_health": true,
        "production_dupes": true,
        "workspace": "@app/core",
        "no_cache": true,
        "threads": 8,
        "coverage": "coverage/coverage-final.json",
        "coverage_root": "/ci/build"
    }"#;
    let params: AuditParams = serde_json::from_str(json).unwrap();
    assert_eq!(params.root.as_deref(), Some("/project"));
    assert_eq!(params.config.as_deref(), Some(".fallowrc.json"));
    assert_eq!(params.base.as_deref(), Some("develop"));
    assert_eq!(params.production, Some(true));
    assert_eq!(params.production_dead_code, Some(true));
    assert_eq!(params.production_health, Some(true));
    assert_eq!(params.production_dupes, Some(true));
    assert_eq!(params.workspace.as_deref(), Some("@app/core"));
    assert_eq!(params.no_cache, Some(true));
    assert_eq!(params.threads, Some(8));
    assert_eq!(
        params.coverage.as_deref(),
        Some("coverage/coverage-final.json")
    );
    assert_eq!(params.coverage_root.as_deref(), Some("/ci/build"));
}

// ── ListBoundariesParams ────────────────────────────────────────

#[test]
fn list_boundaries_params_minimal() {
    let params: ListBoundariesParams = serde_json::from_str("{}").unwrap();
    assert!(params.root.is_none());
    assert!(params.config.is_none());
    assert!(params.no_cache.is_none());
    assert!(params.threads.is_none());
}

#[test]
fn list_boundaries_params_full() {
    let json = r#"{
        "root": "/project",
        "config": ".fallowrc.json",
        "no_cache": true,
        "threads": 4
    }"#;
    let params: ListBoundariesParams = serde_json::from_str(json).unwrap();
    assert_eq!(params.root.as_deref(), Some("/project"));
    assert_eq!(params.config.as_deref(), Some(".fallowrc.json"));
    assert_eq!(params.no_cache, Some(true));
    assert_eq!(params.threads, Some(4));
}

// ── FindDupesParams: changed_since deserialization ───────────────

#[test]
fn find_dupes_params_changed_since_deserialize() {
    let json = r#"{"changed_since": "main"}"#;
    let params: FindDupesParams = serde_json::from_str(json).unwrap();
    assert_eq!(params.changed_since.as_deref(), Some("main"));
}

#[test]
fn find_dupes_params_changed_since_missing_is_none() {
    let params: FindDupesParams = serde_json::from_str("{}").unwrap();
    assert!(params.changed_since.is_none());
}

// ── AnalyzeParams: boundary_violations deserialization ───────────

#[test]
fn analyze_params_boundary_violations_true_deserialize() {
    let json = r#"{"boundary_violations": true}"#;
    let params: AnalyzeParams = serde_json::from_str(json).unwrap();
    assert_eq!(params.boundary_violations, Some(true));
}

#[test]
fn analyze_params_boundary_violations_missing_is_none() {
    let params: AnalyzeParams = serde_json::from_str("{}").unwrap();
    assert!(params.boundary_violations.is_none());
}

// ── ProjectInfoParams: section flags deserialization ─────────────

#[test]
fn project_info_params_section_flags_deserialize() {
    let json = r#"{
        "entry_points": true,
        "files": true,
        "plugins": true,
        "boundaries": true
    }"#;
    let params: ProjectInfoParams = serde_json::from_str(json).unwrap();
    assert_eq!(params.entry_points, Some(true));
    assert_eq!(params.files, Some(true));
    assert_eq!(params.plugins, Some(true));
    assert_eq!(params.boundaries, Some(true));
}

#[test]
fn project_info_params_section_flags_missing_are_none() {
    let params: ProjectInfoParams = serde_json::from_str("{}").unwrap();
    assert!(params.entry_points.is_none());
    assert!(params.files.is_none());
    assert!(params.plugins.is_none());
    assert!(params.boundaries.is_none());
}
