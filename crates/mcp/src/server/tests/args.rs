use crate::params::*;
use crate::tools::{
    ISSUE_TYPE_FLAGS, VALID_DUPES_MODES, build_analyze_args, build_audit_args,
    build_check_changed_args, build_check_production_coverage_args, build_feature_flags_args,
    build_find_dupes_args, build_fix_apply_args, build_fix_preview_args, build_health_args,
    build_list_boundaries_args, build_project_info_args, build_trace_clone_args,
    build_trace_dependency_args, build_trace_export_args, build_trace_file_args,
};

/// Parse a validation error body into its `message` field. Arg builders emit
/// structured JSON (`{"error": true, "message": "...", "exit_code": 0}`) so the
/// handler can forward it verbatim to MCP clients; tests decode it here.
fn parse_validation_message(err: &str) -> String {
    let v: serde_json::Value = serde_json::from_str(err)
        .unwrap_or_else(|e| panic!("validation error should be JSON, got `{err}`: {e}"));
    assert_eq!(
        v["error"].as_bool(),
        Some(true),
        "expected error=true in {err}"
    );
    assert_eq!(
        v["exit_code"].as_i64(),
        Some(0),
        "expected exit_code=0 in {err}"
    );
    v["message"]
        .as_str()
        .unwrap_or_else(|| panic!("expected string message in {err}"))
        .to_string()
}

fn check_production_coverage(coverage: &str) -> CheckProductionCoverageParams {
    CheckProductionCoverageParams {
        coverage: coverage.to_string(),
        root: None,
        config: None,
        production: None,
        workspace: None,
        min_invocations_hot: None,
        min_observation_volume: None,
        low_traffic_threshold: None,
        no_cache: None,
        threads: None,
        max_crap: None,
        group_by: None,
    }
}

// ── Helper: minimal CheckChangedParams ────────────────────────────

fn check_changed(since: &str) -> CheckChangedParams {
    CheckChangedParams {
        root: None,
        since: since.to_string(),
        config: None,
        production: None,
        workspace: None,
        baseline: None,
        save_baseline: None,
        fail_on_regression: None,
        tolerance: None,
        regression_baseline: None,
        save_regression_baseline: None,
        include_entry_exports: None,
        no_cache: None,
        threads: None,
    }
}

// ── Argument building: analyze ────────────────────────────────────

#[test]
fn analyze_args_minimal_produces_base_args() {
    let args = build_analyze_args(&AnalyzeParams::default()).unwrap();
    assert_eq!(
        args,
        ["dead-code", "--format", "json", "--quiet", "--explain"]
    );
}

#[test]
fn analyze_args_with_all_options() {
    let params = AnalyzeParams {
        root: Some("/my/project".to_string()),
        config: Some("fallow.toml".to_string()),
        production: Some(true),
        workspace: Some("@my/pkg".to_string()),
        issue_types: Some(vec![
            "unused-files".to_string(),
            "unused-exports".to_string(),
        ]),
        boundary_violations: None,
        baseline: Some("baseline.json".to_string()),
        save_baseline: Some("new-baseline.json".to_string()),
        fail_on_regression: Some(true),
        tolerance: Some("2%".to_string()),
        regression_baseline: Some("reg.json".to_string()),
        save_regression_baseline: Some("new-reg.json".to_string()),
        group_by: Some("owner".to_string()),
        file: None,
        include_entry_exports: None,
        no_cache: Some(true),
        threads: Some(4),
    };
    let args = build_analyze_args(&params).unwrap();
    assert_eq!(
        args,
        [
            "dead-code",
            "--format",
            "json",
            "--quiet",
            "--explain",
            "--root",
            "/my/project",
            "--config",
            "fallow.toml",
            "--no-cache",
            "--threads",
            "4",
            "--production",
            "--workspace",
            "@my/pkg",
            "--unused-files",
            "--unused-exports",
            "--baseline",
            "baseline.json",
            "--save-baseline",
            "new-baseline.json",
            "--fail-on-regression",
            "--tolerance",
            "2%",
            "--regression-baseline",
            "reg.json",
            "--save-regression-baseline",
            "new-reg.json",
            "--group-by",
            "owner",
        ]
    );
}

#[test]
fn analyze_args_group_by_section() {
    let params = AnalyzeParams {
        group_by: Some("section".to_string()),
        ..Default::default()
    };
    let args = build_analyze_args(&params).unwrap();
    assert!(
        args.windows(2).any(|w| w == ["--group-by", "section"]),
        "expected args to contain --group-by section, got {args:?}"
    );
}

#[test]
fn analyze_args_production_false_is_omitted() {
    let params = AnalyzeParams {
        production: Some(false),
        ..Default::default()
    };
    let args = build_analyze_args(&params).unwrap();
    assert!(!args.contains(&"--production".to_string()));
}

#[test]
fn analyze_args_invalid_issue_type_returns_error() {
    let params = AnalyzeParams {
        issue_types: Some(vec!["nonexistent-type".to_string()]),
        ..Default::default()
    };
    let err = build_analyze_args(&params).unwrap_err();
    let msg = parse_validation_message(&err);
    assert!(msg.contains("Unknown issue type 'nonexistent-type'"));
    assert!(msg.contains("unused-files"));
}

#[test]
fn analyze_args_all_issue_types_accepted() {
    let all_types: Vec<String> = ISSUE_TYPE_FLAGS
        .iter()
        .map(|&(name, _)| name.to_string())
        .collect();
    let params = AnalyzeParams {
        issue_types: Some(all_types),
        ..Default::default()
    };
    let args = build_analyze_args(&params).unwrap();
    for &(_, flag) in ISSUE_TYPE_FLAGS {
        assert!(
            args.contains(&flag.to_string()),
            "missing flag {flag} in args"
        );
    }
}

#[test]
fn analyze_args_mixed_valid_and_invalid_issue_types_fails_on_first_invalid() {
    let params = AnalyzeParams {
        issue_types: Some(vec![
            "unused-files".to_string(),
            "bogus".to_string(),
            "unused-deps".to_string(),
        ]),
        ..Default::default()
    };
    let err = build_analyze_args(&params).unwrap_err();
    let msg = parse_validation_message(&err);
    assert!(msg.contains("'bogus'"));
}

