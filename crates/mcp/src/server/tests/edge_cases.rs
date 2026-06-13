use crate::params::*;
use crate::tools::{
    VALID_DUPES_MODES, build_analyze_args, build_check_changed_args, build_find_dupes_args,
    build_fix_apply_args, build_fix_preview_args, build_health_args, build_project_info_args,
};

#[test]
fn analyze_args_with_spaces_in_paths() {
    let params = AnalyzeParams {
        root: Some("/path/with spaces/project".to_string()),
        config: Some("my config.json".to_string()),
        workspace: Some("my package".to_string()),
        ..Default::default()
    };
    let args = build_analyze_args(&params).unwrap();
    assert!(args.contains(&"/path/with spaces/project".to_string()));
    assert!(args.contains(&"my config.json".to_string()));
    assert!(args.contains(&"my package".to_string()));
}

#[test]
fn check_changed_args_with_special_ref() {
    let params = CheckChangedParams {
        since: "origin/feature/my-branch".to_string(),
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
        no_cache: None,
        threads: None,
    };
    let args = build_check_changed_args(params);
    assert!(args.contains(&"origin/feature/my-branch".to_string()));
}

#[test]
fn health_args_boundary_values() {
    let params = HealthParams {
        max_cyclomatic: Some(0),
        max_cognitive: Some(u16::MAX),
        top: Some(0),
        ..Default::default()
    };
    let args = build_health_args(&params);
    assert!(args.contains(&"0".to_string()));
    assert!(args.contains(&"65535".to_string()));
}

#[test]
fn health_args_file_scores_flag() {
    let params = HealthParams {
        file_scores: Some(true),
        ..Default::default()
    };
    let args = build_health_args(&params);
    assert!(args.contains(&"--file-scores".to_string()));
}

#[test]
fn check_changed_args_production_false_is_omitted() {
    let params = CheckChangedParams {
        since: "main".to_string(),
        production: Some(false),
        root: None,
        config: None,
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
    };
    let args = build_check_changed_args(params);
    assert!(!args.contains(&"--production".to_string()));
}

#[test]
fn find_dupes_args_cross_language_false_is_omitted() {
    let params = FindDupesParams {
        cross_language: Some(false),
        ..Default::default()
    };
    let args = build_find_dupes_args(&params).unwrap();
    assert!(!args.contains(&"--cross-language".to_string()));
}

#[test]
fn find_dupes_args_ignore_imports_false_emits_opt_out() {
    let params = FindDupesParams {
        ignore_imports: Some(false),
        ..Default::default()
    };
    let args = build_find_dupes_args(&params).unwrap();
    assert!(args.contains(&"--no-ignore-imports".to_string()));
    assert!(!args.contains(&"--ignore-imports".to_string()));
}

#[test]
fn find_dupes_args_ignore_imports_true_emits_opt_in() {
    let params = FindDupesParams {
        ignore_imports: Some(true),
        ..Default::default()
    };
    let args = build_find_dupes_args(&params).unwrap();
    assert!(args.contains(&"--ignore-imports".to_string()));
    assert!(!args.contains(&"--no-ignore-imports".to_string()));
}

#[test]
fn find_dupes_args_ignore_imports_none_emits_neither() {
    let params = FindDupesParams {
        ignore_imports: None,
        ..Default::default()
    };
    let args = build_find_dupes_args(&params).unwrap();
    assert!(!args.contains(&"--ignore-imports".to_string()));
    assert!(!args.contains(&"--no-ignore-imports".to_string()));
}

#[test]
fn fix_preview_args_production_false_is_omitted() {
    let params = FixParams {
        production: Some(false),
        ..Default::default()
    };
    let args = build_fix_preview_args(&params);
    assert!(!args.contains(&"--production".to_string()));
}

#[test]
fn fix_apply_args_production_false_is_omitted() {
    let params = FixParams {
        production: Some(false),
        ..Default::default()
    };
    let args = build_fix_apply_args(&params);
    assert!(!args.contains(&"--production".to_string()));
}

#[test]
fn health_args_boolean_flags_false_are_omitted() {
    let params = HealthParams {
        complexity: Some(false),
        file_scores: Some(false),
        hotspots: Some(false),
        production: Some(false),
        no_cache: Some(false),
        ..Default::default()
    };
    let args = build_health_args(&params);
    assert!(!args.contains(&"--complexity".to_string()));
    assert!(!args.contains(&"--file-scores".to_string()));
    assert!(!args.contains(&"--hotspots".to_string()));
    assert!(!args.contains(&"--production".to_string()));
    assert!(!args.contains(&"--no-cache".to_string()));
}

