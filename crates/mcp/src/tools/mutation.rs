//! Read-only mutation tools. MCP plans mutants and inspects one mutant, but
//! never runs the tests of the project: a run is a
//! host permission decision, so it stays with the `fallow mutation` CLI.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use fallow_api::mutation::Operator;
use fallow_api::{MutationRunOptions, MutationRunScope, plan_mutation};
use rmcp::ErrorData as McpError;
use rmcp::model::{CallToolResult, ContentBlock};

use crate::params::{MutationInspectParams, MutationPlanParams};

use super::api_runtime::{
    EnvScope, call_cancellation, json_success, non_empty_path, non_empty_string,
    programmatic_error_body, run_api_blocking, shared_analysis_options, shared_params,
};
use super::{push_global, push_remote_extends, run_tool, validation_error_body};

/// Build the static mutation plan through the typed API. Without the
/// `operators` param, `plan_mutation` applies the `mutation.operators` config
/// key, as `fallow mutation --plan` does.
pub async fn run_mutation_plan(
    _binary: &str,
    params: MutationPlanParams,
) -> Result<CallToolResult, McpError> {
    let cancellation = call_cancellation();
    let options = match mutation_plan_options(&params, Some(Arc::clone(&cancellation))) {
        Ok(options) => options,
        Err(message) => return Ok(CallToolResult::error(vec![ContentBlock::text(message)])),
    };
    let result = run_api_blocking("mutation_plan", cancellation, move || {
        plan_mutation(&options).and_then(|plan| {
            fallow_output::serialize_mutation_plan_json_output(plan).map_err(|error| {
                fallow_api::ProgrammaticError::new(
                    format!("failed to serialize the mutation plan: {error}"),
                    2,
                )
                .with_code("FALLOW_MUTATION_PLAN_FAILED")
            })
        })
    })
    .await?
    .map_or_else(
        |error| CallToolResult::error(vec![ContentBlock::text(programmatic_error_body(&error))]),
        |value| json_success(&value),
    );
    Ok(result)
}

/// Inspect one mutant of a saved report through `fallow mutation inspect`.
/// Without `report`, the CLI reads the report that the last run saved in the
/// cache directory of the project.
pub async fn run_mutation_inspect(
    binary: &str,
    params: MutationInspectParams,
) -> Result<CallToolResult, McpError> {
    match build_mutation_inspect_args(&params) {
        Ok(args) => run_tool(binary, "mutation_inspect", &args).await,
        Err(message) => Ok(CallToolResult::error(vec![ContentBlock::text(message)])),
    }
}

/// Map the plan parameters to the typed plan options, with the same scope
/// rules as the CLI: changed code by default, `all` or `paths` otherwise.
///
/// # Errors
///
/// Returns a validation error body for conflicting scopes, an empty path, or
/// an unknown operator name.
pub fn mutation_plan_options(
    params: &MutationPlanParams,
    cancellation: Option<Arc<AtomicBool>>,
) -> Result<MutationRunOptions, String> {
    Ok(MutationRunOptions {
        // The plan scope comes from the tool's own `changed_since`, `paths`
        // and `all` params, not from the inherited environment.
        analysis: shared_analysis_options(shared_params!(params), EnvScope::Own, cancellation),
        scope: plan_scope(params)?,
        operators: plan_operators(params.operators.as_deref())?,
        max_mutants: params.max_mutants,
        ..MutationRunOptions::default()
    })
}

fn plan_scope(params: &MutationPlanParams) -> Result<MutationRunScope, String> {
    let all = params.all == Some(true);
    let base = non_empty_string(params.changed_since.as_deref());
    let selected = [all, params.paths.is_some(), base.is_some()]
        .into_iter()
        .filter(|selected| *selected)
        .count();
    if selected > 1 {
        return Err(validation_error_body(
            "all, paths and changed_since are mutually exclusive mutation scopes",
        ));
    }
    if all {
        return Ok(MutationRunScope::All);
    }
    let Some(paths) = &params.paths else {
        return Ok(MutationRunScope::Changed { base });
    };
    if paths.is_empty() {
        return Err(validation_error_body(
            "paths must contain at least one entry",
        ));
    }
    if paths.iter().any(|path| path.trim().is_empty()) {
        return Err(validation_error_body("paths entries must not be empty"));
    }
    Ok(MutationRunScope::Paths(
        paths.iter().map(PathBuf::from).collect(),
    ))
}