#[test]
fn analyze_args_empty_issue_types_vec_produces_no_flags() {
    let params = AnalyzeParams {
        issue_types: Some(vec![]),
        ..Default::default()
    };
    let args = build_analyze_args(&params).unwrap();
    assert_eq!(
        args,
        ["dead-code", "--format", "json", "--quiet", "--explain"]
    );
}

// ── Argument building: check_changed ──────────────────────────────

#[test]
fn check_changed_args_includes_since_ref() {
    let args = build_check_changed_args(check_changed("main"));
    assert_eq!(
        args,
        [
            "dead-code",
            "--format",
            "json",
            "--quiet",
            "--explain",
            "--changed-since",
            "main"
        ]
    );
}

#[test]
fn check_changed_args_with_all_options() {
    let params = CheckChangedParams {
        root: Some("/app".to_string()),
        since: "HEAD~5".to_string(),
        config: Some("custom.json".to_string()),
        production: Some(true),
        workspace: Some("frontend".to_string()),
        baseline: Some("base.json".to_string()),
        save_baseline: Some("new.json".to_string()),
        fail_on_regression: Some(true),
        tolerance: Some("5".to_string()),
        regression_baseline: Some("reg.json".to_string()),
        save_regression_baseline: Some("new-reg.json".to_string()),
        include_entry_exports: None,
        no_cache: Some(true),
        threads: Some(2),
    };
    let args = build_check_changed_args(params);
    assert_eq!(
        args,
        [
            "dead-code",
            "--format",
            "json",
            "--quiet",
            "--explain",
            "--changed-since",
            "HEAD~5",
            "--root",
            "/app",
            "--config",
            "custom.json",
            "--no-cache",
            "--threads",
            "2",
            "--production",
            "--workspace",
            "frontend",
            "--baseline",
            "base.json",
            "--save-baseline",
            "new.json",
            "--fail-on-regression",
            "--tolerance",
            "5",
            "--regression-baseline",
            "reg.json",
            "--save-regression-baseline",
            "new-reg.json",
        ]
    );
}

#[test]
fn check_changed_args_with_commit_sha() {
    let args = build_check_changed_args(check_changed("abc123def456"));
    assert!(args.contains(&"abc123def456".to_string()));
}

// ── Argument building: find_dupes ─────────────────────────────────

#[test]
fn find_dupes_args_minimal() {
    let args = build_find_dupes_args(&FindDupesParams::default()).unwrap();
    assert_eq!(args, ["dupes", "--format", "json", "--quiet", "--explain"]);
}

#[test]
fn find_dupes_args_with_all_options() {
    let params = FindDupesParams {
        root: Some("/repo".to_string()),
        config: Some("fallow.toml".to_string()),
        workspace: Some("@my/lib".to_string()),
        mode: Some("semantic".to_string()),
        min_tokens: Some(100),
        min_lines: Some(10),
        threshold: Some(5.5),
        skip_local: Some(true),
        cross_language: Some(true),
        ignore_imports: Some(true),
        top: Some(5),
        baseline: Some("base.json".to_string()),
        save_baseline: Some("new.json".to_string()),
        no_cache: Some(true),
        threads: Some(8),
        changed_since: Some("main".to_string()),
        group_by: None,
    };
    let args = build_find_dupes_args(&params).unwrap();
    assert_eq!(
        args,
        [
            "dupes",
            "--format",
            "json",
            "--quiet",
            "--explain",
            "--root",
            "/repo",
            "--config",
            "fallow.toml",
            "--no-cache",
            "--threads",
            "8",
            "--workspace",
            "@my/lib",
            "--mode",
            "semantic",
            "--min-tokens",
            "100",
            "--min-lines",
            "10",
            "--threshold",
            "5.5",
            "--skip-local",
            "--cross-language",
            "--ignore-imports",
            "--top",
            "5",
            "--baseline",
            "base.json",
            "--save-baseline",
            "new.json",
            "--changed-since",
            "main",
        ]
    );
}

#[test]
fn find_dupes_args_all_valid_modes_accepted() {
    for mode in VALID_DUPES_MODES {
        let params = FindDupesParams {
            mode: Some(mode.to_string()),
            ..Default::default()
        };
        let args = build_find_dupes_args(&params).unwrap();
        assert!(
            args.contains(&mode.to_string()),
            "mode '{mode}' should be in args"
        );
    }
}

#[test]
fn find_dupes_args_invalid_mode_returns_error() {
    let params = FindDupesParams {
        mode: Some("aggressive".to_string()),
        ..Default::default()
    };
    let err = build_find_dupes_args(&params).unwrap_err();
    let msg = parse_validation_message(&err);
    assert!(msg.contains("Invalid mode 'aggressive'"));
    assert!(msg.contains("strict"));
    assert!(msg.contains("mild"));
    assert!(msg.contains("weak"));
    assert!(msg.contains("semantic"));
}

#[test]
fn find_dupes_args_skip_local_false_is_omitted() {
    let params = FindDupesParams {
        skip_local: Some(false),
        ..Default::default()
    };
    let args = build_find_dupes_args(&params).unwrap();
    assert!(!args.contains(&"--skip-local".to_string()));
}

#[test]
fn find_dupes_args_threshold_zero() {
    let params = FindDupesParams {
        threshold: Some(0.0),
        ..Default::default()
    };
    let args = build_find_dupes_args(&params).unwrap();
    assert!(args.contains(&"--threshold".to_string()));
    assert!(args.contains(&"0".to_string()));
}

#[test]
fn find_dupes_args_group_by_section() {
    let params = FindDupesParams {
        group_by: Some("section".to_string()),
        ..Default::default()
    };
    let args = build_find_dupes_args(&params).unwrap();
    assert!(
        args.windows(2).any(|w| w == ["--group-by", "section"]),
        "expected --group-by section, got {args:?}"
    );
}

// ── Argument building: fix_preview vs fix_apply ───────────────────

#[test]
fn fix_preview_args_include_dry_run() {
    let args = build_fix_preview_args(&FixParams::default());
    assert!(args.contains(&"--dry-run".to_string()));
    assert!(!args.contains(&"--yes".to_string()));
    assert_eq!(args[0], "fix");
}