#[test]
fn health_args_complexity_flag_only() {
    let params = HealthParams {
        complexity: Some(true),
        ..Default::default()
    };
    let args = build_health_args(&params);
    assert!(args.contains(&"--complexity".to_string()));
    assert!(!args.contains(&"--file-scores".to_string()));
    assert!(!args.contains(&"--hotspots".to_string()));
}

#[test]
fn health_args_complexity_breakdown_flag() {
    let on = build_health_args(&HealthParams {
        complexity_breakdown: Some(true),
        ..Default::default()
    });
    assert!(on.contains(&"--complexity-breakdown".to_string()));

    // The breakdown flag is independent of --complexity and absent by default.
    let off = build_health_args(&HealthParams::default());
    assert!(!off.contains(&"--complexity-breakdown".to_string()));
}

#[test]
fn health_args_css_flag() {
    let on = build_health_args(&HealthParams {
        css: Some(true),
        ..Default::default()
    });
    assert!(on.contains(&"--css".to_string()));

    let off = build_health_args(&HealthParams::default());
    assert!(!off.contains(&"--css".to_string()));
}

#[test]
fn health_args_hotspots_flag_only() {
    let params = HealthParams {
        hotspots: Some(true),
        ..Default::default()
    };
    let args = build_health_args(&params);
    assert!(args.contains(&"--hotspots".to_string()));
    assert!(!args.contains(&"--complexity".to_string()));
    assert!(!args.contains(&"--file-scores".to_string()));
}

#[test]
fn health_args_since_and_min_commits() {
    let params = HealthParams {
        since: Some("90d".to_string()),
        min_commits: Some(10),
        ..Default::default()
    };
    let args = build_health_args(&params);
    assert!(args.contains(&"--since".to_string()));
    assert!(args.contains(&"90d".to_string()));
    assert!(args.contains(&"--min-commits".to_string()));
    assert!(args.contains(&"10".to_string()));
}

#[test]
fn health_args_workspace_and_production() {
    let params = HealthParams {
        workspace: Some("@scope/pkg".to_string()),
        production: Some(true),
        ..Default::default()
    };
    let args = build_health_args(&params);
    assert!(args.contains(&"--workspace".to_string()));
    assert!(args.contains(&"@scope/pkg".to_string()));
    assert!(args.contains(&"--production".to_string()));
}

#[test]
fn find_dupes_args_individual_numeric_params() {
    let params = FindDupesParams {
        min_tokens: Some(75),
        ..Default::default()
    };
    let args = build_find_dupes_args(&params).unwrap();
    assert!(args.contains(&"--min-tokens".to_string()));
    assert!(args.contains(&"75".to_string()));
    assert!(!args.contains(&"--min-lines".to_string()));
    assert!(!args.contains(&"--threshold".to_string()));
    assert!(!args.contains(&"--top".to_string()));
}

#[test]
fn find_dupes_args_top_only() {
    let params = FindDupesParams {
        top: Some(3),
        ..Default::default()
    };
    let args = build_find_dupes_args(&params).unwrap();
    assert!(args.contains(&"--top".to_string()));
    assert!(args.contains(&"3".to_string()));
}

#[test]
fn check_changed_args_only_root() {
    let params = CheckChangedParams {
        root: Some("/workspace".to_string()),
        since: "HEAD~1".to_string(),
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
    };
    let args = build_check_changed_args(params);
    assert!(args.contains(&"--root".to_string()));
    assert!(args.contains(&"/workspace".to_string()));
    assert!(!args.contains(&"--config".to_string()));
    assert!(!args.contains(&"--production".to_string()));
    assert!(!args.contains(&"--workspace".to_string()));
}

#[test]
fn project_info_args_only_root() {
    let params = ProjectInfoParams {
        root: Some("/app".to_string()),
        ..Default::default()
    };
    let args = build_project_info_args(&params);
    assert!(args.contains(&"--root".to_string()));
    assert!(args.contains(&"/app".to_string()));
    assert!(!args.contains(&"--config".to_string()));
}