fn plan_operators(names: Option<&[String]>) -> Result<Option<Vec<Operator>>, String> {
    let Some(names) = names else {
        return Ok(None);
    };
    if names.is_empty() {
        return Err(validation_error_body(
            "operators must contain at least one entry",
        ));
    }
    names
        .iter()
        .map(|name| {
            Operator::from_name(name.trim()).ok_or_else(|| {
                let valid: Vec<String> = Operator::ALL.iter().map(ToString::to_string).collect();
                validation_error_body(format!(
                    "unknown mutation operator `{name}`; valid operators: {}",
                    valid.join(", ")
                ))
            })
        })
        .collect::<Result<Vec<_>, _>>()
        .map(Some)
}

/// Build the CLI arguments of `mutation_inspect`.
///
/// # Errors
///
/// Returns a validation error body when `mutant_id` or a given `report` is
/// empty.
pub fn build_mutation_inspect_args(params: &MutationInspectParams) -> Result<Vec<String>, String> {
    let mutant_id = params.mutant_id.trim();
    if mutant_id.is_empty() {
        return Err(validation_error_body("mutant_id must not be empty"));
    }
    if params
        .report
        .as_deref()
        .is_some_and(|report| report.trim().is_empty())
    {
        return Err(validation_error_body("report must not be empty"));
    }
    let mut args = vec![
        "mutation".to_owned(),
        "--format".to_owned(),
        "json".to_owned(),
        "--quiet".to_owned(),
    ];
    push_global(
        &mut args,
        params.root.as_deref(),
        params.config.as_deref(),
        None,
        None,
    );
    push_remote_extends(&mut args, params.allow_remote_extends);
    args.extend(["inspect".to_owned(), mutant_id.to_owned()]);
    if let Some(report) = &params.report {
        args.extend([
            "--report".to_owned(),
            report_path(params.root.as_deref(), report.trim()),
        ]);
    }
    Ok(args)
}

/// A relative report path resolves against `root`, as the plan `paths` do.
/// Without a root, the working directory of the MCP server applies.
fn report_path(root: Option<&str>, report: &str) -> String {
    match non_empty_path(root) {
        Some(root) if Path::new(report).is_relative() => {
            root.join(report).to_string_lossy().into_owned()
        }
        _ => report.to_owned(),
    }
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "tests unwrap to keep assertions short")]
mod tests {
    use super::*;

    fn plan_params(value: serde_json::Value) -> MutationPlanParams {
        serde_json::from_value(value).unwrap()
    }

    fn error_message(body: &str) -> String {
        let value: serde_json::Value = serde_json::from_str(body).unwrap();
        assert_eq!(value["exit_code"], 2, "{body}");
        value["message"].as_str().unwrap().to_owned()
    }

    #[test]
    fn plan_scope_follows_the_cli_scope_rules() {
        let cases: [(serde_json::Value, MutationRunScope); 4] = [
            (
                serde_json::json!({}),
                MutationRunScope::Changed { base: None },
            ),
            (
                serde_json::json!({ "changed_since": "origin/main" }),
                MutationRunScope::Changed {
                    base: Some("origin/main".to_owned()),
                },
            ),
            (serde_json::json!({ "all": true }), MutationRunScope::All),
            (
                serde_json::json!({ "paths": ["src/math.ts", "lib"] }),
                MutationRunScope::Paths(vec![PathBuf::from("src/math.ts"), PathBuf::from("lib")]),
            ),
        ];
        for (input, expected) in cases {
            let options = mutation_plan_options(&plan_params(input.clone()), None).unwrap();
            assert_eq!(options.scope, expected, "input {input}");
        }
    }

