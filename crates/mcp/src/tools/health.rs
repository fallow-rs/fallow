use crate::params::HealthParams;

use fallow_api::{
    AnalysisOptions, ComplexityOptions, ComplexitySort, CoverageInputs, OwnershipEmailMode,
    ProgrammaticError, TargetEffort, run_health as run_api_health,
    serialize_health_programmatic_json,
};
use rmcp::ErrorData as McpError;
use rmcp::model::{CallToolResult, ContentBlock};

use super::{
    api_runtime::{
        changed_since_from_param, env_diff_file, json_success, non_empty_path, non_empty_string,
        programmatic_error_body, resolve_typed_coverage_inputs, run_api_blocking,
        workspace_patterns_from_param,
    },
    fallback_policy::{
        CliFallbackReason, baseline_fallback_reason, filled, grouped_fallback_reason,
    },
    push_baseline, push_global, push_remote_extends, push_scope, push_str_flag, run_tool,
    validation_error_body,
};

/// Run `check_health` through the typed API when parameters map cleanly to the
/// programmatic contract, falling back to the CLI for CLI-only surfaces.
pub async fn run_health(binary: &str, params: HealthParams) -> Result<CallToolResult, McpError> {
    if requires_cli_fallback(&params) {
        let args = build_health_args(&params);
        return run_tool(binary, "check_health", &args).await;
    }

    let options = match health_options_from_params(&params) {
        Ok(options) => options,
        Err(msg) => return Ok(CallToolResult::error(vec![ContentBlock::text(msg)])),
    };

    let result = run_api_blocking("check_health", move || health_typed_value(options, None))
        .await?
        .map_or_else(
            |err| CallToolResult::error(vec![ContentBlock::text(programmatic_error_body(&err))]),
            |value| json_success(&value),
        );
    Ok(result)
}

/// The typed route's blocking body: resolve the coverage inputs with the CLI's
/// precedence (#2368), run the health analysis, and serialize it. `env`
/// supplies the environment coverage layer, with `None` reading the process
/// environment as [`run_health`] does. Production and the precedence tests both
/// call this, so the tests hold the shipping code rather than a parallel copy
/// of it.
fn health_typed_value(
    options: ComplexityOptions,
    env: Option<CoverageInputs>,
) -> Result<serde_json::Value, ProgrammaticError> {
    let options = with_resolved_coverage(options, env)?;
    run_api_health(&options).and_then(serialize_health_programmatic_json)
}

/// [`health_typed_value`] reached the way [`run_health`] reaches it: through
/// the CLI-fallback decision and the same parameter mapping, without the async
/// rmcp shell. `env` is injected so the precedence can be exercised without
/// mutating the process environment.
#[cfg(test)]
fn run_health_api_value_with_env(
    params: &HealthParams,
    env: Option<CoverageInputs>,
) -> Result<Option<serde_json::Value>, String> {
    if requires_cli_fallback(params) {
        return Ok(None);
    }

    let options = health_options_from_params(params)?;
    health_typed_value(options, env)
        .map(Some)
        .map_err(|err| programmatic_error_body(&err))
}

/// Fill the typed route's coverage inputs from the explicit parameters, the
/// environment, and the project config with the CLI's precedence (#2368).
fn with_resolved_coverage(
    mut options: ComplexityOptions,
    env: Option<CoverageInputs>,
) -> Result<ComplexityOptions, ProgrammaticError> {
    let explicit = CoverageInputs {
        coverage: options.coverage.take(),
        coverage_root: options.coverage_root.take(),
    };
    let resolved =
        resolve_typed_coverage_inputs(&options.analysis, explicit, env, "health.coverage_root")?;
    options.coverage = resolved.coverage;
    options.coverage_root = resolved.coverage_root;
    Ok(options)
}

/// Build CLI arguments for the `check_health` tool.
pub fn build_health_args(params: &HealthParams) -> Vec<String> {
    HealthArgsBuilder {
        args: vec![
            "health".to_string(),
            "--format".to_string(),
            "json".to_string(),
            "--quiet".to_string(),
            "--explain".to_string(),
        ],
        params,
    }
    .build()
}