#[test]
fn fix_apply_args_include_yes_flag() {
    let args = build_fix_apply_args(&FixParams::default());
    assert!(args.contains(&"--yes".to_string()));
    assert!(!args.contains(&"--dry-run".to_string()));
    assert_eq!(args[0], "fix");
}

#[test]
fn fix_preview_args_with_all_options() {
    let params = FixParams {
        root: Some("/app".to_string()),
        config: Some("config.json".to_string()),
        production: Some(true),
        workspace: Some("frontend".to_string()),
        no_cache: Some(true),
        threads: Some(4),
    };
    let args = build_fix_preview_args(&params);
    assert_eq!(
        args,
        [
            "fix",
            "--dry-run",
            "--format",
            "json",
            "--quiet",
            "--root",
            "/app",
            "--config",
            "config.json",
            "--no-cache",
            "--threads",
            "4",
            "--production",
            "--workspace",
            "frontend",
        ]
    );
}

#[test]
fn fix_apply_args_with_all_options() {
    let params = FixParams {
        root: Some("/app".to_string()),
        config: Some("config.json".to_string()),
        production: Some(true),
        workspace: Some("frontend".to_string()),
        no_cache: Some(true),
        threads: Some(4),
    };
    let args = build_fix_apply_args(&params);
    assert_eq!(
        args,
        [
            "fix",
            "--yes",
            "--format",
            "json",
            "--quiet",
            "--root",
            "/app",
            "--config",
            "config.json",
            "--no-cache",
            "--threads",
            "4",
            "--production",
            "--workspace",
            "frontend",
        ]
    );
}

// ── Argument building: project_info ───────────────────────────────

#[test]
fn project_info_args_minimal() {
    let args = build_project_info_args(&ProjectInfoParams::default());
    assert_eq!(args, ["list", "--format", "json", "--quiet"]);
}

#[test]
fn project_info_args_with_all_options() {
    let params = ProjectInfoParams {
        root: Some("/workspace".to_string()),
        config: Some("fallow.toml".to_string()),
        entry_points: Some(true),
        files: Some(true),
        plugins: Some(true),
        boundaries: Some(true),
        no_cache: Some(true),
        threads: Some(2),
    };
    let args = build_project_info_args(&params);
    assert_eq!(
        args,
        [
            "list",
            "--format",
            "json",
            "--quiet",
            "--root",
            "/workspace",
            "--config",
            "fallow.toml",
            "--no-cache",
            "--threads",
            "2",
            "--entry-points",
            "--files",
            "--plugins",
            "--boundaries",
        ]
    );
}

// ── Argument building: trace tools ───────────────────────────────

#[test]
fn trace_export_args_minimal() {
    let args = build_trace_export_args(&TraceExportParams {
        file: "src/utils.ts".to_string(),
        export_name: "usedFunction".to_string(),
        root: None,
        config: None,
        production: None,
        workspace: None,
        no_cache: None,
        threads: None,
    })
    .unwrap();
    assert_eq!(
        args,
        [
            "dead-code",
            "--format",
            "json",
            "--quiet",
            "--trace",
            "src/utils.ts:usedFunction",
        ]
    );
}

#[test]
fn trace_file_args_with_scope() {
    let args = build_trace_file_args(&TraceFileParams {
        file: "src/utils.ts".to_string(),
        root: Some("/repo".to_string()),
        config: Some("fallow.toml".to_string()),
        production: Some(true),
        workspace: Some("packages/web".to_string()),
        no_cache: Some(true),
        threads: Some(3),
    })
    .unwrap();
    assert_eq!(
        args,
        [
            "dead-code",
            "--format",
            "json",
            "--quiet",
            "--root",
            "/repo",
            "--config",
            "fallow.toml",
            "--no-cache",
            "--threads",
            "3",
            "--production",
            "--workspace",
            "packages/web",
            "--trace-file",
            "src/utils.ts",
        ]
    );
}

#[test]
fn trace_dependency_args_minimal() {
    let args = build_trace_dependency_args(&TraceDependencyParams {
        package_name: "react".to_string(),
        root: None,
        config: None,
        production: None,
        workspace: None,
        no_cache: None,
        threads: None,
    })
    .unwrap();
    assert_eq!(
        args,
        [
            "dead-code",
            "--format",
            "json",
            "--quiet",
            "--trace-dependency",
            "react",
        ]
    );
}

#[test]
fn trace_clone_args_with_all_options() {
    let args = build_trace_clone_args(&TraceCloneParams {
        file: "src/original.ts".to_string(),
        line: 12,
        root: Some("/repo".to_string()),
        config: Some("fallow.toml".to_string()),
        workspace: Some("packages/ui".to_string()),
        mode: Some("semantic".to_string()),
        min_tokens: Some(80),
        min_lines: Some(7),
        threshold: Some(3.5),
        skip_local: Some(true),
        cross_language: Some(true),
        ignore_imports: Some(true),
        no_cache: Some(true),
        threads: Some(6),
    })
    .unwrap();
    assert_eq!(
        args,
        [
            "dupes",
            "--format",
            "json",
            "--quiet",
            "--root",
            "/repo",
            "--config",
            "fallow.toml",
            "--no-cache",
            "--threads",
            "6",
            "--workspace",
            "packages/ui",
            "--mode",
            "semantic",
            "--min-tokens",
            "80",
            "--min-lines",
            "7",
            "--threshold",
            "3.5",
            "--skip-local",
            "--cross-language",
            "--ignore-imports",
            "--trace",
            "src/original.ts:12",
        ]
    );
}

#[test]
fn trace_clone_args_invalid_mode_returns_error() {
    let err = build_trace_clone_args(&TraceCloneParams {
        file: "src/original.ts".to_string(),
        line: 2,
        root: None,
        config: None,
        workspace: None,
        mode: Some("bogus".to_string()),
        min_tokens: None,
        min_lines: None,
        threshold: None,
        skip_local: None,
        cross_language: None,
        ignore_imports: None,
        no_cache: None,
        threads: None,
    })
    .unwrap_err();
    assert!(parse_validation_message(&err).contains("Invalid mode 'bogus'"));
}