    #[test]
    fn plan_options_carry_the_limit_operators_and_project_flags() {
        let options = mutation_plan_options(
            &plan_params(serde_json::json!({
                "root": "/repo",
                "config": "/repo/.fallowrc.json",
                "allow_remote_extends": true,
                "no_cache": true,
                "threads": 2,
                "max_mutants": 25,
                "operators": ["relational-operator", "return-value"],
            })),
            None,
        )
        .unwrap();
        assert_eq!(options.max_mutants, Some(25));
        assert_eq!(
            options.operators,
            Some(vec![Operator::RelationalOperator, Operator::ReturnValue])
        );
        assert_eq!(options.analysis.root, Some(PathBuf::from("/repo")));
        assert_eq!(
            options.analysis.config_path,
            Some(PathBuf::from("/repo/.fallowrc.json"))
        );
        assert!(options.analysis.allow_remote_extends);
        assert!(options.analysis.no_cache);
        assert_eq!(options.analysis.threads, Some(2));
        assert!(!options.allow_side_effects, "MCP never keeps sink mutants");
    }

    #[test]
    fn plan_options_refuse_conflicting_or_malformed_input() {
        let cases = [
            (
                serde_json::json!({ "all": true, "paths": ["src"] }),
                "all, paths and changed_since are mutually exclusive",
            ),
            (
                serde_json::json!({ "all": true, "changed_since": "main" }),
                "all, paths and changed_since are mutually exclusive",
            ),
            (
                serde_json::json!({ "paths": ["src"], "changed_since": "main" }),
                "all, paths and changed_since are mutually exclusive",
            ),
            (
                serde_json::json!({ "paths": [] }),
                "paths must contain at least one entry",
            ),
            (
                serde_json::json!({ "paths": ["src", " "] }),
                "paths entries must not be empty",
            ),
            (
                serde_json::json!({ "operators": [] }),
                "operators must contain at least one entry",
            ),
            (
                serde_json::json!({ "operators": ["relational-operator", "swap-everything"] }),
                "unknown mutation operator `swap-everything`",
            ),
        ];
        for (input, expected) in cases {
            let error = mutation_plan_options(&plan_params(input.clone()), None)
                .err()
                .unwrap_or_else(|| panic!("input {input} must be refused"));
            let message = error_message(&error);
            assert!(
                message.contains(expected),
                "input {input}: expected `{expected}` in `{message}`"
            );
        }
    }

    #[test]
    fn unknown_operator_refusal_lists_the_valid_names() {
        let error = mutation_plan_options(
            &plan_params(serde_json::json!({
                "operators": ["relational"],
            })),
            None,
        )
        .unwrap_err();
        let message = error_message(&error);
        for name in ["relational-operator", "removed-call", "optional-chaining"] {
            assert!(message.contains(name), "{message}");
        }
    }

    #[test]
    fn plan_ignores_an_explicit_false_all_flag() {
        let options = mutation_plan_options(
            &plan_params(serde_json::json!({
                "all": false,
                "changed_since": "main",
            })),
            None,
        )
        .unwrap();
        assert_eq!(
            options.scope,
            MutationRunScope::Changed {
                base: Some("main".to_owned())
            }
        );
    }

    fn inspect_params(value: serde_json::Value) -> MutationInspectParams {
        serde_json::from_value(value).unwrap()
    }

    #[test]
    fn inspect_forwards_the_mutant_and_report_to_the_cli() {
        let args = build_mutation_inspect_args(&inspect_params(serde_json::json!({
            "mutant_id": " m_1a2b3c ",
            "report": "fallow-mutation.json",
            "root": "/repo",
            "config": "/repo/.fallowrc.json",
            "allow_remote_extends": true,
        })))
        .unwrap();
        assert_eq!(
            args,
            [
                "mutation",
                "--format",
                "json",
                "--quiet",
                "--root",
                "/repo",
                "--config",
                "/repo/.fallowrc.json",
                "--allow-remote-extends",
                "inspect",
                "m_1a2b3c",
                "--report",
                "/repo/fallow-mutation.json",
            ]
        );
    }