#[test]
fn project_info_args_only_config() {
    let params = ProjectInfoParams {
        config: Some(".fallowrc.json".to_string()),
        ..Default::default()
    };
    let args = build_project_info_args(&params);
    assert!(args.contains(&"--config".to_string()));
    assert!(args.contains(&".fallowrc.json".to_string()));
    assert!(!args.contains(&"--root".to_string()));
}

#[test]
fn analyze_args_baseline_only() {
    let params = AnalyzeParams {
        baseline: Some("baseline.json".to_string()),
        ..Default::default()
    };
    let args = build_analyze_args(&params).unwrap();
    assert!(args.contains(&"--baseline".to_string()));
    assert!(args.contains(&"baseline.json".to_string()));
    assert!(!args.contains(&"--save-baseline".to_string()));
}

#[test]
fn analyze_args_threads_only() {
    let params = AnalyzeParams {
        threads: Some(16),
        ..Default::default()
    };
    let args = build_analyze_args(&params).unwrap();
    assert!(args.contains(&"--threads".to_string()));
    assert!(args.contains(&"16".to_string()));
}

#[test]
fn find_dupes_args_config_and_workspace() {
    let params = FindDupesParams {
        config: Some("custom.toml".to_string()),
        workspace: Some("libs/core".to_string()),
        ..Default::default()
    };
    let args = build_find_dupes_args(&params).unwrap();
    assert!(args.contains(&"--config".to_string()));
    assert!(args.contains(&"custom.toml".to_string()));
    assert!(args.contains(&"--workspace".to_string()));
    assert!(args.contains(&"libs/core".to_string()));
}

#[test]
fn fix_args_workspace_only() {
    let params = FixParams {
        workspace: Some("@my/pkg".to_string()),
        ..Default::default()
    };
    let preview = build_fix_preview_args(&params);
    assert!(preview.contains(&"--workspace".to_string()));
    assert!(preview.contains(&"@my/pkg".to_string()));

    let apply = build_fix_apply_args(&params);
    assert!(apply.contains(&"--workspace".to_string()));
    assert!(apply.contains(&"@my/pkg".to_string()));
}

#[test]
fn health_args_config_only() {
    let params = HealthParams {
        config: Some("health.toml".to_string()),
        ..Default::default()
    };
    let args = build_health_args(&params);
    assert!(args.contains(&"--config".to_string()));
    assert!(args.contains(&"health.toml".to_string()));
}

#[test]
fn health_args_baseline_and_save_baseline() {
    let params = HealthParams {
        baseline: Some("old.json".to_string()),
        save_baseline: Some("new.json".to_string()),
        ..Default::default()
    };
    let args = build_health_args(&params);
    assert!(args.contains(&"--baseline".to_string()));
    assert!(args.contains(&"old.json".to_string()));
    assert!(args.contains(&"--save-baseline".to_string()));
    assert!(args.contains(&"new.json".to_string()));
}

#[test]
fn health_args_targets_flag_only() {
    let params = HealthParams {
        targets: Some(true),
        ..Default::default()
    };
    let args = build_health_args(&params);
    assert!(args.contains(&"--targets".to_string()));
    assert!(!args.contains(&"--complexity".to_string()));
    assert!(!args.contains(&"--file-scores".to_string()));
    assert!(!args.contains(&"--hotspots".to_string()));
}

#[test]
fn health_args_targets_false_is_omitted() {
    let params = HealthParams {
        targets: Some(false),
        ..Default::default()
    };
    let args = build_health_args(&params);
    assert!(!args.contains(&"--targets".to_string()));
}

#[test]
fn health_args_save_snapshot_with_path() {
    let params = HealthParams {
        save_snapshot: Some("snapshots/v1.json".to_string()),
        ..Default::default()
    };
    let args = build_health_args(&params);
    assert!(args.contains(&"--save-snapshot".to_string()));
    assert!(args.contains(&"snapshots/v1.json".to_string()));
}

#[test]
fn health_args_save_snapshot_empty_string_produces_valueless_flag() {
    let params = HealthParams {
        save_snapshot: Some(String::new()),
        ..Default::default()
    };
    let args = build_health_args(&params);
    assert!(args.contains(&"--save-snapshot".to_string()));
    let snap_idx = args.iter().position(|a| a == "--save-snapshot").unwrap();
    if let Some(next) = args.get(snap_idx + 1) {
        assert!(
            next.starts_with("--"),
            "expected no value after --save-snapshot for empty path, got '{next}'"
        );
    }
}