#[test]
fn trace_args_reject_blank_required_values() {
    let export_err = build_trace_export_args(&TraceExportParams {
        file: " ".to_string(),
        export_name: "usedFunction".to_string(),
        root: None,
        config: None,
        production: None,
        workspace: None,
        no_cache: None,
        threads: None,
    })
    .unwrap_err();
    assert_eq!(
        parse_validation_message(&export_err),
        "file must not be empty"
    );

    let export_name_err = build_trace_export_args(&TraceExportParams {
        file: "src/utils.ts".to_string(),
        export_name: String::new(),
        root: None,
        config: None,
        production: None,
        workspace: None,
        no_cache: None,
        threads: None,
    })
    .unwrap_err();
    assert_eq!(
        parse_validation_message(&export_name_err),
        "export_name must not be empty"
    );

    let file_err = build_trace_file_args(&TraceFileParams {
        file: "\t".to_string(),
        root: None,
        config: None,
        production: None,
        workspace: None,
        no_cache: None,
        threads: None,
    })
    .unwrap_err();
    assert_eq!(
        parse_validation_message(&file_err),
        "file must not be empty"
    );

    let dependency_err = build_trace_dependency_args(&TraceDependencyParams {
        package_name: String::new(),
        root: None,
        config: None,
        production: None,
        workspace: None,
        no_cache: None,
        threads: None,
    })
    .unwrap_err();
    assert_eq!(
        parse_validation_message(&dependency_err),
        "package_name must not be empty"
    );
}

#[test]
fn trace_clone_args_reject_zero_line() {
    let err = build_trace_clone_args(&TraceCloneParams {
        file: "src/original.ts".to_string(),
        line: 0,
        root: None,
        config: None,
        workspace: None,
        mode: None,
        min_tokens: None,
        min_lines: None,
        threshold: None,
        skip_local: None,
        cross_language: None,
        ignore_imports: None,
        no_cache: None,
        threads: None,
    })
    .unwrap_err();
    assert_eq!(
        parse_validation_message(&err),
        "line must be greater than 0"
    );
}

// ── Validation error body shape ──────────────────────────────────

#[test]
fn validation_errors_use_structured_json_body() {
    // Every arg-builder validation failure must emit the same JSON shape that
    // `run_fallow` uses for CLI error exits, so MCP clients can decode one shape
    // for both error sources. `exit_code` is 0 on validation paths (no subprocess).
    let errors = [
        build_trace_clone_args(&TraceCloneParams {
            file: "src/original.ts".to_string(),
            line: 0,
            root: None,
            config: None,
            workspace: None,
            mode: None,
            min_tokens: None,
            min_lines: None,
            threshold: None,
            skip_local: None,
            cross_language: None,
            ignore_imports: None,
            no_cache: None,
            threads: None,
        })
        .unwrap_err(),
        build_trace_export_args(&TraceExportParams {
            file: String::new(),
            export_name: "foo".to_string(),
            root: None,
            config: None,
            production: None,
            workspace: None,
            no_cache: None,
            threads: None,
        })
        .unwrap_err(),
        build_find_dupes_args(&FindDupesParams {
            mode: Some("nope".to_string()),
            ..Default::default()
        })
        .unwrap_err(),
        build_analyze_args(&AnalyzeParams {
            issue_types: Some(vec!["bogus".to_string()]),
            ..Default::default()
        })
        .unwrap_err(),
    ];

    for body in &errors {
        let v: serde_json::Value = serde_json::from_str(body)
            .unwrap_or_else(|e| panic!("body should be valid JSON: `{body}` ({e})"));
        assert_eq!(
            v.as_object().map(serde_json::Map::len),
            Some(3),
            "exactly 3 keys expected in {body}"
        );
        assert_eq!(
            v["error"],
            serde_json::Value::Bool(true),
            "error=true in {body}"
        );
        assert_eq!(
            v["exit_code"],
            serde_json::Value::from(0),
            "exit_code=0 in {body}"
        );
        assert!(v["message"].is_string(), "message is a string in {body}");
        assert!(
            !v["message"].as_str().unwrap().is_empty(),
            "message is non-empty in {body}",
        );
    }
}

// ── Argument building: health ─────────────────────────────────────

#[test]
fn health_args_minimal() {
    let args = build_health_args(&HealthParams::default());
    assert_eq!(args, ["health", "--format", "json", "--quiet", "--explain"]);
}

#[test]
fn health_args_with_all_options() {
    let params = HealthParams {
        root: Some("/src".to_string()),
        config: Some("fallow.toml".to_string()),
        max_cyclomatic: Some(25),
        max_cognitive: Some(15),
        max_crap: Some(42.0),
        top: Some(20),
        sort: Some("cognitive".to_string()),
        changed_since: Some("develop".to_string()),
        complexity: Some(true),
        file_scores: Some(true),
        hotspots: Some(true),
        targets: None,
        coverage_gaps: Some(true),
        score: None,
        min_score: None,
        since: Some("6m".to_string()),
        min_commits: Some(5),
        workspace: Some("packages/ui".to_string()),
        production: Some(true),
        save_snapshot: None,
        baseline: Some("base.json".to_string()),
        save_baseline: Some("new.json".to_string()),
        no_cache: Some(true),
        threads: Some(4),
        trend: None,
        effort: Some("high".to_string()),
        summary: Some(true),
        coverage: Some("coverage/coverage-final.json".to_string()),
        coverage_root: Some("/ci/build".to_string()),
        production_coverage: Some("./coverage".to_string()),
        min_invocations_hot: Some(250),
        min_observation_volume: Some(7500),
        low_traffic_threshold: Some(0.002),
        min_severity: None,
        ownership: None,
        ownership_email_mode: None,
        group_by: None,
    };
    let args = build_health_args(&params);
    assert_eq!(
        args,
        [
            "health",
            "--format",
            "json",
            "--quiet",
            "--explain",
            "--root",
            "/src",
            "--config",
            "fallow.toml",
            "--no-cache",
            "--threads",
            "4",
            "--production",
            "--workspace",
            "packages/ui",
            "--max-cyclomatic",
            "25",
            "--max-cognitive",
            "15",
            "--max-crap",
            "42",
            "--top",
            "20",
            "--sort",
            "cognitive",
            "--changed-since",
            "develop",
            "--complexity",
            "--file-scores",
            "--hotspots",
            "--coverage-gaps",
            "--since",
            "6m",
            "--min-commits",
            "5",
            "--baseline",
            "base.json",
            "--save-baseline",
            "new.json",
            "--effort",
            "high",
            "--summary",
            "--coverage",
            "coverage/coverage-final.json",
            "--coverage-root",
            "/ci/build",
            "--production-coverage",
            "./coverage",
            "--min-invocations-hot",
            "250",
            "--min-observation-volume",
            "7500",
            "--low-traffic-threshold",
            "0.002",
        ]
    );
}

