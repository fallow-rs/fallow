use crate::params::AuditParams;

use fallow_api::{
    AnalysisOptions, AuditGate, AuditOptions, CoverageInputs, ProgrammaticError,
    run_audit as run_audit_api, serialize_audit_programmatic_json,
};
use rmcp::ErrorData as McpError;
use rmcp::model::{CallToolResult, ContentBlock};

use super::{
    VALID_AUDIT_GATES,
    api_runtime::{
        changed_since_from_param, env_coverage_inputs, env_diff_file, json_success, non_empty_path,
        non_empty_string, programmatic_error_body, resolve_typed_coverage_inputs, run_api_blocking,
        workspace_patterns_from_param,
    },
    fallback_policy::{baseline_fallback_reason, filled, grouped_fallback_reason},
    push_global, push_remote_extends, push_scope, push_str_flag, run_tool, validation_error_body,
};

/// Run the `audit` tool through the typed API when parameters map cleanly to
/// the programmatic contract, falling back to the CLI for CLI-only surfaces.
pub async fn run_audit(binary: &str, params: AuditParams) -> Result<CallToolResult, McpError> {
    if !requires_cli_fallback(&params) {
        let options = match audit_options_from_params(&params) {
            Ok(options) => options,
            Err(msg) => return Ok(CallToolResult::error(vec![ContentBlock::text(msg)])),
        };
        let env = env_coverage_inputs();
        let result = run_api_blocking("audit", move || {
            let options = with_resolved_coverage(options, env)?;
            run_audit_api(&options).and_then(serialize_audit_programmatic_json)
        })
        .await?
        .map_or_else(
            |err| CallToolResult::error(vec![ContentBlock::text(programmatic_error_body(&err))]),
            |value| json_success(&value),
        );
        return Ok(result);
    }

    match build_audit_args(&params) {
        Ok(args) => run_tool(binary, "audit", &args).await,
        Err(msg) => Ok(CallToolResult::error(vec![ContentBlock::text(msg)])),
    }
}

pub fn run_audit_api_value(params: &AuditParams) -> Result<Option<serde_json::Value>, String> {
    run_audit_api_value_with_env(params, env_coverage_inputs())
}

/// [`run_audit_api_value`] with the environment coverage layer injected, so
/// the typed route can be exercised without mutating the process environment.
fn run_audit_api_value_with_env(
    params: &AuditParams,
    env: CoverageInputs,
) -> Result<Option<serde_json::Value>, String> {
    if requires_cli_fallback(params) {
        return Ok(None);
    }
    let options = audit_options_from_params(params)?;
    with_resolved_coverage(options, env)
        .and_then(|options| run_audit_api(&options))
        .and_then(serialize_audit_programmatic_json)
        .map(Some)
        .map_err(|err| programmatic_error_body(&err))
}

/// Fill the typed route's coverage inputs from the explicit parameters, the
/// environment, and the project config with the CLI's precedence (#2368).
fn with_resolved_coverage(
    mut options: AuditOptions,
    env: CoverageInputs,
) -> Result<AuditOptions, ProgrammaticError> {
    let explicit = CoverageInputs {
        coverage: options.coverage.take(),
        coverage_root: options.coverage_root.take(),
    };
    let resolved =
        resolve_typed_coverage_inputs(&options.analysis, explicit, env, "audit.coverageRoot")?;
    options.coverage = resolved.coverage;
    options.coverage_root = resolved.coverage_root;
    Ok(options)
}

/// Build CLI arguments for the `audit` tool.
pub fn build_audit_args(params: &AuditParams) -> Result<Vec<String>, String> {
    if let Some(ref gate) = params.gate
        && !VALID_AUDIT_GATES.contains(&gate.as_str())
    {
        return Err(validation_error_body(format!(
            "Invalid gate '{gate}'. Valid values: new-only, all"
        )));
    }

    let mut args = vec![
        "audit".to_string(),
        "--format".to_string(),
        "json".to_string(),
        "--quiet".to_string(),
        "--explain".to_string(),
    ];

    push_global(
        &mut args,
        params.root.as_deref(),
        params.config.as_deref(),
        params.no_cache,
        params.threads,
    );
    push_remote_extends(&mut args, params.allow_remote_extends);
    super::push_type_aware(
        &mut args,
        params.type_aware,
        params.type_aware_projects.as_deref(),
        params.type_aware_require,
    );
    push_str_flag(&mut args, "--base", params.base.as_deref());
    push_scope(&mut args, params.production, params.workspace.as_deref());
    push_audit_production_flags(&mut args, params);
    if params.css == Some(false) {
        args.push("--no-css".to_string());
    }
    if params.css_deep == Some(true) {
        args.push("--css-deep".to_string());
    } else if params.css_deep == Some(false) {
        args.push("--no-css-deep".to_string());
    }
    push_str_flag(&mut args, "--group-by", params.group_by.as_deref());
    push_str_flag(&mut args, "--gate", params.gate.as_deref());
    push_audit_baseline_flags(&mut args, params);
    if params.explain_skipped == Some(true) {
        args.push("--explain-skipped".to_string());
    }
    push_audit_coverage_flags(&mut args, params);

    Ok(args)
}