struct HealthArgsBuilder<'a> {
    args: Vec<String>,
    params: &'a HealthParams,
}

impl HealthArgsBuilder<'_> {
    fn build(mut self) -> Vec<String> {
        self.push_global_scope();
        self.push_thresholds();
        self.push_sort_and_diff();
        self.push_analysis_sections();
        self.push_ownership();
        self.push_target_and_coverage_gates();
        self.push_score_and_severity();
        self.push_history();
        self.push_snapshot_and_baseline();
        self.push_coverage();
        self.push_runtime_coverage();
        push_str_flag(
            &mut self.args,
            "--group-by",
            self.params.group_by.as_deref(),
        );
        self.args
    }

    fn push_global_scope(&mut self) {
        push_global(
            &mut self.args,
            self.params.root.as_deref(),
            self.params.config.as_deref(),
            self.params.no_cache,
            self.params.threads,
        );
        push_remote_extends(&mut self.args, self.params.allow_remote_extends);
        super::push_type_aware(
            &mut self.args,
            self.params.type_aware,
            self.params.type_aware_projects.as_deref(),
            self.params.type_aware_require,
        );
        push_scope(
            &mut self.args,
            self.params.production,
            self.params.workspace.as_deref(),
        );
    }

    fn push_thresholds(&mut self) {
        if let Some(max_cyclomatic) = self.params.max_cyclomatic {
            self.args
                .extend(["--max-cyclomatic".to_string(), max_cyclomatic.to_string()]);
        }
        if let Some(max_cognitive) = self.params.max_cognitive {
            self.args
                .extend(["--max-cognitive".to_string(), max_cognitive.to_string()]);
        }
        if let Some(max_crap) = self.params.max_crap {
            self.args
                .extend(["--max-crap".to_string(), format!("{max_crap}")]);
        }
        if let Some(top) = self.params.top {
            self.args.extend(["--top".to_string(), top.to_string()]);
        }
    }

    fn push_sort_and_diff(&mut self) {
        push_str_flag(&mut self.args, "--sort", self.params.sort.as_deref());
        push_str_flag(
            &mut self.args,
            "--changed-since",
            self.params.changed_since.as_deref(),
        );
    }

    fn push_analysis_sections(&mut self) {
        if self.params.complexity == Some(true) {
            self.args.push("--complexity".to_string());
        }
        if self.params.complexity_breakdown == Some(true) {
            self.args.push("--complexity-breakdown".to_string());
        }
        if self.params.file_scores == Some(true) {
            self.args.push("--file-scores".to_string());
        }
        if self.params.css == Some(true) {
            self.args.push("--css".to_string());
        }
        if self.params.type_coupling == Some(true) {
            self.args.push("--type-coupling".to_string());
        }
    }

    fn push_ownership(&mut self) {
        let ownership_active =
            self.params.ownership == Some(true) || self.params.ownership_email_mode.is_some();
        if self.params.hotspots == Some(true) || ownership_active {
            self.args.push("--hotspots".to_string());
        }
        if ownership_active {
            self.args.push("--ownership".to_string());
        }
        if let Some(mode) = self.params.ownership_email_mode {
            self.args
                .extend(["--ownership-emails".to_string(), mode.as_cli().to_string()]);
        }
    }

    fn push_target_and_coverage_gates(&mut self) {
        if self.params.targets == Some(true) {
            self.args.push("--targets".to_string());
        }
        if self.params.coverage_gaps == Some(true) {
            self.args.push("--coverage-gaps".to_string());
        }
    }

    fn push_score_and_severity(&mut self) {
        if self.params.score == Some(true) {
            self.args.push("--score".to_string());
        }
        if let Some(min_score) = self.params.min_score {
            self.args
                .extend(["--min-score".to_string(), min_score.to_string()]);
        }
        push_str_flag(
            &mut self.args,
            "--min-severity",
            self.params.min_severity.as_deref(),
        );
    }

    fn push_history(&mut self) {
        push_str_flag(&mut self.args, "--since", self.params.since.as_deref());
        if let Some(min_commits) = self.params.min_commits {
            self.args
                .extend(["--min-commits".to_string(), min_commits.to_string()]);
        }
        push_str_flag(
            &mut self.args,
            "--churn-file",
            self.params.churn_file.as_deref(),
        );
    }

    fn push_snapshot_and_baseline(&mut self) {
        if let Some(ref path) = self.params.save_snapshot {
            if path.is_empty() {
                self.args.push("--save-snapshot".to_string());
            } else {
                self.args
                    .extend(["--save-snapshot".to_string(), path.clone()]);
            }
        }
        push_baseline(
            &mut self.args,
            self.params.baseline.as_deref(),
            self.params.save_baseline.as_deref(),
        );
        push_str_flag(
            &mut self.args,
            "--baseline-mode",
            self.params.baseline_mode.map(|mode| mode.as_cli()),
        );
        if self.params.trend == Some(true) {
            self.args.push("--trend".to_string());
        }
        push_str_flag(&mut self.args, "--effort", self.params.effort.as_deref());
        if self.params.summary == Some(true) {
            self.args.push("--summary".to_string());
        }
    }

    fn push_coverage(&mut self) {
        push_str_flag(
            &mut self.args,
            "--coverage",
            self.params.coverage.as_deref(),
        );
        push_str_flag(
            &mut self.args,
            "--coverage-root",
            self.params.coverage_root.as_deref(),
        );
        push_str_flag(
            &mut self.args,
            "--runtime-coverage",
            self.params.runtime_coverage.as_deref(),
        );
    }

    fn push_runtime_coverage(&mut self) {
        if let Some(min_invocations_hot) = self.params.min_invocations_hot {
            self.args.extend([
                "--min-invocations-hot".to_string(),
                min_invocations_hot.to_string(),
            ]);
        }
        if let Some(min_observation_volume) = self.params.min_observation_volume {
            self.args.extend([
                "--min-observation-volume".to_string(),
                min_observation_volume.to_string(),
            ]);
        }
        if let Some(low_traffic_threshold) = self.params.low_traffic_threshold {
            self.args.extend([
                "--low-traffic-threshold".to_string(),
                format!("{low_traffic_threshold}"),
            ]);
        }
    }
}