#[test]
fn health_args_partial_options() {
    let params = HealthParams {
        max_cyclomatic: Some(10),
        sort: Some("cyclomatic".to_string()),
        ..Default::default()
    };
    let args = build_health_args(&params);
    assert_eq!(
        args,
        [
            "health",
            "--format",
            "json",
            "--quiet",
            "--explain",
            "--max-cyclomatic",
            "10",
            "--sort",
            "cyclomatic",
        ]
    );
}

#[test]
fn health_args_group_by_section() {
    let params = HealthParams {
        group_by: Some("section".to_string()),
        ..Default::default()
    };
    let args = build_health_args(&params);
    assert!(
        args.windows(2).any(|w| w == ["--group-by", "section"]),
        "expected --group-by section, got {args:?}"
    );
}

#[test]
fn health_args_max_crap_alone() {
    let params = HealthParams {
        max_crap: Some(25.5),
        ..Default::default()
    };
    let args = build_health_args(&params);
    assert!(
        args.windows(2).any(|w| w == ["--max-crap", "25.5"]),
        "expected --max-crap 25.5, got {args:?}"
    );
}

#[test]
fn health_args_max_crap_integer_value() {
    let params = HealthParams {
        max_crap: Some(30.0),
        ..Default::default()
    };
    let args = build_health_args(&params);
    // Integer-valued floats render without a trailing ".0", keeping CLI
    // surface area stable for agents comparing args literally.
    assert!(
        args.windows(2).any(|w| w == ["--max-crap", "30"]),
        "expected --max-crap 30 (no trailing zero), got {args:?}"
    );
}

// ── All tools produce --format json --quiet ───────────────────────

#[test]
fn all_arg_builders_include_format_json_and_quiet() {
    let analyze = build_analyze_args(&AnalyzeParams::default()).unwrap();
    let check_changed = build_check_changed_args(check_changed("main"));
    let dupes = build_find_dupes_args(&FindDupesParams::default()).unwrap();
    let fix_preview = build_fix_preview_args(&FixParams::default());
    let fix_apply = build_fix_apply_args(&FixParams::default());
    let project_info = build_project_info_args(&ProjectInfoParams::default());
    let trace_export = build_trace_export_args(&TraceExportParams {
        file: "src/utils.ts".to_string(),
        export_name: "usedFunction".to_string(),
        root: None,
        config: None,
        production: None,
        workspace: None,
        no_cache: None,
        threads: None,
    })
    .unwrap();
    let trace_file = build_trace_file_args(&TraceFileParams {
        file: "src/utils.ts".to_string(),
        root: None,
        config: None,
        production: None,
        workspace: None,
        no_cache: None,
        threads: None,
    })
    .unwrap();
    let trace_dependency = build_trace_dependency_args(&TraceDependencyParams {
        package_name: "react".to_string(),
        root: None,
        config: None,
        production: None,
        workspace: None,
        no_cache: None,
        threads: None,
    })
    .unwrap();
    let trace_clone = build_trace_clone_args(&TraceCloneParams {
        file: "src/original.ts".to_string(),
        line: 2,
        root: None,
        config: None,
        workspace: None,
        mode: None,
        min_tokens: None,
        min_lines: None,
        threshold: None,
        skip_local: None,
        cross_language: None,
        ignore_imports: None,
        no_cache: None,
        threads: None,
    })
    .unwrap();
    let health = build_health_args(&HealthParams::default());
    let audit = build_audit_args(&AuditParams::default());
    let list_boundaries = build_list_boundaries_args(&ListBoundariesParams::default());
    let feature_flags = build_feature_flags_args(&FeatureFlagsParams::default());
    let check_production_coverage =
        build_check_production_coverage_args(&check_production_coverage("./coverage"));

    for (name, args) in [
        ("analyze", &analyze),
        ("check_changed", &check_changed),
        ("find_dupes", &dupes),
        ("fix_preview", &fix_preview),
        ("fix_apply", &fix_apply),
        ("project_info", &project_info),
        ("trace_export", &trace_export),
        ("trace_file", &trace_file),
        ("trace_dependency", &trace_dependency),
        ("trace_clone", &trace_clone),
        ("health", &health),
        ("audit", &audit),
        ("list_boundaries", &list_boundaries),
        ("feature_flags", &feature_flags),
        ("check_production_coverage", &check_production_coverage),
    ] {
        assert!(
            args.contains(&"--format".to_string()),
            "{name} missing --format"
        );
        assert!(args.contains(&"json".to_string()), "{name} missing json");
        assert!(
            args.contains(&"--quiet".to_string()),
            "{name} missing --quiet"
        );
    }
}

// ── Correct subcommand for each tool ──────────────────────────────