#[test]
fn health_args_save_snapshot_none_is_omitted() {
    let params = HealthParams {
        save_snapshot: None,
        ..Default::default()
    };
    let args = build_health_args(&params);
    assert!(!args.contains(&"--save-snapshot".to_string()));
}

#[test]
fn health_args_all_section_flags_together() {
    let params = HealthParams {
        complexity: Some(true),
        file_scores: Some(true),
        hotspots: Some(true),
        targets: Some(true),
        ..Default::default()
    };
    let args = build_health_args(&params);
    assert!(args.contains(&"--complexity".to_string()));
    assert!(args.contains(&"--file-scores".to_string()));
    assert!(args.contains(&"--hotspots".to_string()));
    assert!(args.contains(&"--targets".to_string()));
}

#[test]
fn find_dupes_args_cross_language_true() {
    let params = FindDupesParams {
        cross_language: Some(true),
        ..Default::default()
    };
    let args = build_find_dupes_args(&params).unwrap();
    assert!(args.contains(&"--cross-language".to_string()));
}

#[test]
fn valid_dupes_modes_count_and_contents() {
    assert_eq!(VALID_DUPES_MODES.len(), 4);
    assert!(VALID_DUPES_MODES.contains(&"strict"));
    assert!(VALID_DUPES_MODES.contains(&"mild"));
    assert!(VALID_DUPES_MODES.contains(&"weak"));
    assert!(VALID_DUPES_MODES.contains(&"semantic"));
}

#[test]
fn analyze_args_unicode_in_paths() {
    let params = AnalyzeParams {
        root: Some("/home/ユーザー/プロジェクト".to_string()),
        workspace: Some("パッケージ".to_string()),
        ..Default::default()
    };
    let args = build_analyze_args(&params).unwrap();
    assert!(args.contains(&"/home/ユーザー/プロジェクト".to_string()));
    assert!(args.contains(&"パッケージ".to_string()));
}

#[test]
fn analyze_args_empty_root_is_dropped() {
    let params = AnalyzeParams {
        root: Some(String::new()),
        ..Default::default()
    };
    let args = build_analyze_args(&params).unwrap();
    assert!(
        !args.iter().any(|a| a == "--root"),
        "expected empty --root to be dropped, got {args:?}"
    );
}

#[test]
fn health_args_empty_sort_is_dropped() {
    let params = HealthParams {
        sort: Some(String::new()),
        ..Default::default()
    };
    let args = build_health_args(&params);
    assert!(
        !args.iter().any(|a| a == "--sort"),
        "expected empty --sort to be dropped, got {args:?}"
    );
}

#[test]
fn find_dupes_args_min_lines_only() {
    let params = FindDupesParams {
        min_lines: Some(20),
        ..Default::default()
    };
    let args = build_find_dupes_args(&params).unwrap();
    assert!(args.contains(&"--min-lines".to_string()));
    assert!(args.contains(&"20".to_string()));
    assert!(!args.contains(&"--min-tokens".to_string()));
}

#[test]
fn find_dupes_args_baseline_and_save_baseline() {
    let params = FindDupesParams {
        baseline: Some("old.json".to_string()),
        save_baseline: Some("new.json".to_string()),
        ..Default::default()
    };
    let args = build_find_dupes_args(&params).unwrap();
    assert!(args.contains(&"--baseline".to_string()));
    assert!(args.contains(&"old.json".to_string()));
    assert!(args.contains(&"--save-baseline".to_string()));
    assert!(args.contains(&"new.json".to_string()));
}

#[test]
fn check_changed_args_baseline_only() {
    let params = CheckChangedParams {
        since: "main".to_string(),
        baseline: Some("baseline.json".to_string()),
        root: None,
        config: None,
        production: None,
        workspace: None,
        save_baseline: None,
        fail_on_regression: None,
        tolerance: None,
        regression_baseline: None,
        save_regression_baseline: None,
        include_entry_exports: None,
        no_cache: None,
        threads: None,
    };
    let args = build_check_changed_args(params);
    assert!(args.contains(&"--baseline".to_string()));
    assert!(args.contains(&"baseline.json".to_string()));
    assert!(!args.contains(&"--save-baseline".to_string()));
}