fn requires_cli_fallback(params: &HealthParams) -> bool {
    params.type_aware == Some(true)
        || params.type_coupling == Some(true)
        || params
            .type_aware_projects
            .as_ref()
            .is_some_and(|projects| !projects.is_empty())
        || params.type_aware_require.is_some()
        || cli_fallback_reason(params).is_some()
}

fn cli_fallback_reason(params: &HealthParams) -> Option<CliFallbackReason> {
    if params.min_score.is_some() {
        return Some(CliFallbackReason::HealthMinScoreGate);
    }
    if filled(params.min_severity.as_deref()) {
        return Some(CliFallbackReason::HealthMinSeverity);
    }
    if filled(params.churn_file.as_deref()) {
        return Some(CliFallbackReason::HealthChurnFile);
    }
    if params.save_snapshot.is_some() {
        return Some(CliFallbackReason::HealthSnapshot);
    }
    if let Some(reason) =
        baseline_fallback_reason(params.baseline.as_deref(), params.save_baseline.as_deref())
    {
        return Some(reason);
    }
    if params.trend == Some(true) {
        return Some(CliFallbackReason::HealthTrend);
    }
    if params.summary == Some(true) {
        return Some(CliFallbackReason::HealthSummary);
    }
    if filled(params.runtime_coverage.as_deref())
        || params.min_invocations_hot.is_some()
        || params.min_observation_volume.is_some()
        || params.low_traffic_threshold.is_some()
    {
        return Some(CliFallbackReason::HealthRuntimeCoverage);
    }
    grouped_fallback_reason(params.group_by.as_deref())
}