#[test]
fn each_tool_uses_correct_subcommand() {
    assert_eq!(
        build_analyze_args(&AnalyzeParams::default()).unwrap()[0],
        "dead-code"
    );
    assert_eq!(build_check_changed_args(check_changed("x"))[0], "dead-code");
    assert_eq!(
        build_find_dupes_args(&FindDupesParams::default()).unwrap()[0],
        "dupes"
    );
    assert_eq!(build_fix_preview_args(&FixParams::default())[0], "fix");
    assert_eq!(build_fix_apply_args(&FixParams::default())[0], "fix");
    assert_eq!(
        build_project_info_args(&ProjectInfoParams::default())[0],
        "list"
    );
    assert_eq!(build_health_args(&HealthParams::default())[0], "health");
    assert_eq!(
        build_list_boundaries_args(&ListBoundariesParams::default())[0],
        "list"
    );
    assert_eq!(
        build_feature_flags_args(&FeatureFlagsParams::default())[0],
        "flags"
    );
    assert_eq!(
        build_trace_export_args(&TraceExportParams {
            file: "src/utils.ts".to_string(),
            export_name: "usedFunction".to_string(),
            root: None,
            config: None,
            production: None,
            workspace: None,
            no_cache: None,
            threads: None,
        })
        .unwrap()[0],
        "dead-code"
    );
    assert_eq!(
        build_trace_file_args(&TraceFileParams {
            file: "src/utils.ts".to_string(),
            root: None,
            config: None,
            production: None,
            workspace: None,
            no_cache: None,
            threads: None,
        })
        .unwrap()[0],
        "dead-code"
    );
    assert_eq!(
        build_trace_dependency_args(&TraceDependencyParams {
            package_name: "react".to_string(),
            root: None,
            config: None,
            production: None,
            workspace: None,
            no_cache: None,
            threads: None,
        })
        .unwrap()[0],
        "dead-code"
    );
    assert_eq!(
        build_trace_clone_args(&TraceCloneParams {
            file: "src/original.ts".to_string(),
            line: 2,
            root: None,
            config: None,
            workspace: None,
            mode: None,
            min_tokens: None,
            min_lines: None,
            threshold: None,
            skip_local: None,
            cross_language: None,
            ignore_imports: None,
            no_cache: None,
            threads: None,
        })
        .unwrap()[0],
        "dupes"
    );
    assert_eq!(
        build_check_production_coverage_args(&check_production_coverage("./coverage"))[0],
        "health"
    );
}

// ── Argument building: check_production_coverage ─────────────────

#[test]
fn check_production_coverage_minimal_emits_coverage_flag() {
    let args = build_check_production_coverage_args(&check_production_coverage("./coverage"));
    assert_eq!(args[0], "health");
    assert!(args.contains(&"--production-coverage".to_string()));
    let idx = args
        .iter()
        .position(|a| a == "--production-coverage")
        .unwrap();
    assert_eq!(args[idx + 1], "./coverage");
    // Minimal params should NOT emit the tuning flags.
    assert!(!args.contains(&"--min-invocations-hot".to_string()));
    assert!(!args.contains(&"--min-observation-volume".to_string()));
    assert!(!args.contains(&"--low-traffic-threshold".to_string()));
    assert!(!args.contains(&"--group-by".to_string()));
}

#[test]
fn check_production_coverage_all_tuning_flags_emit() {
    let params = CheckProductionCoverageParams {
        coverage: "./coverage/coverage-final.json".to_string(),
        root: Some("/my/project".to_string()),
        config: Some(".fallowrc.json".to_string()),
        production: Some(true),
        workspace: Some("apps/web".to_string()),
        min_invocations_hot: Some(500),
        min_observation_volume: Some(10_000),
        low_traffic_threshold: Some(0.005),
        no_cache: Some(true),
        threads: Some(8),
        max_crap: Some(42.5),
        group_by: Some("owner".to_string()),
    };
    let args = build_check_production_coverage_args(&params);
    assert!(args.contains(&"--root".to_string()));
    assert!(args.contains(&"/my/project".to_string()));
    assert!(args.contains(&"--config".to_string()));
    assert!(args.contains(&".fallowrc.json".to_string()));
    assert!(args.contains(&"--production".to_string()));
    assert!(args.contains(&"--workspace".to_string()));
    assert!(args.contains(&"apps/web".to_string()));
    assert!(args.contains(&"--min-invocations-hot".to_string()));
    assert!(args.contains(&"500".to_string()));
    assert!(args.contains(&"--min-observation-volume".to_string()));
    assert!(args.contains(&"10000".to_string()));
    assert!(args.contains(&"--low-traffic-threshold".to_string()));
    assert!(args.contains(&"0.005".to_string()));
    assert!(args.contains(&"--no-cache".to_string()));
    assert!(args.contains(&"--threads".to_string()));
    assert!(args.contains(&"8".to_string()));
    assert!(
        args.windows(2).any(|w| w == ["--max-crap", "42.5"]),
        "expected --max-crap 42.5, got {args:?}"
    );
    assert!(
        args.windows(2).any(|w| w == ["--group-by", "owner"]),
        "expected --group-by owner, got {args:?}"
    );
}

#[test]
fn check_production_coverage_includes_explain() {
    let args = build_check_production_coverage_args(&check_production_coverage("./coverage"));
    assert!(
        args.contains(&"--explain".to_string()),
        "check_production_coverage should include --explain"
    );
}

// ── Explain flag presence ────────────────────────────────────────

#[test]
fn tools_with_explain_include_flag() {
    let analyze = build_analyze_args(&AnalyzeParams::default()).unwrap();
    assert!(
        analyze.contains(&"--explain".to_string()),
        "analyze should include --explain"
    );

    let changed = build_check_changed_args(check_changed("main"));
    assert!(
        changed.contains(&"--explain".to_string()),
        "check_changed should include --explain"
    );

    let dupes = build_find_dupes_args(&FindDupesParams::default()).unwrap();
    assert!(
        dupes.contains(&"--explain".to_string()),
        "find_dupes should include --explain"
    );

    let health = build_health_args(&HealthParams::default());
    assert!(
        health.contains(&"--explain".to_string()),
        "health should include --explain"
    );

    let feature_flags = build_feature_flags_args(&FeatureFlagsParams::default());
    assert!(
        feature_flags.contains(&"--explain".to_string()),
        "feature_flags should include --explain"
    );
}

#[test]
fn fix_tools_do_not_include_explain() {
    let preview = build_fix_preview_args(&FixParams::default());
    assert!(
        !preview.contains(&"--explain".to_string()),
        "fix_preview should not include --explain"
    );

    let apply = build_fix_apply_args(&FixParams::default());
    assert!(
        !apply.contains(&"--explain".to_string()),
        "fix_apply should not include --explain"
    );
}

#[test]
fn project_info_does_not_include_explain() {
    let args = build_project_info_args(&ProjectInfoParams::default());
    assert!(
        !args.contains(&"--explain".to_string()),
        "project_info should not include --explain"
    );
}