    #[test]
    fn inspect_resolves_a_relative_report_against_the_root() {
        let report_arg = |value: serde_json::Value| {
            let args = build_mutation_inspect_args(&inspect_params(value)).unwrap();
            args.last().unwrap().clone()
        };
        let absolute = std::env::temp_dir().join("plan.json");
        let absolute = absolute.to_string_lossy().into_owned();
        assert_eq!(
            report_arg(serde_json::json!({
                "mutant_id": "m_1",
                "report": "reports/plan.json",
                "root": "/repo",
            })),
            std::path::Path::new("/repo")
                .join("reports/plan.json")
                .to_string_lossy()
        );
        assert_eq!(
            report_arg(serde_json::json!({
                "mutant_id": "m_1",
                "report": absolute,
                "root": "/repo",
            })),
            absolute,
            "an absolute report path stays as it is"
        );
        assert_eq!(
            report_arg(serde_json::json!({
                "mutant_id": "m_1",
                "report": " reports/plan.json ",
                "root": "/repo",
            })),
            std::path::Path::new("/repo")
                .join("reports/plan.json")
                .to_string_lossy(),
            "outer whitespace is trimmed, as for mutant_id"
        );
        assert_eq!(
            report_arg(serde_json::json!({
                "mutant_id": "m_1",
                "report": "plan.json",
            })),
            "plan.json",
            "without a root the server working directory applies"
        );
    }

    #[test]
    fn inspect_without_a_report_lets_the_cli_read_the_last_run_report() {
        let args = build_mutation_inspect_args(&inspect_params(serde_json::json!({
            "mutant_id": "m_1a2b3c",
            "root": "/repo",
            "config": "/repo/.fallowrc.json",
        })))
        .unwrap();
        assert_eq!(
            args,
            [
                "mutation",
                "--format",
                "json",
                "--quiet",
                "--root",
                "/repo",
                "--config",
                "/repo/.fallowrc.json",
                "inspect",
                "m_1a2b3c",
            ]
        );
    }

    #[test]
    fn inspect_refuses_an_empty_mutant_or_report() {
        for (input, expected) in [
            (
                serde_json::json!({ "mutant_id": "  ", "report": "r.json" }),
                "mutant_id must not be empty",
            ),
            (
                serde_json::json!({ "mutant_id": "m_1", "report": " " }),
                "report must not be empty",
            ),
        ] {
            let error = build_mutation_inspect_args(&inspect_params(input.clone()))
                .err()
                .unwrap_or_else(|| panic!("input {input} must be refused"));
            assert!(error_message(&error).contains(expected), "{error}");
        }
    }

    fn fixture_root() -> String {
        let manifest_dir = std::path::PathBuf::from(
            std::env::var_os("CARGO_MANIFEST_DIR").expect("cargo sets CARGO_MANIFEST_DIR"),
        );
        let workspace = manifest_dir
            .parent()
            .and_then(std::path::Path::parent)
            .unwrap();
        workspace
            .join("tests/fixtures/mutation-plan")
            .to_string_lossy()
            .into_owned()
    }

    async fn plan_json(value: serde_json::Value) -> (bool, serde_json::Value) {
        let result = run_mutation_plan("fallow", plan_params(value))
            .await
            .unwrap();
        let ContentBlock::Text(text) = &result.content[0] else {
            panic!("expected text content");
        };
        (
            result.is_error == Some(true),
            serde_json::from_str(&text.text).unwrap(),
        )
    }

    #[tokio::test]
    async fn plan_call_returns_the_plan_envelope_for_a_paths_scope() {
        let (is_error, plan) = plan_json(serde_json::json!({
            "root": fixture_root(),
            "paths": ["src/math.ts"],
            "operators": ["arithmetic-operator"],
            "no_cache": true,
        }))
        .await;
        assert!(!is_error, "{plan}");
        assert_eq!(
            plan.as_object().unwrap().keys().next().map(String::as_str),
            Some("kind"),
            "the plan envelope leads with its root discriminator, as the CLI does"
        );
        assert_eq!(plan["kind"], "mutation-plan");
        assert_eq!(plan["schema_version"], "1");
        assert_eq!(plan["scope"]["kind"], "paths");
        let mutants = plan["mutants"].as_array().unwrap();
        assert!(!mutants.is_empty(), "{plan}");
        for mutant in mutants {
            assert_eq!(mutant["file"], "src/math.ts", "{mutant}");
            assert_eq!(mutant["operator"], "arithmetic-operator", "{mutant}");
            assert_eq!(mutant["disposition"], "planned", "{mutant}");
        }
        let files = plan["files"].as_array().unwrap();
        assert_eq!(files.len(), 1, "{plan}");
        assert_eq!(files[0]["path"], "src/math.ts");
        assert_eq!(files[0]["source_sha256"].as_str().unwrap().len(), 64);
        assert_eq!(files[0]["static_test_files"][0], "src/math.test.ts");
    }