/// Push the per-analysis production-mode flags for the `audit` tool.
fn push_audit_production_flags(args: &mut Vec<String>, params: &AuditParams) {
    if params.production_dead_code == Some(true) {
        args.push("--production-dead-code".to_string());
    }
    if params.production_health == Some(true) {
        args.push("--production-health".to_string());
    }
    if params.production_dupes == Some(true) {
        args.push("--production-dupes".to_string());
    }
}

/// Push the per-sub-analysis baseline flags for the `audit` tool.
fn push_audit_baseline_flags(args: &mut Vec<String>, params: &AuditParams) {
    push_str_flag(
        args,
        "--dead-code-baseline",
        params.dead_code_baseline.as_deref(),
    );
    push_str_flag(args, "--health-baseline", params.health_baseline.as_deref());
    push_str_flag(
        args,
        "--baseline-mode",
        params.health_baseline_mode.map(|mode| mode.as_cli()),
    );
    push_str_flag(args, "--dupes-baseline", params.dupes_baseline.as_deref());
}

/// Push the coverage, entry-export, and runtime-coverage flags for `audit`.
fn push_audit_coverage_flags(args: &mut Vec<String>, params: &AuditParams) {
    if let Some(max_crap) = params.max_crap {
        args.extend(["--max-crap".to_string(), format!("{max_crap}")]);
    }
    push_str_flag(args, "--coverage", params.coverage.as_deref());
    push_str_flag(args, "--coverage-root", params.coverage_root.as_deref());
    if params.include_entry_exports == Some(true) {
        args.push("--include-entry-exports".to_string());
    }
    push_str_flag(
        args,
        "--runtime-coverage",
        params.runtime_coverage.as_deref(),
    );
    if let Some(min_invocations_hot) = params.min_invocations_hot {
        args.extend([
            "--min-invocations-hot".to_string(),
            format!("{min_invocations_hot}"),
        ]);
    }
}

fn requires_cli_fallback(params: &AuditParams) -> bool {
    params.type_aware == Some(true)
        || params
            .type_aware_projects
            .as_ref()
            .is_some_and(|projects| !projects.is_empty())
        || params.type_aware_require.is_some()
        || cli_fallback_reason(params).is_some()
}

fn cli_fallback_reason(params: &AuditParams) -> Option<&'static str> {
    let gate = params.gate.as_deref().unwrap_or("new-only");
    if !VALID_AUDIT_GATES.contains(&gate) {
        return Some("invalid gate");
    }
    baseline_fallback_reason(params.dead_code_baseline.as_deref(), None)
        .or_else(|| baseline_fallback_reason(params.health_baseline.as_deref(), None))
        .or_else(|| baseline_fallback_reason(params.dupes_baseline.as_deref(), None))
        .or_else(|| grouped_fallback_reason(params.group_by.as_deref()))
        .map(|_| "baseline or grouped output")
        .or_else(|| (params.explain_skipped == Some(true)).then_some("duplication skipped notes"))
        .or_else(|| filled(params.runtime_coverage.as_deref()).then_some("runtime coverage"))
}