#[test]
fn trace_tools_do_not_include_explain() {
    let export = build_trace_export_args(&TraceExportParams {
        file: "src/utils.ts".to_string(),
        export_name: "usedFunction".to_string(),
        root: None,
        config: None,
        production: None,
        workspace: None,
        no_cache: None,
        threads: None,
    })
    .unwrap();
    let file = build_trace_file_args(&TraceFileParams {
        file: "src/utils.ts".to_string(),
        root: None,
        config: None,
        production: None,
        workspace: None,
        no_cache: None,
        threads: None,
    })
    .unwrap();
    let dep = build_trace_dependency_args(&TraceDependencyParams {
        package_name: "react".to_string(),
        root: None,
        config: None,
        production: None,
        workspace: None,
        no_cache: None,
        threads: None,
    })
    .unwrap();
    let clone = build_trace_clone_args(&TraceCloneParams {
        file: "src/original.ts".to_string(),
        line: 2,
        root: None,
        config: None,
        workspace: None,
        mode: None,
        min_tokens: None,
        min_lines: None,
        threshold: None,
        skip_local: None,
        cross_language: None,
        ignore_imports: None,
        no_cache: None,
        threads: None,
    })
    .unwrap();

    for (name, args) in [
        ("trace_export", export),
        ("trace_file", file),
        ("trace_dependency", dep),
        ("trace_clone", clone),
    ] {
        assert!(
            !args.contains(&"--explain".to_string()),
            "{name} should not include --explain"
        );
    }
}

// ── Global flags: no_cache boolean false is omitted ───────────────

#[test]
fn no_cache_false_is_omitted_across_all_tools() {
    let analyze = build_analyze_args(&AnalyzeParams {
        no_cache: Some(false),
        ..Default::default()
    })
    .unwrap();
    assert!(!analyze.contains(&"--no-cache".to_string()));

    let check_changed = build_check_changed_args(CheckChangedParams {
        since: "main".to_string(),
        no_cache: Some(false),
        root: None,
        config: None,
        production: None,
        workspace: None,
        baseline: None,
        save_baseline: None,
        fail_on_regression: None,
        tolerance: None,
        regression_baseline: None,
        save_regression_baseline: None,
        include_entry_exports: None,
        threads: None,
    });
    assert!(!check_changed.contains(&"--no-cache".to_string()));

    let dupes = build_find_dupes_args(&FindDupesParams {
        no_cache: Some(false),
        ..Default::default()
    })
    .unwrap();
    assert!(!dupes.contains(&"--no-cache".to_string()));

    let fix = build_fix_preview_args(&FixParams {
        no_cache: Some(false),
        ..Default::default()
    });
    assert!(!fix.contains(&"--no-cache".to_string()));

    let info = build_project_info_args(&ProjectInfoParams {
        no_cache: Some(false),
        ..Default::default()
    });
    assert!(!info.contains(&"--no-cache".to_string()));

    let health = build_health_args(&HealthParams {
        no_cache: Some(false),
        ..Default::default()
    });
    assert!(!health.contains(&"--no-cache".to_string()));

    let fix_apply = build_fix_apply_args(&FixParams {
        no_cache: Some(false),
        ..Default::default()
    });
    assert!(!fix_apply.contains(&"--no-cache".to_string()));

    let audit = build_audit_args(&AuditParams {
        no_cache: Some(false),
        ..Default::default()
    });
    assert!(!audit.contains(&"--no-cache".to_string()));

    let list_boundaries = build_list_boundaries_args(&ListBoundariesParams {
        no_cache: Some(false),
        ..Default::default()
    });
    assert!(!list_boundaries.contains(&"--no-cache".to_string()));

    let feature_flags = build_feature_flags_args(&FeatureFlagsParams {
        no_cache: Some(false),
        ..Default::default()
    });
    assert!(!feature_flags.contains(&"--no-cache".to_string()));
}

// ── Argument building: audit ─────────────────────────────────────

#[test]
fn audit_args_minimal_produces_base_args() {
    let args = build_audit_args(&AuditParams::default());
    assert_eq!(args, ["audit", "--format", "json", "--quiet", "--explain"]);
}

#[test]
fn audit_args_with_base() {
    let args = build_audit_args(&AuditParams {
        base: Some("main".to_string()),
        ..Default::default()
    });
    assert!(args.contains(&"--base".to_string()));
    assert!(args.contains(&"main".to_string()));
}

#[test]
fn audit_args_with_all_options() {
    let args = build_audit_args(&AuditParams {
        root: Some("/project".to_string()),
        config: Some(".fallowrc.json".to_string()),
        base: Some("develop".to_string()),
        production: Some(true),
        workspace: Some("@app/core".to_string()),
        no_cache: Some(true),
        threads: Some(4),
        group_by: None,
        dead_code_baseline: None,
        health_baseline: None,
        dupes_baseline: None,
        max_crap: Some(30.0),
    });
    assert!(args.contains(&"--root".to_string()));
    assert!(args.contains(&"/project".to_string()));
    assert!(args.contains(&"--config".to_string()));
    assert!(args.contains(&".fallowrc.json".to_string()));
    assert!(args.contains(&"--base".to_string()));
    assert!(args.contains(&"develop".to_string()));
    assert!(args.contains(&"--production".to_string()));
    assert!(args.contains(&"--workspace".to_string()));
    assert!(args.contains(&"@app/core".to_string()));
    assert!(args.contains(&"--no-cache".to_string()));
    assert!(args.contains(&"--threads".to_string()));
    assert!(args.contains(&"4".to_string()));
}

#[test]
fn audit_args_group_by_section() {
    let args = build_audit_args(&AuditParams {
        group_by: Some("section".to_string()),
        ..Default::default()
    });
    assert!(
        args.windows(2).any(|w| w == ["--group-by", "section"]),
        "expected --group-by section, got {args:?}"
    );
}

#[test]
fn audit_args_max_crap_forwards_to_cli() {
    let args = build_audit_args(&AuditParams {
        max_crap: Some(42.5),
        ..Default::default()
    });
    assert!(
        args.windows(2).any(|w| w == ["--max-crap", "42.5"]),
        "expected --max-crap 42.5, got {args:?}"
    );
}

#[test]
fn audit_args_per_analysis_baselines() {
    let args = build_audit_args(&AuditParams {
        dead_code_baseline: Some(".fallow-dead-code.json".to_string()),
        health_baseline: Some(".fallow-health.json".to_string()),
        dupes_baseline: Some(".fallow-dupes.json".to_string()),
        ..Default::default()
    });
    assert!(
        args.windows(2)
            .any(|w| w == ["--dead-code-baseline", ".fallow-dead-code.json"]),
        "expected --dead-code-baseline, got {args:?}"
    );
    assert!(
        args.windows(2)
            .any(|w| w == ["--health-baseline", ".fallow-health.json"]),
        "expected --health-baseline, got {args:?}"
    );
    assert!(
        args.windows(2)
            .any(|w| w == ["--dupes-baseline", ".fallow-dupes.json"]),
        "expected --dupes-baseline, got {args:?}"
    );
}