#[test]
fn check_changed_args_save_baseline_only() {
    let params = CheckChangedParams {
        since: "main".to_string(),
        save_baseline: Some("new.json".to_string()),
        root: None,
        config: None,
        production: None,
        workspace: None,
        baseline: None,
        fail_on_regression: None,
        tolerance: None,
        regression_baseline: None,
        save_regression_baseline: None,
        include_entry_exports: None,
        no_cache: None,
        threads: None,
    };
    let args = build_check_changed_args(params);
    assert!(!args.contains(&"--baseline".to_string()));
    assert!(args.contains(&"--save-baseline".to_string()));
    assert!(args.contains(&"new.json".to_string()));
}

#[test]
fn analyze_args_threads_zero() {
    let params = AnalyzeParams {
        threads: Some(0),
        ..Default::default()
    };
    let args = build_analyze_args(&params).unwrap();
    assert!(args.contains(&"--threads".to_string()));
    assert!(args.contains(&"0".to_string()));
}

#[test]
fn health_args_threads_large() {
    let params = HealthParams {
        threads: Some(1024),
        ..Default::default()
    };
    let args = build_health_args(&params);
    assert!(args.contains(&"--threads".to_string()));
    assert!(args.contains(&"1024".to_string()));
}

#[test]
fn health_args_changed_since_only() {
    let params = HealthParams {
        changed_since: Some("HEAD~10".to_string()),
        ..Default::default()
    };
    let args = build_health_args(&params);
    assert!(args.contains(&"--changed-since".to_string()));
    assert!(args.contains(&"HEAD~10".to_string()));
    assert!(!args.contains(&"--since".to_string()));
}

#[test]
fn health_args_max_cognitive_only() {
    let params = HealthParams {
        max_cognitive: Some(20),
        ..Default::default()
    };
    let args = build_health_args(&params);
    assert!(args.contains(&"--max-cognitive".to_string()));
    assert!(args.contains(&"20".to_string()));
    assert!(!args.contains(&"--max-cyclomatic".to_string()));
}

#[test]
fn health_args_save_snapshot_whitespace_only_passes_value() {
    let params = HealthParams {
        save_snapshot: Some("   ".to_string()),
        ..Default::default()
    };
    let args = build_health_args(&params);
    assert!(args.contains(&"--save-snapshot".to_string()));
    assert!(args.contains(&"   ".to_string()));
}

#[test]
fn health_args_with_all_options_including_targets_and_snapshot() {
    let params = HealthParams {
        root: Some("/project".to_string()),
        config: Some("fallow.toml".to_string()),
        max_cyclomatic: Some(25),
        max_cognitive: Some(15),
        max_crap: Some(30.0),
        top: Some(20),
        sort: Some("cognitive".to_string()),
        changed_since: Some("develop".to_string()),
        complexity: Some(true),
        complexity_breakdown: Some(true),
        css: None,
        file_scores: Some(true),
        hotspots: Some(true),
        targets: Some(true),
        coverage_gaps: Some(true),
        score: Some(true),
        min_score: Some(70.0),
        since: Some("6m".to_string()),
        min_commits: Some(5),
        churn_file: Some("churn.json".to_string()),
        workspace: Some("packages/ui".to_string()),
        production: Some(true),
        save_snapshot: Some("snap.json".to_string()),
        baseline: Some("base.json".to_string()),
        save_baseline: Some("new.json".to_string()),
        no_cache: Some(true),
        threads: Some(4),
        trend: Some(true),
        effort: Some("high".to_string()),
        summary: Some(true),
        coverage: Some("coverage/coverage-final.json".to_string()),
        coverage_root: Some("/home/runner/work/myapp".to_string()),
        runtime_coverage: Some("./coverage".to_string()),
        min_invocations_hot: Some(500),
        min_observation_volume: Some(10_000),
        low_traffic_threshold: Some(0.005),
        min_severity: Some("critical".to_string()),
        ownership: Some(true),
        ownership_email_mode: Some(crate::params::EmailModeParam::Anonymized),
        group_by: Some("section".to_string()),
    };
    let args = build_health_args(&params);
    assert!(args.contains(&"--ownership".to_string()));
    assert!(args.contains(&"--ownership-emails".to_string()));
    assert!(args.contains(&"anonymized".to_string()));
    assert_eq!(args.iter().filter(|a| *a == "--hotspots").count(), 1);
    assert!(args.contains(&"--targets".to_string()));
    assert!(args.contains(&"--coverage-gaps".to_string()));
    assert!(args.contains(&"--score".to_string()));
    assert!(args.contains(&"--min-score".to_string()));
    assert!(args.contains(&"70".to_string()));
    assert!(args.contains(&"--save-snapshot".to_string()));
    assert!(args.contains(&"snap.json".to_string()));
    assert!(args.contains(&"--churn-file".to_string()));
    assert!(args.contains(&"churn.json".to_string()));
    assert!(args.contains(&"--complexity".to_string()));
    assert!(args.contains(&"--file-scores".to_string()));
    assert!(args.contains(&"--hotspots".to_string()));
    assert!(args.contains(&"--production".to_string()));
    assert!(args.contains(&"--effort".to_string()));
    assert!(args.contains(&"high".to_string()));
    assert!(args.contains(&"--summary".to_string()));
    assert!(args.contains(&"--no-cache".to_string()));
    assert!(args.contains(&"--trend".to_string()));
    assert!(args.contains(&"--coverage".to_string()));
    assert!(args.contains(&"coverage/coverage-final.json".to_string()));
    assert!(args.contains(&"--coverage-root".to_string()));
    assert!(args.contains(&"/home/runner/work/myapp".to_string()));
    assert!(args.contains(&"--min-severity".to_string()));
    assert!(args.contains(&"critical".to_string()));
}