fn audit_options_from_params(params: &AuditParams) -> Result<AuditOptions, String> {
    let gate = audit_gate_from_param(params.gate.as_deref())?;
    Ok(AuditOptions {
        analysis: AnalysisOptions {
            root: non_empty_path(params.root.as_deref()),
            config_path: non_empty_path(params.config.as_deref()),
            allow_remote_extends: params.allow_remote_extends.unwrap_or(false),
            no_cache: params.no_cache.unwrap_or(false),
            threads: params.threads,
            diff_file: env_diff_file(),
            production: params.production.unwrap_or(false),
            production_override: params.production,
            changed_since: changed_since_from_param(None),
            workspace: workspace_patterns_from_param(params.workspace.as_deref()),
            changed_workspaces: None,
            explain: true,
            type_aware: fallow_api::TypeAwareOptions::default(),
        },
        base: non_empty_string(params.base.as_deref()),
        production: params.production.unwrap_or(false),
        production_dead_code: params.production_dead_code,
        production_health: params.production_health,
        production_dupes: params.production_dupes,
        css: params.css,
        css_deep: params.css_deep,
        gate,
        max_crap: params.max_crap,
        coverage: non_empty_path(params.coverage.as_deref()),
        coverage_root: non_empty_path(params.coverage_root.as_deref()),
        include_entry_exports: params.include_entry_exports.unwrap_or(false),
        runtime_coverage: non_empty_path(params.runtime_coverage.as_deref()),
        min_invocations_hot: params.min_invocations_hot.unwrap_or(100),
    })
}