#[test]
fn audit_args_without_baselines_does_not_emit_flags() {
    let args = build_audit_args(&AuditParams::default());
    assert!(!args.iter().any(|a| a == "--dead-code-baseline"));
    assert!(!args.iter().any(|a| a == "--health-baseline"));
    assert!(!args.iter().any(|a| a == "--dupes-baseline"));
}

// ── Argument building: list_boundaries ──────────────────────────

#[test]
fn list_boundaries_args_minimal() {
    let args = build_list_boundaries_args(&ListBoundariesParams::default());
    assert_eq!(
        args,
        ["list", "--boundaries", "--format", "json", "--quiet"]
    );
}

#[test]
fn list_boundaries_args_all_options() {
    let params = ListBoundariesParams {
        root: Some("/workspace".to_string()),
        config: Some("fallow.toml".to_string()),
        no_cache: Some(true),
        threads: Some(4),
    };
    let args = build_list_boundaries_args(&params);
    assert_eq!(
        args,
        [
            "list",
            "--boundaries",
            "--format",
            "json",
            "--quiet",
            "--root",
            "/workspace",
            "--config",
            "fallow.toml",
            "--no-cache",
            "--threads",
            "4",
        ]
    );
}

// ── Argument building: find_dupes changed_since ─────────────────

#[test]
fn find_dupes_args_changed_since() {
    let params = FindDupesParams {
        changed_since: Some("feature/branch".to_string()),
        ..Default::default()
    };
    let args = build_find_dupes_args(&params).unwrap();
    assert!(args.contains(&"--changed-since".to_string()));
    assert!(args.contains(&"feature/branch".to_string()));
}

// ── Argument building: analyze boundary_violations ──────────────

#[test]
fn analyze_args_boundary_violations() {
    let params = AnalyzeParams {
        boundary_violations: Some(true),
        ..Default::default()
    };
    let args = build_analyze_args(&params).unwrap();
    assert!(args.contains(&"--boundary-violations".to_string()));
}

#[test]
fn analyze_args_boundary_violations_false_is_omitted() {
    let params = AnalyzeParams {
        boundary_violations: Some(false),
        ..Default::default()
    };
    let args = build_analyze_args(&params).unwrap();
    // boundary_violations=false should not add the flag
    assert_eq!(
        args.iter()
            .filter(|a| *a == "--boundary-violations")
            .count(),
        0
    );
}

#[test]
fn analyze_args_boundary_violations_deduped_with_issue_types() {
    // When both boundary_violations=true AND issue_types includes "boundary-violations",
    // the flag must appear exactly once — clap rejects duplicate boolean flags.
    let params = AnalyzeParams {
        boundary_violations: Some(true),
        issue_types: Some(vec!["boundary-violations".to_string()]),
        ..Default::default()
    };
    let args = build_analyze_args(&params).unwrap();
    let count = args
        .iter()
        .filter(|a| *a == "--boundary-violations")
        .count();
    assert_eq!(
        count, 1,
        "boundary_violations convenience param is skipped when issue_types already includes it"
    );
}

#[test]
fn analyze_args_boundary_violations_emitted_when_not_in_issue_types() {
    // boundary_violations=true with other issue_types (not boundary-violations) should still emit the flag
    let params = AnalyzeParams {
        boundary_violations: Some(true),
        issue_types: Some(vec!["unused-files".to_string()]),
        ..Default::default()
    };
    let args = build_analyze_args(&params).unwrap();
    assert!(args.contains(&"--boundary-violations".to_string()));
    assert!(args.contains(&"--unused-files".to_string()));
}

// ── Argument building: project_info section flags ───────────────

#[test]
fn project_info_args_section_flags() {
    let params = ProjectInfoParams {
        entry_points: Some(true),
        files: Some(true),
        plugins: Some(true),
        boundaries: Some(true),
        ..Default::default()
    };
    let args = build_project_info_args(&params);
    assert!(args.contains(&"--entry-points".to_string()));
    assert!(args.contains(&"--files".to_string()));
    assert!(args.contains(&"--plugins".to_string()));
    assert!(args.contains(&"--boundaries".to_string()));
}

#[test]
fn project_info_args_section_flags_false_are_omitted() {
    let params = ProjectInfoParams {
        entry_points: Some(false),
        files: Some(false),
        plugins: Some(false),
        boundaries: Some(false),
        ..Default::default()
    };
    let args = build_project_info_args(&params);
    assert!(!args.contains(&"--entry-points".to_string()));
    assert!(!args.contains(&"--files".to_string()));
    assert!(!args.contains(&"--plugins".to_string()));
    assert!(!args.contains(&"--boundaries".to_string()));
}

// ── Argument building: feature_flags ────────────────────────────

#[test]
fn feature_flags_args_minimal_produces_base_args() {
    let args = build_feature_flags_args(&FeatureFlagsParams::default());
    assert_eq!(args, ["flags", "--format", "json", "--quiet", "--explain"]);
}

#[test]
fn feature_flags_args_with_all_options() {
    let args = build_feature_flags_args(&FeatureFlagsParams {
        root: Some("/project".to_string()),
        config: Some(".fallowrc.json".to_string()),
        production: Some(true),
        workspace: Some("@app/core".to_string()),
        flag_type: None,
        confidence: None,
        no_cache: Some(true),
        threads: Some(4),
    });
    assert!(args.contains(&"--root".to_string()));
    assert!(args.contains(&"/project".to_string()));
    assert!(args.contains(&"--config".to_string()));
    assert!(args.contains(&".fallowrc.json".to_string()));
    assert!(args.contains(&"--production".to_string()));
    assert!(args.contains(&"--workspace".to_string()));
    assert!(args.contains(&"@app/core".to_string()));
    assert!(args.contains(&"--no-cache".to_string()));
    assert!(args.contains(&"--threads".to_string()));
    assert!(args.contains(&"4".to_string()));
}