    #[tokio::test]
    async fn plan_call_for_all_files_reports_pruned_dead_code() {
        let (is_error, plan) = plan_json(serde_json::json!({
            "root": fixture_root(),
            "all": true,
            "no_cache": true,
        }))
        .await;
        assert!(!is_error, "{plan}");
        assert_eq!(plan["scope"]["kind"], "all");
        let disposition = |file: &str| -> Vec<String> {
            plan["mutants"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|mutant| mutant["file"] == file)
                .map(|mutant| {
                    format!(
                        "{}:{}",
                        mutant["disposition"].as_str().unwrap(),
                        mutant["prune_reason"].as_str().unwrap_or("-")
                    )
                })
                .collect()
        };
        let dead = disposition("src/dead.ts");
        assert!(!dead.is_empty(), "{plan}");
        assert!(
            dead.iter().all(|entry| entry == "pruned:dead-code"),
            "{dead:?}"
        );
        assert!(
            disposition("src/math.ts")
                .iter()
                .all(|entry| entry == "planned:-"),
            "{plan}"
        );
    }

    fn mutant_operators(plan: &serde_json::Value) -> Vec<String> {
        let mut operators: Vec<String> = plan["mutants"]
            .as_array()
            .unwrap()
            .iter()
            .map(|mutant| mutant["operator"].as_str().unwrap().to_owned())
            .collect();
        operators.sort();
        operators.dedup();
        operators
    }

    /// `fallow mutation --plan` uses the `mutation.operators` config key when
    /// no operator flag is set. The MCP plan must list the same mutants.
    #[tokio::test]
    async fn plan_call_uses_the_config_operators_unless_the_param_sets_them() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(
            root.join("package.json"),
            r#"{ "name": "mutation-config", "private": true, "devDependencies": { "vitest": "4.1.11" } }"#,
        )
        .unwrap();
        std::fs::write(
            root.join("src/math.ts"),
            std::fs::read_to_string(std::path::Path::new(&fixture_root()).join("src/math.ts"))
                .unwrap(),
        )
        .unwrap();
        std::fs::write(
            root.join(".fallowrc.json"),
            r#"{ "mutation": { "operators": ["arithmetic-operator"] } }"#,
        )
        .unwrap();
        let root = root.to_string_lossy().into_owned();

        let (is_error, from_config) = plan_json(serde_json::json!({
            "root": root,
            "paths": ["src/math.ts"],
            "no_cache": true,
        }))
        .await;
        assert!(!is_error, "{from_config}");
        assert_eq!(
            mutant_operators(&from_config),
            ["arithmetic-operator"],
            "{from_config}"
        );

        let (is_error, from_param) = plan_json(serde_json::json!({
            "root": root,
            "paths": ["src/math.ts"],
            "operators": ["relational-operator"],
            "no_cache": true,
        }))
        .await;
        assert!(!is_error, "{from_param}");
        assert_eq!(
            mutant_operators(&from_param),
            ["relational-operator"],
            "the param wins over the config: {from_param}"
        );
    }

    #[tokio::test]
    async fn plan_call_returns_a_structured_error_for_a_zero_limit() {
        let (is_error, body) = plan_json(serde_json::json!({
            "root": fixture_root(),
            "all": true,
            "max_mutants": 0,
            "no_cache": true,
        }))
        .await;
        assert!(is_error, "{body}");
        assert_eq!(body["code"], "FALLOW_INVALID_MUTATION_OPTIONS");
        assert_eq!(body["exit_code"], 2);
    }
}