#[test]
fn check_changed_args_unicode_in_paths() {
    let params = CheckChangedParams {
        since: "main".to_string(),
        root: Some("/home/用户/项目".to_string()),
        config: Some("配置.json".to_string()),
        workspace: Some("包裹".to_string()),
        production: None,
        baseline: None,
        save_baseline: None,
        fail_on_regression: None,
        tolerance: None,
        regression_baseline: None,
        save_regression_baseline: None,
        include_entry_exports: None,
        no_cache: None,
        threads: None,
    };
    let args = build_check_changed_args(params);
    assert!(args.contains(&"/home/用户/项目".to_string()));
    assert!(args.contains(&"配置.json".to_string()));
    assert!(args.contains(&"包裹".to_string()));
}

#[test]
fn find_dupes_args_unicode_in_paths() {
    let params = FindDupesParams {
        root: Some("/home/ユーザー/プロジェクト".to_string()),
        config: Some("設定.toml".to_string()),
        workspace: Some("パッケージ".to_string()),
        ..Default::default()
    };
    let args = build_find_dupes_args(&params).unwrap();
    assert!(args.contains(&"/home/ユーザー/プロジェクト".to_string()));
    assert!(args.contains(&"設定.toml".to_string()));
    assert!(args.contains(&"パッケージ".to_string()));
}

#[test]
fn fix_args_unicode_in_paths() {
    let params = FixParams {
        root: Some("/home/사용자/프로젝트".to_string()),
        config: Some("설정.json".to_string()),
        workspace: Some("패키지".to_string()),
        ..Default::default()
    };
    let preview = build_fix_preview_args(&params);
    assert!(preview.contains(&"/home/사용자/프로젝트".to_string()));
    assert!(preview.contains(&"설정.json".to_string()));
    assert!(preview.contains(&"패키지".to_string()));

    let apply = build_fix_apply_args(&params);
    assert!(apply.contains(&"/home/사용자/프로젝트".to_string()));
    assert!(apply.contains(&"설정.json".to_string()));
    assert!(apply.contains(&"패키지".to_string()));
}

#[test]
fn health_args_unicode_in_paths() {
    let params = HealthParams {
        root: Some("/home/Benutzer/Projekt".to_string()),
        config: Some("Konfiguration.toml".to_string()),
        workspace: Some("Paket".to_string()),
        save_snapshot: Some("/Schnappschüsse/v1.json".to_string()),
        ..Default::default()
    };
    let args = build_health_args(&params);
    assert!(args.contains(&"/home/Benutzer/Projekt".to_string()));
    assert!(args.contains(&"Konfiguration.toml".to_string()));
    assert!(args.contains(&"Paket".to_string()));
    assert!(args.contains(&"/Schnappschüsse/v1.json".to_string()));
}