fn health_options_from_params(params: &HealthParams) -> Result<ComplexityOptions, String> {
    Ok(ComplexityOptions {
        analysis: AnalysisOptions {
            root: non_empty_path(params.root.as_deref()),
            config_path: non_empty_path(params.config.as_deref()),
            allow_remote_extends: params.allow_remote_extends.unwrap_or(false),
            no_cache: params.no_cache.unwrap_or(false),
            threads: params.threads,
            production: params.production.unwrap_or(false),
            production_override: params.production,
            changed_since: changed_since_from_param(params.changed_since.as_deref()),
            diff_file: env_diff_file(),
            workspace: workspace_patterns_from_param(params.workspace.as_deref()),
            explain: true,
            ..AnalysisOptions::default()
        },
        max_cyclomatic: params.max_cyclomatic,
        max_cognitive: params.max_cognitive,
        max_crap: params.max_crap,
        top: params.top,
        sort: complexity_sort_from_param(params.sort.as_deref())?,
        complexity_breakdown: params.complexity_breakdown.unwrap_or(false),
        complexity: params.complexity.unwrap_or(false),
        file_scores: params.file_scores.unwrap_or(false),
        coverage_gaps: params.coverage_gaps.unwrap_or(false),
        hotspots: params.hotspots.unwrap_or(false),
        ownership: params.ownership.unwrap_or(false),
        ownership_emails: params
            .ownership_email_mode
            .map(ownership_email_mode_from_param),
        targets: params.targets.unwrap_or(false),
        css: params.css.unwrap_or(false),
        css_deep: false,
        effort: target_effort_from_param(params.effort.as_deref())?,
        score: params.score.unwrap_or(false),
        since: non_empty_string(params.since.as_deref()),
        min_commits: params.min_commits,
        coverage: non_empty_path(params.coverage.as_deref()),
        coverage_root: non_empty_path(params.coverage_root.as_deref()),
        coverage_relocated: false,
    })
}

fn complexity_sort_from_param(value: Option<&str>) -> Result<ComplexitySort, String> {
    match value {
        None | Some("") | Some("cyclomatic") => Ok(ComplexitySort::Cyclomatic),
        Some("cognitive") => Ok(ComplexitySort::Cognitive),
        Some("lines") => Ok(ComplexitySort::Lines),
        Some("severity") => Ok(ComplexitySort::Severity),
        Some(value) => Err(validation_error_body(format!(
            "Invalid sort '{value}'. Valid values: cyclomatic, cognitive, lines, severity"
        ))),
    }
}

const fn ownership_email_mode_from_param(
    value: crate::params::EmailModeParam,
) -> OwnershipEmailMode {
    match value {
        crate::params::EmailModeParam::Raw => OwnershipEmailMode::Raw,
        crate::params::EmailModeParam::Handle => OwnershipEmailMode::Handle,
        crate::params::EmailModeParam::Anonymized => OwnershipEmailMode::Anonymized,
        crate::params::EmailModeParam::Hash => OwnershipEmailMode::Hash,
    }
}