fn audit_gate_from_param(value: Option<&str>) -> Result<AuditGate, String> {
    match value.unwrap_or("new-only") {
        "new-only" => Ok(AuditGate::NewOnly),
        "all" => Ok(AuditGate::All),
        other => Err(validation_error_body(format!(
            "Invalid gate '{other}'. Valid values: new-only, all"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use std::process::Command;

    use rmcp::model::ContentBlock;

    use super::super::coverage_fixture::{COVERAGE_ROOT, CoverageFixture, branchy_finding};
    use super::*;

    #[test]
    fn default_new_only_audit_uses_programmatic_api_route() {
        let params = AuditParams::default();
        assert!(!requires_cli_fallback(&params));
        let options = audit_options_from_params(&params).expect("audit options");
        assert_eq!(options.gate, AuditGate::NewOnly);
    }

    /// #2368: the typed route honors `health.coverage` / `health.coverageRoot`
    /// exactly like the CLI fallback, so config-only coverage yields the same
    /// verdict and attribution as the explicit parameters. The config path is
    /// passed explicitly and the coverage path is project-relative, which the
    /// MCP process (whose cwd is not the project) must resolve against `root`.
    #[test]
    fn typed_route_config_only_coverage_matches_explicit_parameters() {
        let fixture = CoverageFixture::new(true);
        let config = fixture.write_health_config(&CoverageFixture::config_health_section());
        let params = || AuditParams {
            root: Some(fixture.root_string()),
            config: Some(config.display().to_string()),
            base: Some("HEAD~1".to_string()),
            max_crap: Some(10.0),
            no_cache: Some(true),
            ..AuditParams::default()
        };

        let from_config = run_audit_api_value_with_env(&params(), CoverageInputs::default())
            .expect("config-only typed route result")
            .expect("typed route");
        assert_eq!(from_config["verdict"], "pass", "{from_config}");

        let explicit = run_audit_api_value_with_env(
            &AuditParams {
                coverage: Some(fixture.coverage_path().display().to_string()),
                coverage_root: Some(COVERAGE_ROOT.to_string()),
                ..params()
            },
            CoverageInputs::default(),
        )
        .expect("explicit typed route result")
        .expect("typed route");
        assert_eq!(explicit["verdict"], "pass", "{explicit}");
        assert_eq!(
            from_config["complexity"]["findings"],
            explicit["complexity"]["findings"]
        );
        assert_eq!(from_config["attribution"], explicit["attribution"]);
    }

    /// #2368: a discovered `.fallowrc.json` feeds the typed route too, and the
    /// finding it produces is scored from the map (`coverage_source` istanbul)
    /// rather than the reachability estimate.
    #[test]
    fn typed_route_discovered_config_coverage_reports_istanbul_source() {
        let fixture = CoverageFixture::new(false);
        fixture.write_health_config(&CoverageFixture::config_health_section());

        let json = run_audit_api_value_with_env(&gated_params(&fixture), CoverageInputs::default())
            .expect("typed route result")
            .expect("typed route");

        assert_eq!(json["verdict"], "fail", "{json}");
        let branchy =
            branchy_finding(&json["complexity"]["findings"]).expect("branchy above max_crap");
        assert_eq!(branchy["coverage_source"], "istanbul", "{branchy}");
    }

    /// #2368 precedence: the `coverage` parameter beats `FALLOW_COVERAGE`,
    /// which beats `health.coverage`, and each input resolves independently
    /// (an env map pairs with the config root, an env root with the config
    /// map). The losing layer points at a map recorded for a different file
    /// or under the wrong prefix, so only the winning layer lets `branchy`
    /// score below `max_crap`.
    #[test]
    fn typed_route_parameter_beats_env_beats_config_coverage() {
        let fixture = CoverageFixture::new(true);
        fixture.write_stale_coverage("artifacts/stale.json");
        let verdict = |params: &AuditParams, env: CoverageInputs| {
            let json = run_audit_api_value_with_env(params, env)
                .expect("typed route result")
                .expect("typed route");
            json["verdict"].as_str().map(str::to_string)
        };

        fixture.write_health_config(
            r#"{"coverage":"artifacts/stale.json","coverageRoot":"/ci/workspace"}"#,
        );
        assert_eq!(
            verdict(&gated_params(&fixture), CoverageInputs::default()).as_deref(),
            Some("fail"),
            "a stale config map falls back to the estimate"
        );
        let env_map = CoverageInputs {
            coverage: Some(fixture.coverage_path()),
            coverage_root: None,
        };
        assert_eq!(
            verdict(&gated_params(&fixture), env_map).as_deref(),
            Some("pass"),
            "FALLOW_COVERAGE beats health.coverage while health.coverageRoot still applies"
        );
        let stale_env = CoverageInputs {
            coverage: Some(fixture.path("artifacts/stale.json")),
            coverage_root: Some(std::path::PathBuf::from(COVERAGE_ROOT)),
        };
        assert_eq!(
            verdict(&gated_params(&fixture), stale_env.clone()).as_deref(),
            Some("fail"),
            "a stale env map falls back to the estimate"
        );
        let with_parameter = AuditParams {
            coverage: Some(fixture.coverage_path().display().to_string()),
            ..gated_params(&fixture)
        };
        assert_eq!(
            verdict(&with_parameter, stale_env).as_deref(),
            Some("pass"),
            "the coverage parameter beats FALLOW_COVERAGE while FALLOW_COVERAGE_ROOT still applies"
        );

        fixture.write_health_config(
            r#"{"coverage":"artifacts/coverage-final.json","coverageRoot":"/wrong/root"}"#,
        );
        assert_eq!(
            verdict(&gated_params(&fixture), CoverageInputs::default()).as_deref(),
            Some("fail"),
            "a wrong config root strips nothing and matches nothing"
        );
        let env_root = CoverageInputs {
            coverage: None,
            coverage_root: Some(std::path::PathBuf::from(COVERAGE_ROOT)),
        };
        assert_eq!(
            verdict(&gated_params(&fixture), env_root).as_deref(),
            Some("pass"),
            "FALLOW_COVERAGE_ROOT beats health.coverageRoot while health.coverage still applies"
        );
    }

    /// #2368: a configured map that does not exist is the structured
    /// `FALLOW_INVALID_COVERAGE_PATH` error, checked under `root` rather than
    /// under the MCP server's working directory.
    #[test]
    fn typed_route_missing_config_coverage_is_structured_error() {
        let fixture = CoverageFixture::new(true);
        fixture.write_health_config(r#"{"coverage":"artifacts/missing.json"}"#);

        let error = typed_route_error(&gated_params(&fixture));
        assert_eq!(error["code"], "FALLOW_INVALID_COVERAGE_PATH", "{error}");
        assert_eq!(error["exit_code"], 2, "{error}");
        let message = error["message"].as_str().expect("message");
        assert!(
            message.contains("missing.json") && message.contains(&fixture.root_string()),
            "the message names the path under root: {message}"
        );
    }

    /// #2368: a relative `health.coverageRoot` is the structured
    /// `FALLOW_INVALID_COVERAGE_ROOT` error whose context names the config
    /// key that supplied it.
    #[test]
    fn typed_route_relative_config_coverage_root_is_structured_error() {
        let fixture = CoverageFixture::new(true);
        fixture.write_health_config(r#"{"coverageRoot":"src"}"#);

        let error = typed_route_error(&gated_params(&fixture));
        assert_eq!(error["code"], "FALLOW_INVALID_COVERAGE_ROOT", "{error}");
        assert_eq!(error["context"], "health.coverageRoot", "{error}");
        assert_eq!(error["exit_code"], 2, "{error}");
        let message = error["message"].as_str().expect("message");
        assert!(
            message.contains("--coverage-root expects an absolute path")
                && message.contains("got 'src'"),
            "{message}"
        );
    }

    fn gated_params(fixture: &CoverageFixture) -> AuditParams {
        AuditParams {
            root: Some(fixture.root_string()),
            base: Some("HEAD~1".to_string()),
            max_crap: Some(10.0),
            no_cache: Some(true),
            ..AuditParams::default()
        }
    }

    fn typed_route_error(params: &AuditParams) -> serde_json::Value {
        let body = run_audit_api_value_with_env(params, CoverageInputs::default())
            .expect_err("the typed route rejects the inputs");
        serde_json::from_str(&body).expect("structured error body")
    }

    #[test]
    fn gate_all_audit_uses_programmatic_api_route() {
        let params = AuditParams {
            gate: Some("all".to_string()),
            ..AuditParams::default()
        };
        assert!(!requires_cli_fallback(&params));
        let options = audit_options_from_params(&params).expect("audit options");
        assert_eq!(options.gate, AuditGate::All);
        assert!(options.analysis.explain);
    }

    #[test]
    fn cli_only_audit_surfaces_keep_fallback() {
        let baseline = AuditParams {
            gate: Some("all".to_string()),
            dead_code_baseline: Some("baseline.json".to_string()),
            ..AuditParams::default()
        };
        let grouped = AuditParams {
            gate: Some("all".to_string()),
            group_by: Some("owner".to_string()),
            ..AuditParams::default()
        };
        let runtime = AuditParams {
            gate: Some("all".to_string()),
            runtime_coverage: Some("coverage".to_string()),
            ..AuditParams::default()
        };

        assert!(requires_cli_fallback(&baseline));
        assert!(requires_cli_fallback(&grouped));
        assert!(requires_cli_fallback(&runtime));
    }

    #[test]
    fn audit_args_forward_health_baseline_mode() {
        let params = AuditParams {
            health_baseline: Some("health.json".to_string()),
            health_baseline_mode: Some(crate::params::BaselineModeParam::Identity),
            ..AuditParams::default()
        };
        let args = build_audit_args(&params).expect("args");
        assert!(args.contains(&"--health-baseline".to_string()));
        assert!(args.contains(&"--baseline-mode".to_string()));
        assert!(args.contains(&"identity".to_string()));

        let omitted = build_audit_args(&AuditParams::default()).expect("args");
        assert!(!omitted.contains(&"--baseline-mode".to_string()));
    }

    #[tokio::test]
    async fn run_audit_gate_all_api_path_returns_json_without_cli_binary() {
        let project = audit_fixture();

        let result = run_audit(
            "unused-binary-on-api-path",
            AuditParams {
                root: Some(project.path().display().to_string()),
                base: Some("HEAD".to_string()),
                gate: Some("all".to_string()),
                no_cache: Some(true),
                ..AuditParams::default()
            },
        )
        .await
        .expect("api result");

        assert_eq!(result.is_error, Some(false));
        let text = match &result.content[0] {
            ContentBlock::Text(text) => &text.text,
            _ => panic!("expected text content"),
        };
        let json: serde_json::Value = serde_json::from_str(text).expect("json");
        assert_eq!(json["kind"], "audit");
        assert_eq!(json["command"], "audit");
        assert!(json["dead_code"].is_object());
    }

    #[tokio::test]
    async fn run_audit_default_new_only_api_path_marks_introduced_without_cli_binary() {
        let project = audit_fixture();

        let result = run_audit(
            "unused-binary-on-api-path",
            AuditParams {
                root: Some(project.path().display().to_string()),
                base: Some("HEAD".to_string()),
                no_cache: Some(true),
                ..AuditParams::default()
            },
        )
        .await
        .expect("api result");

        assert_eq!(result.is_error, Some(false));
        let text = match &result.content[0] {
            ContentBlock::Text(text) => &text.text,
            _ => panic!("expected text content"),
        };
        let json: serde_json::Value = serde_json::from_str(text).expect("json");
        assert_eq!(json["kind"], "audit");
        assert_eq!(json["attribution"]["gate"], "new-only");
        assert_eq!(json["attribution"]["dead_code_introduced"], 1);
        assert_eq!(json["dead_code"]["unused_files"][0]["introduced"], true);
    }

    #[test]
    fn audit_api_value_preserves_styling_gate_parity() {
        let project = audit_styling_fixture();
        std::fs::write(
            project.path().join("src/styles.css"),
            "#app .legacy .title { color: red; }\n.plain { color: blue; }\n",
        )
        .expect("write inherited-only change");

        let all = run_audit_api_value(&AuditParams {
            root: Some(project.path().display().to_string()),
            base: Some("HEAD".to_string()),
            gate: Some("all".to_string()),
            no_cache: Some(true),
            ..AuditParams::default()
        })
        .expect("all-gate MCP API value")
        .expect("typed API path");
        assert_eq!(all["verdict"], "fail");

        let new_only = run_audit_api_value(&AuditParams {
            root: Some(project.path().display().to_string()),
            base: Some("HEAD".to_string()),
            no_cache: Some(true),
            ..AuditParams::default()
        })
        .expect("new-only MCP API value")
        .expect("typed API path");
        assert_eq!(new_only["verdict"], "pass");
        assert_eq!(new_only["attribution"]["styling_introduced"], 0);
        assert_eq!(new_only["attribution"]["styling_inherited"], 1);
        assert_eq!(new_only["attribution"]["duplication_demoted"], 0);
        assert_eq!(
            new_only["complexity"]["styling_findings"][0]["introduced"],
            false
        );

        std::fs::write(
            project.path().join("src/styles.css"),
            "#app .legacy .title { color: red; }\n.plain { color: blue; }\n#app .introduced .title { color: green; }\n",
        )
        .expect("write introduced styling change");
        let introduced = run_audit_api_value(&AuditParams {
            root: Some(project.path().display().to_string()),
            base: Some("HEAD".to_string()),
            no_cache: Some(true),
            ..AuditParams::default()
        })
        .expect("introduced MCP API value")
        .expect("typed API path");
        assert_eq!(introduced["verdict"], "fail");
        assert_eq!(introduced["attribution"]["styling_introduced"], 1);
        assert_eq!(introduced["attribution"]["styling_inherited"], 1);
        assert!(
            introduced["complexity"]["styling_findings"]
                .as_array()
                .expect("styling findings")
                .iter()
                .any(|finding| finding["line"] == 3 && finding["introduced"] == true)
        );
    }

    fn audit_fixture() -> tempfile::TempDir {
        let project = tempfile::tempdir().expect("project");
        std::fs::create_dir_all(project.path().join("src")).expect("create src");
        std::fs::write(
            project.path().join("package.json"),
            r#"{"name":"audit-api","type":"module","main":"src/index.ts"}"#,
        )
        .expect("write package");
        std::fs::write(
            project.path().join("src/index.ts"),
            "console.log('entry');\n",
        )
        .expect("write entry");
        git(project.path(), &["init"]);
        git(project.path(), &["add", "."]);
        git(
            project.path(),
            &[
                "-c",
                "user.email=test@example.com",
                "-c",
                "user.name=Test",
                "-c",
                "commit.gpgsign=false",
                "commit",
                "-m",
                "initial",
            ],
        );
        std::fs::write(
            project.path().join("src/feature.ts"),
            "export const unused = 1;\n",
        )
        .expect("write changed source");
        project
    }

    fn audit_styling_fixture() -> tempfile::TempDir {
        let project = tempfile::tempdir().expect("project");
        std::fs::create_dir_all(project.path().join("src")).expect("create src");
        std::fs::write(
            project.path().join("package.json"),
            r#"{"name":"audit-mcp-styling","type":"module","main":"src/index.ts"}"#,
        )
        .expect("write package");
        std::fs::write(
            project.path().join(".fallowrc.json"),
            r#"{"rules":{"css-selector-complexity":"error"}}"#,
        )
        .expect("write config");
        std::fs::write(
            project.path().join("src/index.ts"),
            "console.log('entry');\n",
        )
        .expect("write entry");
        std::fs::write(
            project.path().join("src/styles.css"),
            "#app .legacy .title { color: red; }\n",
        )
        .expect("write inherited styling");
        git(project.path(), &["init"]);
        git(project.path(), &["add", "."]);
        git(
            project.path(),
            &[
                "-c",
                "user.email=test@example.com",
                "-c",
                "user.name=Test",
                "-c",
                "commit.gpgsign=false",
                "commit",
                "-m",
                "initial",
            ],
        );
        project
    }

    fn git(root: &std::path::Path, args: &[&str]) {
        let status = Command::new("git")
            .args(args)
            .current_dir(root)
            .status()
            .expect("git command");
        assert!(status.success(), "git {args:?} failed");
    }
}