#[test]
fn project_info_args_unicode_in_paths() {
    let params = ProjectInfoParams {
        root: Some("/домой/пользователь/проект".to_string()),
        config: Some("конфиг.toml".to_string()),
        ..Default::default()
    };
    let args = build_project_info_args(&params);
    assert!(args.contains(&"/домой/пользователь/проект".to_string()));
    assert!(args.contains(&"конфиг.toml".to_string()));
}

#[test]
fn check_changed_args_empty_config_is_dropped() {
    let params = CheckChangedParams {
        since: "main".to_string(),
        config: Some(String::new()),
        root: None,
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
    };
    let args = build_check_changed_args(params);
    assert!(
        !args.iter().any(|a| a == "--config"),
        "expected empty --config to be dropped, got {args:?}"
    );
}

#[test]
fn find_dupes_args_empty_root_is_dropped() {
    let params = FindDupesParams {
        root: Some(String::new()),
        ..Default::default()
    };
    let args = build_find_dupes_args(&params).unwrap();
    assert!(
        !args.iter().any(|a| a == "--root"),
        "expected empty --root to be dropped, got {args:?}"
    );
}

#[test]
fn fix_args_empty_config_is_dropped() {
    let params = FixParams {
        config: Some(String::new()),
        ..Default::default()
    };
    let preview = build_fix_preview_args(&params);
    assert!(
        !preview.iter().any(|a| a == "--config"),
        "expected empty --config to be dropped from fix_preview, got {preview:?}"
    );

    let apply = build_fix_apply_args(&params);
    assert!(
        !apply.iter().any(|a| a == "--config"),
        "expected empty --config to be dropped from fix_apply, got {apply:?}"
    );
}

#[test]
fn check_changed_args_threads_boundary() {
    let params = CheckChangedParams {
        since: "main".to_string(),
        threads: Some(1),
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
        no_cache: None,
    };
    let args = build_check_changed_args(params);
    assert!(args.contains(&"--threads".to_string()));
    assert!(args.contains(&"1".to_string()));
}

#[test]
fn find_dupes_args_threads_zero() {
    let params = FindDupesParams {
        threads: Some(0),
        ..Default::default()
    };
    let args = build_find_dupes_args(&params).unwrap();
    assert!(args.contains(&"--threads".to_string()));
    assert!(args.contains(&"0".to_string()));
}

#[test]
fn fix_args_threads_large() {
    let params = FixParams {
        threads: Some(256),
        ..Default::default()
    };
    let preview = build_fix_preview_args(&params);
    assert!(preview.contains(&"--threads".to_string()));
    assert!(preview.contains(&"256".to_string()));

    let apply = build_fix_apply_args(&params);
    assert!(apply.contains(&"--threads".to_string()));
    assert!(apply.contains(&"256".to_string()));
}

#[test]
fn project_info_args_threads_zero() {
    let params = ProjectInfoParams {
        threads: Some(0),
        ..Default::default()
    };
    let args = build_project_info_args(&params);
    assert!(args.contains(&"--threads".to_string()));
    assert!(args.contains(&"0".to_string()));
}

#[test]
fn find_dupes_args_cross_language_none_is_omitted() {
    let params = FindDupesParams {
        cross_language: None,
        ..Default::default()
    };
    let args = build_find_dupes_args(&params).unwrap();
    assert!(!args.contains(&"--cross-language".to_string()));
}

// `ignore_imports` opt-in / opt-out / defer coverage lives in the
// `find_dupes_args_ignore_imports_{false_emits_opt_out,true_emits_opt_in,none_emits_neither}`
// trio earlier in this file (asserts both the emitted flag and the absence of
// its opposite); the older absence-only tests were removed as redundant.

#[test]
fn find_dupes_args_skip_local_true() {
    let params = FindDupesParams {
        skip_local: Some(true),
        ..Default::default()
    };
    let args = build_find_dupes_args(&params).unwrap();
    assert!(args.contains(&"--skip-local".to_string()));
}

#[test]
fn find_dupes_args_min_tokens_zero() {
    let params = FindDupesParams {
        min_tokens: Some(0),
        ..Default::default()
    };
    let args = build_find_dupes_args(&params).unwrap();
    assert!(args.contains(&"--min-tokens".to_string()));
    assert!(args.contains(&"0".to_string()));
}

#[test]
fn find_dupes_args_min_lines_zero() {
    let params = FindDupesParams {
        min_lines: Some(0),
        ..Default::default()
    };
    let args = build_find_dupes_args(&params).unwrap();
    assert!(args.contains(&"--min-lines".to_string()));
    assert!(args.contains(&"0".to_string()));
}