fn target_effort_from_param(value: Option<&str>) -> Result<Option<TargetEffort>, String> {
    match value {
        None | Some("") => Ok(None),
        Some("low") => Ok(Some(TargetEffort::Low)),
        Some("medium") => Ok(Some(TargetEffort::Medium)),
        Some("high") => Ok(Some(TargetEffort::High)),
        Some(value) => Err(validation_error_body(format!(
            "Invalid effort '{value}'. Valid values: low, medium, high"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use rmcp::model::ContentBlock;

    use super::super::coverage_fixture::{CoverageFixture, branchy_finding};
    use super::*;

    #[test]
    fn api_path_splits_comma_list_workspace_like_the_cli() {
        let params = HealthParams {
            workspace: Some("web,admin".to_string()),
            ..HealthParams::default()
        };

        assert!(!requires_cli_fallback(&params));
        let options = health_options_from_params(&params).expect("options");
        assert_eq!(
            options.analysis.workspace,
            Some(vec!["web".to_string(), "admin".to_string()])
        );
    }

    #[test]
    fn api_path_maps_supported_health_params() {
        let params = HealthParams {
            root: Some(String::new()),
            config: Some(String::new()),
            max_cyclomatic: Some(9),
            max_cognitive: Some(8),
            max_crap: Some(24.5),
            top: Some(5),
            sort: Some("severity".to_string()),
            changed_since: Some("main".to_string()),
            complexity_breakdown: Some(true),
            complexity: Some(true),
            css: Some(true),
            file_scores: Some(true),
            hotspots: Some(true),
            ownership: Some(true),
            ownership_email_mode: Some(crate::params::EmailModeParam::Anonymized),
            targets: Some(true),
            coverage_gaps: Some(true),
            score: Some(true),
            since: Some("90d".to_string()),
            min_commits: Some(2),
            workspace: Some("apps/web".to_string()),
            production: Some(false),
            no_cache: Some(true),
            threads: Some(3),
            coverage: Some("coverage/coverage-final.json".to_string()),
            coverage_root: Some("/tmp/project".to_string()),
            effort: Some("high".to_string()),
            ..HealthParams::default()
        };

        assert!(!requires_cli_fallback(&params));
        let options = health_options_from_params(&params).expect("options");
        assert!(options.analysis.root.is_none());
        assert!(options.analysis.config_path.is_none());
        assert_eq!(options.analysis.changed_since.as_deref(), Some("main"));
        assert_eq!(
            options.analysis.workspace,
            Some(vec!["apps/web".to_string()])
        );
        assert_eq!(options.analysis.production_override, Some(false));
        assert!(options.analysis.no_cache);
        assert_eq!(options.analysis.threads, Some(3));
        assert_eq!(options.max_cyclomatic, Some(9));
        assert_eq!(options.max_cognitive, Some(8));
        assert_eq!(options.max_crap, Some(24.5));
        assert_eq!(options.top, Some(5));
        assert!(matches!(options.sort, ComplexitySort::Severity));
        assert!(options.complexity_breakdown);
        assert!(options.complexity);
        assert!(options.css);
        assert!(options.file_scores);
        assert!(options.hotspots);
        assert!(options.ownership);
        assert!(matches!(
            options.ownership_emails,
            Some(OwnershipEmailMode::Anonymized)
        ));
        assert!(options.targets);
        assert!(options.coverage_gaps);
        assert!(options.score);
        assert_eq!(options.since.as_deref(), Some("90d"));
        assert_eq!(options.min_commits, Some(2));
        assert_eq!(
            options.coverage,
            Some(PathBuf::from("coverage/coverage-final.json"))
        );
        assert_eq!(options.coverage_root, Some(PathBuf::from("/tmp/project")));
        assert!(matches!(options.effort, Some(TargetEffort::High)));
    }

    #[test]
    fn api_path_reuses_cli_validation_for_bad_sort_and_effort() {
        let bad_sort = HealthParams {
            sort: Some("weighted".to_string()),
            ..HealthParams::default()
        };
        let err = health_options_from_params(&bad_sort).expect_err("invalid sort");
        assert!(err.contains("Invalid sort"));

        let bad_effort = HealthParams {
            effort: Some("extreme".to_string()),
            ..HealthParams::default()
        };
        let err = health_options_from_params(&bad_effort).expect_err("invalid effort");
        assert!(err.contains("Invalid effort"));
    }

    #[test]
    fn cli_fallback_keeps_cli_only_health_surfaces() {
        for params in [
            HealthParams {
                min_score: Some(80.0),
                ..HealthParams::default()
            },
            HealthParams {
                min_severity: Some("high".to_string()),
                ..HealthParams::default()
            },
            HealthParams {
                churn_file: Some("churn.json".to_string()),
                ..HealthParams::default()
            },
            HealthParams {
                save_snapshot: Some(String::new()),
                ..HealthParams::default()
            },
            HealthParams {
                baseline: Some("baseline.json".to_string()),
                ..HealthParams::default()
            },
            HealthParams {
                save_baseline: Some("baseline.json".to_string()),
                ..HealthParams::default()
            },
            HealthParams {
                trend: Some(true),
                ..HealthParams::default()
            },
            HealthParams {
                summary: Some(true),
                ..HealthParams::default()
            },
            HealthParams {
                runtime_coverage: Some("coverage".to_string()),
                ..HealthParams::default()
            },
            HealthParams {
                min_invocations_hot: Some(1),
                ..HealthParams::default()
            },
            HealthParams {
                min_observation_volume: Some(1),
                ..HealthParams::default()
            },
            HealthParams {
                low_traffic_threshold: Some(0.1),
                ..HealthParams::default()
            },
            HealthParams {
                group_by: Some("owner".to_string()),
                ..HealthParams::default()
            },
        ] {
            assert!(requires_cli_fallback(&params));
        }
    }

    #[tokio::test]
    async fn run_health_api_path_returns_json_without_cli_binary() {
        let project = tempfile::tempdir().expect("project");
        std::fs::write(
            project.path().join("index.ts"),
            "export function score(value: number) {\n  if (value > 1) {\n    return value;\n  }\n  return 0;\n}\n",
        )
        .expect("write source");

        let result = run_health(
            "unused-binary-on-api-path",
            HealthParams {
                root: Some(project.path().display().to_string()),
                complexity: Some(true),
                no_cache: Some(true),
                ..HealthParams::default()
            },
        )
        .await
        .expect("mcp result");

        assert!(!result.is_error.unwrap_or(false));
        let [content] = result.content.as_slice() else {
            panic!("expected one content item");
        };
        let ContentBlock::Text(text) = content else {
            panic!("expected text content");
        };
        let json: serde_json::Value = serde_json::from_str(&text.text).expect("json");
        assert_eq!(json["kind"], "health");
    }

    /// #2368: `check_health` on the typed route scores CRAP from the
    /// configured Istanbul map instead of the reachability estimate. The
    /// config is discovered from `root` and its coverage path is
    /// project-relative.
    #[test]
    fn typed_route_config_only_coverage_scores_from_istanbul_map() {
        let uncovered = CoverageFixture::new(false);
        uncovered.write_health_config(&CoverageFixture::config_health_section());
        let json = typed_route_json(&health_params(&uncovered), CoverageInputs::default());
        let branchy = branchy_finding(&json["findings"]).expect("branchy above max_crap");
        assert_eq!(branchy["coverage_source"], "istanbul", "{json}");

        let covered = CoverageFixture::new(true);
        covered.write_health_config(&CoverageFixture::config_health_section());
        let json = typed_route_json(&health_params(&covered), CoverageInputs::default());
        assert!(
            branchy_finding(&json["findings"]).is_none(),
            "a covered map scores branchy below max_crap: {json}"
        );
    }

    /// #2368 precedence on `check_health`: the `coverage` parameter beats
    /// `FALLOW_COVERAGE`, which beats `health.coverage`, with each input
    /// resolving independently. `branchy` is reported only while the winning
    /// layer fails to match it.
    #[test]
    fn typed_route_parameter_beats_env_beats_config_coverage() {
        let fixture = CoverageFixture::new(true);
        fixture.write_stale_coverage("artifacts/stale.json");
        let branchy_reported = |params: &HealthParams, env: CoverageInputs| {
            branchy_finding(&typed_route_json(params, env)["findings"]).is_some()
        };

        fixture.write_health_config(
            r#"{"coverage":"artifacts/stale.json","coverageRoot":"/ci/workspace"}"#,
        );
        assert!(
            branchy_reported(&health_params(&fixture), CoverageInputs::default()),
            "a stale config map falls back to the estimate"
        );
        let env_map = CoverageInputs {
            coverage: Some(fixture.coverage_path()),
            coverage_root: None,
        };
        assert!(
            !branchy_reported(&health_params(&fixture), env_map),
            "FALLOW_COVERAGE beats health.coverage while health.coverageRoot still applies"
        );
        let stale_env = CoverageInputs {
            coverage: Some(fixture.path("artifacts/stale.json")),
            coverage_root: Some(PathBuf::from("/ci/workspace")),
        };
        assert!(
            branchy_reported(&health_params(&fixture), stale_env.clone()),
            "a stale env map falls back to the estimate"
        );
        let with_parameter = HealthParams {
            coverage: Some(fixture.coverage_path().display().to_string()),
            ..health_params(&fixture)
        };
        assert!(
            !branchy_reported(&with_parameter, stale_env),
            "the coverage parameter beats FALLOW_COVERAGE while FALLOW_COVERAGE_ROOT still applies"
        );

        fixture.write_health_config(
            r#"{"coverage":"artifacts/coverage-final.json","coverageRoot":"/wrong/root"}"#,
        );
        assert!(
            branchy_reported(&health_params(&fixture), CoverageInputs::default()),
            "a wrong config root strips nothing and matches nothing"
        );
        let env_root = CoverageInputs {
            coverage: None,
            coverage_root: Some(PathBuf::from("/ci/workspace")),
        };
        assert!(
            !branchy_reported(&health_params(&fixture), env_root),
            "FALLOW_COVERAGE_ROOT beats health.coverageRoot while health.coverage still applies"
        );
    }

    /// #2368: the configured map's existence is checked under `root`, and a
    /// missing one keeps the structured `FALLOW_INVALID_COVERAGE_PATH` error.
    #[test]
    fn typed_route_missing_config_coverage_is_structured_error() {
        let fixture = CoverageFixture::new(true);
        fixture.write_health_config(r#"{"coverage":"artifacts/missing.json"}"#);

        let error = typed_route_error(&health_params(&fixture));
        assert_eq!(error["code"], "FALLOW_INVALID_COVERAGE_PATH", "{error}");
        assert_eq!(error["exit_code"], 2, "{error}");
        let message = error["message"].as_str().expect("message");
        assert!(
            message.contains("missing.json") && message.contains(&fixture.root_string()),
            "the message names the path under root: {message}"
        );
    }

    /// #2368: a relative `health.coverageRoot` keeps the structured
    /// `FALLOW_INVALID_COVERAGE_ROOT` error and names the config key.
    #[test]
    fn typed_route_relative_config_coverage_root_is_structured_error() {
        let fixture = CoverageFixture::new(true);
        fixture.write_health_config(r#"{"coverageRoot":"src"}"#);

        let error = typed_route_error(&health_params(&fixture));
        assert_eq!(error["code"], "FALLOW_INVALID_COVERAGE_ROOT", "{error}");
        assert_eq!(error["context"], "health.coverageRoot", "{error}");
        let message = error["message"].as_str().expect("message");
        assert!(
            message.contains("--coverage-root expects an absolute path")
                && message.contains("got 'src'"),
            "{message}"
        );
    }

    fn typed_route_json(params: &HealthParams, env: CoverageInputs) -> serde_json::Value {
        run_health_api_value_with_env(params, Some(env))
            .expect("typed route result")
            .expect("typed route")
    }

    fn typed_route_error(params: &HealthParams) -> serde_json::Value {
        let body = run_health_api_value_with_env(params, Some(CoverageInputs::default()))
            .expect_err("the typed route rejects the inputs");
        serde_json::from_str(&body).expect("structured error body")
    }

    fn health_params(fixture: &CoverageFixture) -> HealthParams {
        HealthParams {
            root: Some(fixture.root_string()),
            max_crap: Some(10.0),
            complexity: Some(true),
            no_cache: Some(true),
            ..HealthParams::default()
        }
    }
}