#[test]
fn find_dupes_args_threshold_negative() {
    let params = FindDupesParams {
        threshold: Some(-1.0),
        ..Default::default()
    };
    let args = build_find_dupes_args(&params).unwrap();
    assert!(args.contains(&"--threshold".to_string()));
    assert!(args.contains(&"-1".to_string()));
}

#[test]
fn check_changed_args_no_cache_true() {
    let params = CheckChangedParams {
        since: "main".to_string(),
        no_cache: Some(true),
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
    };
    let args = build_check_changed_args(params);
    assert!(args.contains(&"--no-cache".to_string()));
}

#[test]
fn fix_preview_args_config_only() {
    let params = FixParams {
        config: Some("custom.toml".to_string()),
        ..Default::default()
    };
    let args = build_fix_preview_args(&params);
    assert!(args.contains(&"--config".to_string()));
    assert!(args.contains(&"custom.toml".to_string()));
    assert!(!args.contains(&"--root".to_string()));
}

#[test]
fn fix_apply_args_root_only() {
    let params = FixParams {
        root: Some("/project".to_string()),
        ..Default::default()
    };
    let args = build_fix_apply_args(&params);
    assert!(args.contains(&"--root".to_string()));
    assert!(args.contains(&"/project".to_string()));
    assert!(!args.contains(&"--config".to_string()));
}

#[test]
fn project_info_args_no_cache_true() {
    let params = ProjectInfoParams {
        no_cache: Some(true),
        ..Default::default()
    };
    let args = build_project_info_args(&params);
    assert!(args.contains(&"--no-cache".to_string()));
    assert!(!args.contains(&"--root".to_string()));
    assert!(!args.contains(&"--config".to_string()));
}

#[test]
fn health_args_save_snapshot_with_value_has_correct_order() {
    let params = HealthParams {
        save_snapshot: Some("output/snap.json".to_string()),
        ..Default::default()
    };
    let args = build_health_args(&params);
    let snap_idx = args.iter().position(|a| a == "--save-snapshot").unwrap();
    assert_eq!(args[snap_idx + 1], "output/snap.json");
}

#[test]
fn health_args_min_commits_zero() {
    let params = HealthParams {
        min_commits: Some(0),
        ..Default::default()
    };
    let args = build_health_args(&params);
    assert!(args.contains(&"--min-commits".to_string()));
    assert!(args.contains(&"0".to_string()));
}

#[test]
fn health_args_max_cyclomatic_one() {
    let params = HealthParams {
        max_cyclomatic: Some(1),
        ..Default::default()
    };
    let args = build_health_args(&params);
    assert!(args.contains(&"--max-cyclomatic".to_string()));
    assert!(args.contains(&"1".to_string()));
}

#[test]
fn analyze_args_save_baseline_only() {
    let params = AnalyzeParams {
        save_baseline: Some("new.json".to_string()),
        ..Default::default()
    };
    let args = build_analyze_args(&params).unwrap();
    assert!(args.contains(&"--save-baseline".to_string()));
    assert!(args.contains(&"new.json".to_string()));
    assert!(!args.contains(&"--baseline".to_string()));
}

#[test]
fn analyze_args_single_issue_type() {
    let params = AnalyzeParams {
        issue_types: Some(vec!["circular-deps".to_string()]),
        ..Default::default()
    };
    let args = build_analyze_args(&params).unwrap();
    assert!(args.contains(&"--circular-deps".to_string()));
    assert!(!args.contains(&"--unused-files".to_string()));
    assert!(!args.contains(&"--unused-exports".to_string()));
}

#[test]
fn find_dupes_args_uppercase_mode_returns_error() {
    let params = FindDupesParams {
        mode: Some("Strict".to_string()),
        ..Default::default()
    };
    let err = build_find_dupes_args(&params).unwrap_err();
    assert!(err.contains("Invalid mode 'Strict'"));
}

#[test]
fn find_dupes_args_empty_mode_is_dropped() {
    let params = FindDupesParams {
        mode: Some(String::new()),
        ..Default::default()
    };
    let args = build_find_dupes_args(&params).unwrap();
    assert!(
        !args.iter().any(|a| a == "--mode"),
        "expected empty --mode to be dropped, got {args:?}"
    );
}
