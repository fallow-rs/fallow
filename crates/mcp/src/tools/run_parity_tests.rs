//! The MCP rows of the run parity matrix, and the production precedence of
//! each typed tool.
//!
//! The typed row reads the `mcp` input of a case into the params of the tool,
//! builds the API options with the options builder of the tool, and resolves
//! the run with `resolve_programmatic_run`. The case root is the `root`
//! parameter of the call. The CLI route row builds the argv of the CLI route
//! from the same params, without `root`, and compares it with the `cli`
//! input of the case.

use std::path::{Path, PathBuf};

use fallow_api::AnalysisOptions;
use fallow_api::run::parity::{
    Case, RowFailure, RowResult, assert_rows, load_cases, normalize_mcp_cli_argv,
};
use fallow_api::run::{EnvSnapshot, ProgrammaticEntry, Surface, resolve_programmatic_run};
use serde::de::DeserializeOwned;
use serde_json::Value;

use super::super::api_runtime::SharedParams;
use super::super::{
    analyze, architecture, audit, check_changed, decision_surface, dupes, flags, health,
    list_boundaries, mutation, project_info, trace,
};
use super::{ToolProduction, TypedTool, typed_call};

const ROW: &str = "mcp";

fn parse<P: DeserializeOwned>(params: &Value) -> Result<P, String> {
    serde_json::from_value(params.clone())
        .map_err(|err| format!("the params of the tool do not parse: {err}"))
}

/// The entry of `resolve_audit_run` for the audit options of a call.
fn audit_entry(options: &fallow_api::AuditOptions) -> ProgrammaticEntry {
    ProgrammaticEntry::Audit {
        dead_code: options.dead_code,
        duplication: options.duplication,
        health: options.health,
        production: options.production,
        production_dead_code: options.production_dead_code,
        production_health: options.production_health,
        production_dupes: options.production_dupes,
    }
}

/// The entry and the analysis options that the options builder of `tool`
/// gives for `params`. `entry_of` is the table of the entries.
fn tool_call(
    tool: TypedTool,
    params: &Value,
    env: EnvSnapshot,
    entry_of: impl Fn(TypedTool) -> ProgrammaticEntry,
) -> Result<(ProgrammaticEntry, AnalysisOptions), String> {
    let analysis = match tool {
        TypedTool::Audit => {
            let options = audit::audit_options_from_params(&parse(params)?, None, env)?;
            return Ok((audit_entry(&options), options.analysis));
        }
        TypedTool::Analyze => {
            analyze::dead_code_options_from_params(&parse(params)?, None, env)?.analysis
        }
        TypedTool::CheckChanged => {
            check_changed::check_changed_options_from_params(&parse(params)?, None, env).analysis
        }
        TypedTool::CheckArchitecture => {
            architecture::architecture_options_from_params(&parse(params)?, None, env).analysis
        }
        TypedTool::FindDupes => {
            dupes::duplication_options_from_params(&parse(params)?, None, env)?.analysis
        }
        TypedTool::CheckHealth => {
            health::health_options_from_params(&parse(params)?, None, env)?.analysis
        }
        TypedTool::DecisionSurface => {
            decision_surface::decision_surface_options_from_params(&parse(params)?, None, env)
                .analysis
        }
        TypedTool::ProjectInfo => {
            project_info::project_info_options_from_params(&parse(params)?, None, env).analysis
        }
        TypedTool::ListBoundaries => {
            list_boundaries::list_boundaries_options_from_params(&parse(params)?, None, env)
                .analysis
        }
        TypedTool::FeatureFlags => {
            flags::feature_flags_options_from_params(&parse(params)?, None, env)?.analysis
        }
        TypedTool::TraceExport => {
            trace::trace_export_options_from_params(&parse(params)?, None, env)?.analysis
        }
        TypedTool::TraceFile => {
            trace::trace_file_options_from_params(&parse(params)?, None, env)?.analysis
        }
        TypedTool::TraceImportPath => {
            trace::trace_import_path_options_from_params(&parse(params)?, None, env)?.analysis
        }
        TypedTool::TraceError => {
            trace::trace_error_options_from_params(&parse(params)?, None, env).analysis
        }
        TypedTool::TraceDependency => {
            trace::trace_dependency_options_from_params(&parse(params)?, None, env)?.analysis
        }
        TypedTool::TraceClone => {
            trace::trace_clone_options_from_params(&parse(params)?, None, env)?
                .duplication
                .analysis
        }
        TypedTool::MutationPlan => {
            mutation::mutation_plan_options(&parse(params)?, None, env)?.analysis
        }
        TypedTool::Combined => {
            return Err("the row has no options builder for the combined host call".to_owned());
        }
    };
    Ok((entry_of(tool), analysis))
}

/// The production table of the translator.
fn table(tool: TypedTool) -> ProgrammaticEntry {
    tool.entry(ToolProduction::default())
}

/// The typed row, with the table of the entries and a hook that changes the
/// analysis options before the resolve.
fn translate_with(
    case: &Case,
    root: &Path,
    env: EnvSnapshot,
    entry_of: impl Fn(TypedTool) -> ProgrammaticEntry,
    change: impl Fn(&mut AnalysisOptions),
) -> RowResult {
    let input = &case.inputs[ROW];
    if input.get("not_expressible").is_some() {
        return RowResult::NotExpressible;
    }
    let Some(tool) = input
        .get("tool")
        .and_then(Value::as_str)
        .and_then(TypedTool::from_name)
    else {
        return RowResult::Failed(format!("the mcp input names no typed tool: {input}"));
    };
    let mut params = input
        .get("params")
        .cloned()
        .unwrap_or_else(|| serde_json::json!({}));
    params["root"] = Value::String(root.display().to_string());
    if tool == TypedTool::Audit {
        return audit_row(&params, env, change);
    }
    let (entry, mut analysis) = match tool_call(tool, &params, env, entry_of) {
        Ok(call) => call,
        Err(message) => return RowResult::Failed(message),
    };
    change(&mut analysis);
    match resolve_programmatic_run(&analysis, &entry) {
        Ok(run) => RowResult::Run(Box::new(run)),
        Err(err) => RowResult::Failed(err.message),
    }
}

/// The run of a typed `audit` call, resolved through the steps of the audit
/// runner, so that the `base` parameter reaches the run.
fn audit_row(params: &Value, env: EnvSnapshot, change: impl Fn(&mut AnalysisOptions)) -> RowResult {
    let mut options = match parse(params)
        .and_then(|params| audit::audit_options_from_params(&params, None, env))
    {
        Ok(options) => options,
        Err(message) => return RowResult::Failed(message),
    };
    change(&mut options.analysis);
    match fallow_api::run::resolve_audit_head_run(&options) {
        Ok(run) => RowResult::Run(Box::new(run)),
        Err(err) => RowResult::Failed(err.message),
    }
}

fn translate(case: &Case, root: &Path, env: EnvSnapshot) -> RowResult {
    translate_with(case, root, env, table, |_| {})
}

fn failed_keys(failures: &[RowFailure]) -> Vec<(&str, &str)> {
    failures
        .iter()
        .map(|failure| (failure.case.as_str(), failure.key.as_str()))
        .collect()
}

#[test]
fn the_typed_row_passes_every_case() {
    let cases = load_cases();
    assert_eq!(assert_rows(ROW, &cases, translate), Vec::new());
}

#[test]
fn wrong_tool_kind_fails_the_matrix() {
    let cases = load_cases();
    let failures = assert_rows(ROW, &cases, |case, root, env| {
        let entry_of = |tool| match tool {
            TypedTool::TraceImportPath => ProgrammaticEntry::DeadCode,
            other => table(other),
        };
        translate_with(case, root, env, entry_of, |_| {})
    });
    let failed = failed_keys(&failures);
    assert!(
        failed.contains(&("trace-import-path-production-env", "command")),
        "{failures:?}"
    );
    assert!(
        failed.contains(&(
            "trace-import-path-production-env",
            "analyses.dead_code.production"
        )),
        "{failures:?}"
    );
}

#[test]
fn a_typed_row_with_the_api_surface_fails() {
    let cases = load_cases();
    let failures = assert_rows(ROW, &cases, |case, root, env| {
        translate_with(case, root, env, table, |analysis| {
            if let Some(host) = analysis.host.as_mut() {
                host.surface = Surface::Api;
            }
        })
    });
    let failed = failed_keys(&failures);
    assert!(
        failed.contains(&("analyze-production-env", "surface")),
        "{failures:?}"
    );
    assert!(
        failed.contains(&("health-production-env", "analyses.health.production")),
        "{failures:?}"
    );
}

/// The argv of the CLI route of `tool` for `params`, or `None` when the tool
/// has no CLI route.
fn cli_argv(tool: TypedTool, params: &Value) -> Option<Result<Vec<String>, String>> {
    let argv = match tool {
        TypedTool::Analyze => {
            parse(params).and_then(|params| super::super::build_analyze_args(&params))
        }
        TypedTool::CheckChanged => parse(params).map(super::super::build_check_changed_args),
        TypedTool::CheckArchitecture => {
            parse(params).map(|params| super::super::build_architecture_args(&params))
        }
        TypedTool::FindDupes => {
            parse(params).and_then(|params| super::super::build_find_dupes_args(&params))
        }
        TypedTool::CheckHealth => {
            parse(params).map(|params| super::super::build_health_args(&params))
        }
        TypedTool::Audit => {
            parse(params).and_then(|params| super::super::build_audit_args(&params))
        }
        TypedTool::ProjectInfo => {
            parse(params).map(|params| super::super::build_project_info_args(&params))
        }
        TypedTool::ListBoundaries => {
            parse(params).map(|params| super::super::build_list_boundaries_args(&params))
        }
        TypedTool::FeatureFlags => {
            parse(params).map(|params| super::super::build_feature_flags_args(&params))
        }
        TypedTool::TraceExport => {
            parse(params).and_then(|params| trace::build_trace_export_args(&params))
        }
        TypedTool::TraceFile => {
            parse(params).and_then(|params| trace::build_trace_file_args(&params))
        }
        TypedTool::TraceDependency => {
            parse(params).and_then(|params| trace::build_trace_dependency_args(&params))
        }
        TypedTool::TraceClone => {
            parse(params).and_then(|params| trace::build_trace_clone_args(&params))
        }
        TypedTool::TraceImportPath
        | TypedTool::TraceError
        | TypedTool::DecisionSurface
        | TypedTool::MutationPlan
        | TypedTool::Combined => return None,
    };
    Some(argv)
}

/// The failures of the CLI route row: for each case with a typed tool that
/// has a CLI route, `fallow` and the normalized argv of the route must equal
/// the `cli.argv` of the case.
fn cli_route_failures(
    cases: &[Case],
    argv_of: impl Fn(TypedTool, &Value) -> Option<Result<Vec<String>, String>>,
) -> Vec<String> {
    let mut failures = Vec::new();
    for case in cases {
        let Some(input) = case.inputs.get(ROW) else {
            failures.push(format!("{}: the case has no mcp input", case.name));
            continue;
        };
        if input.get("not_expressible").is_some() {
            continue;
        }
        let Some(tool) = input
            .get("tool")
            .and_then(Value::as_str)
            .and_then(TypedTool::from_name)
        else {
            failures.push(format!("{}: the mcp input names no typed tool", case.name));
            continue;
        };
        let params = input
            .get("params")
            .cloned()
            .unwrap_or_else(|| serde_json::json!({}));
        let Some(argv) = argv_of(tool, &params) else {
            continue;
        };
        let argv = match argv {
            Ok(argv) => argv,
            Err(message) => {
                failures.push(format!("{}: {message}", case.name));
                continue;
            }
        };
        let Some(expected) = case
            .inputs
            .get("cli")
            .and_then(|cli| cli.get("argv"))
            .and_then(Value::as_array)
        else {
            failures.push(format!("{}: the case has no cli argv", case.name));
            continue;
        };
        let expected: Vec<&str> = expected.iter().filter_map(Value::as_str).collect();
        let mut actual = vec!["fallow".to_owned()];
        actual.extend(normalize_mcp_cli_argv(&argv));
        if actual != expected {
            failures.push(format!(
                "{}: expected argv {expected:?}, got {actual:?}",
                case.name
            ));
        }
    }
    failures
}

#[test]
fn the_cli_route_row_passes_every_case() {
    assert_eq!(
        cli_route_failures(&load_cases(), cli_argv),
        Vec::<String>::new()
    );
}

#[test]
fn a_cli_route_that_drops_the_production_flag_fails() {
    let failures = cli_route_failures(&load_cases(), |tool, params| {
        cli_argv(tool, params).map(|argv| {
            argv.map(|argv| {
                argv.into_iter()
                    .filter(|arg| arg != "--production" && arg != "--no-production")
                    .collect()
            })
        })
    });
    for case in ["given-production-flag", "explicit-production-off"] {
        assert!(
            failures.iter().any(|failure| failure.starts_with(case)),
            "{case}: {failures:?}"
        );
    }
}

/// A project with `config` as its `.fallowrc.json`.
fn project(config: Option<&str>) -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().expect("project dir");
    let root = dir.path().to_path_buf();
    std::fs::create_dir(root.join(".git")).expect("git marker");
    std::fs::write(root.join("package.json"), r#"{"name":"app"}"#).expect("package.json");
    if let Some(config) = config {
        std::fs::write(root.join(".fallowrc.json"), config).expect("config");
    }
    (dir, root)
}

/// The decided production value and source of `analysis` in the run view.
fn production_of(run: &fallow_api::run::ResolvedRun, analysis: &str) -> (bool, String) {
    let view = serde_json::to_value(run.view()).expect("view");
    let decision = &view["analyses"][analysis]["production"];
    let source = &decision["source"];
    let name = ["name", "key", "rule", "from"]
        .iter()
        .find_map(|key| source.get(*key).and_then(Value::as_str))
        .or_else(|| source["origin"]["name"].as_str())
        .unwrap_or_default();
    (
        decision["value"].as_bool().expect("production value"),
        format!("{}:{name}", source["kind"].as_str().unwrap_or_default()),
    )
}

#[test]
fn audit_false_per_analysis_param_is_not_set() {
    let (_dir, root) = project(None);
    let cwd = tempfile::tempdir().expect("cwd");
    let env = EnvSnapshot::from_pairs(&[("FALLOW_PRODUCTION_DEAD_CODE", "1")], Some(cwd.path()));
    let params = serde_json::json!({
        "root": root.display().to_string(),
        "production_dead_code": false,
    });
    let (entry, analysis) = tool_call(TypedTool::Audit, &params, env, table).expect("audit call");
    let run = resolve_programmatic_run(&analysis, &entry).expect("audit run");
    assert_eq!(
        production_of(&run, "dead_code"),
        (true, "environment:FALLOW_PRODUCTION_DEAD_CODE".to_owned())
    );
}

#[test]
fn list_boundaries_honors_an_off_env_value() {
    let (_dir, root) = project(Some(r#"{"production":true}"#));
    let cwd = tempfile::tempdir().expect("cwd");
    let env = EnvSnapshot::from_pairs(&[("FALLOW_PRODUCTION", "0")], Some(cwd.path()));
    let params = serde_json::json!({ "root": root.display().to_string() });
    let (entry, analysis) =
        tool_call(TypedTool::ListBoundaries, &params, env, table).expect("list_boundaries call");
    let run = resolve_programmatic_run(&analysis, &entry).expect("list_boundaries run");
    assert_eq!(
        production_of(&run, "dead_code"),
        (false, "environment:FALLOW_PRODUCTION".to_owned())
    );
}

/// The analyses whose production mode a tool decides.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Analysis {
    DeadCode,
    Health,
    Dupes,
}

impl Analysis {
    const fn view_key(self) -> &'static str {
        match self {
            Self::DeadCode => "dead_code",
            Self::Health => "health",
            Self::Dupes => "duplication",
        }
    }

    const fn env_name(self) -> &'static str {
        match self {
            Self::DeadCode => "FALLOW_PRODUCTION_DEAD_CODE",
            Self::Health => "FALLOW_PRODUCTION_HEALTH",
            Self::Dupes => "FALLOW_PRODUCTION_DUPES",
        }
    }

    /// The value of the config `production` field for this analysis.
    fn config_value(self, config: Option<&str>) -> bool {
        match config {
            None => false,
            Some(config) => {
                let value: Value = serde_json::from_str(config).expect("config json");
                let production = &value["production"];
                production.as_bool().unwrap_or_else(|| {
                    let key = match self {
                        Self::DeadCode => "deadCode",
                        Self::Health => "health",
                        Self::Dupes => "dupes",
                    };
                    production[key].as_bool().unwrap_or(false)
                })
            }
        }
    }
}

const fn tool_analyses(tool: TypedTool) -> &'static [Analysis] {
    match tool {
        TypedTool::Audit | TypedTool::Combined => {
            &[Analysis::DeadCode, Analysis::Health, Analysis::Dupes]
        }
        TypedTool::FindDupes | TypedTool::TraceClone => &[Analysis::Dupes],
        TypedTool::CheckHealth => &[Analysis::Health],
        _ => &[Analysis::DeadCode],
    }
}

/// Whether the params of the tool have a `production` field.
const fn has_production_param(tool: TypedTool) -> bool {
    !matches!(
        tool,
        TypedTool::FindDupes
            | TypedTool::TraceClone
            | TypedTool::ProjectInfo
            | TypedTool::ListBoundaries
            | TypedTool::DecisionSurface
            | TypedTool::MutationPlan
    )
}

/// A boolean variable value of the test: `1` and `0`, else not a value.
fn reference_env(value: Option<&str>) -> Option<bool> {
    match value? {
        "1" => Some(true),
        "0" => Some(false),
        _ => None,
    }
}

/// The production override of one analysis for each typed tool, written
/// without the resolver: the oracle of
/// [`typed_production_matches_the_resolver`]. `None` lets the config decide.
/// A tool with a CLI route that reads the `FALLOW_PRODUCTION*` variables
/// takes the parameter, then the variable of the analysis, then
/// `FALLOW_PRODUCTION`. An audit parameter of one analysis counts only when
/// it is `true`.
fn reference_override(
    tool: TypedTool,
    own_param: Option<bool>,
    param: Option<bool>,
    own_env: Option<bool>,
    global_env: Option<bool>,
) -> Option<bool> {
    let env = own_env.or(global_env);
    match tool {
        TypedTool::Analyze
        | TypedTool::CheckChanged
        | TypedTool::CheckArchitecture
        | TypedTool::CheckHealth
        | TypedTool::FeatureFlags
        | TypedTool::TraceExport
        | TypedTool::TraceFile
        | TypedTool::TraceDependency
        | TypedTool::Combined => param.or(env),
        TypedTool::FindDupes
        | TypedTool::TraceClone
        | TypedTool::ProjectInfo
        | TypedTool::ListBoundaries => env,
        TypedTool::TraceImportPath | TypedTool::TraceError => param,
        TypedTool::DecisionSurface | TypedTool::MutationPlan => None,
        TypedTool::Audit => own_param.filter(|&on| on).or(param).or(env),
    }
}

const ENV_VALUES: [Option<&str>; 4] = [None, Some("1"), Some("0"), Some("maybe")];
const PARAM_VALUES: [Option<bool>; 3] = [None, Some(true), Some(false)];
const CONFIGS: [Option<&str>; 3] = [
    None,
    Some(r#"{"production":true}"#),
    Some(r#"{"production":{"deadCode":true,"health":false,"dupes":true}}"#),
];

/// One combination of the inputs of [`typed_production_matches_the_resolver`].
struct Probe<'a> {
    tool: TypedTool,
    analysis: Analysis,
    root: &'a Path,
    config: Option<&'a str>,
    own_param: Option<bool>,
    param: Option<bool>,
    own_env: Option<&'a str>,
    global_env: Option<&'a str>,
}

impl Probe<'_> {
    fn resolved(&self, cwd: &Path) -> bool {
        let mut pairs = Vec::new();
        if let Some(value) = self.own_env {
            pairs.push((self.analysis.env_name(), value));
        }
        if let Some(value) = self.global_env {
            pairs.push(("FALLOW_PRODUCTION", value));
        }
        let env = EnvSnapshot::from_pairs(&pairs, Some(cwd));
        let root = self.root.display().to_string();
        let shared = SharedParams {
            root: Some(&root),
            threads: Some(1),
            ..SharedParams::default()
        };
        let own = |analysis| {
            (self.analysis == analysis)
                .then_some(self.own_param)
                .flatten()
        };
        let production = ToolProduction {
            production: self.param,
            dead_code: own(Analysis::DeadCode),
            health: own(Analysis::Health),
            dupes: own(Analysis::Dupes),
        };
        let call = typed_call(self.tool, shared, production, env, None);
        let run = resolve_programmatic_run(&call.analysis, &call.entry)
            .unwrap_or_else(|err| panic!("{:?}: {}", self.tool, err.message));
        production_of(&run, self.analysis.view_key()).0
    }

    fn expected(&self) -> bool {
        reference_override(
            self.tool,
            self.own_param,
            self.param,
            reference_env(self.own_env),
            reference_env(self.global_env),
        )
        .unwrap_or_else(|| self.analysis.config_value(self.config))
    }
}

/// The parameter and variable values of one probe: the parameter of the
/// analysis, the `production` parameter, the variable of the analysis and
/// `FALLOW_PRODUCTION`.
type Values = (
    Option<bool>,
    Option<bool>,
    Option<&'static str>,
    Option<&'static str>,
);

/// Every combination of the given parameter values and of [`ENV_VALUES`].
fn value_combinations(own_params: &[Option<bool>], params: &[Option<bool>]) -> Vec<Values> {
    let envs: Vec<_> = ENV_VALUES
        .iter()
        .flat_map(|own| ENV_VALUES.iter().map(move |global| (*own, *global)))
        .collect();
    own_params
        .iter()
        .flat_map(|own_param| params.iter().map(move |param| (*own_param, *param)))
        .flat_map(|(own_param, param)| {
            envs.iter()
                .map(move |(own_env, global_env)| (own_param, param, *own_env, *global_env))
        })
        .collect()
}

#[test]
fn typed_production_matches_the_resolver() {
    let cwd = tempfile::tempdir().expect("cwd");
    let projects: Vec<_> = CONFIGS
        .iter()
        .map(|config| (*config, project(*config)))
        .collect();
    let mut mismatches = Vec::new();
    for tool in TypedTool::ALL {
        let params: &[Option<bool>] = if has_production_param(tool) {
            &PARAM_VALUES
        } else {
            &[None]
        };
        let own_params: &[Option<bool>] = if tool == TypedTool::Audit {
            &PARAM_VALUES
        } else {
            &[None]
        };
        let combinations = value_combinations(own_params, params);
        for &analysis in tool_analyses(tool) {
            for (config, (_dir, root)) in &projects {
                for &(own_param, param, own_env, global_env) in &combinations {
                    let probe = Probe {
                        tool,
                        analysis,
                        root,
                        config: *config,
                        own_param,
                        param,
                        own_env,
                        global_env,
                    };
                    let (expected, actual) = (probe.expected(), probe.resolved(cwd.path()));
                    if expected != actual {
                        mismatches.push(format!(
                            "{} {analysis:?} config {config:?} own param {own_param:?} param {param:?} own env {own_env:?} env {global_env:?}: expected {expected}, got {actual}",
                            tool.name()
                        ));
                    }
                }
            }
        }
    }
    assert!(mismatches.is_empty(), "{}", mismatches.join("\n"));
}

/// The params that the options builder of `tool` needs, without the
/// production parameters.
fn required_params(tool: TypedTool) -> Value {
    match tool {
        TypedTool::CheckChanged => serde_json::json!({ "since": "HEAD" }),
        TypedTool::TraceExport => serde_json::json!({ "file": "src/a.ts", "export_name": "a" }),
        TypedTool::TraceFile => serde_json::json!({ "file": "src/a.ts" }),
        TypedTool::TraceImportPath => serde_json::json!({ "from": "src/a.ts", "to": "src/b.ts" }),
        TypedTool::TraceError => serde_json::json!({ "trace": "Error: x" }),
        TypedTool::TraceDependency => serde_json::json!({ "package_name": "react" }),
        _ => serde_json::json!({}),
    }
}

/// Each production parameter of each typed tool reaches the run through the
/// real options builder of the tool. A builder that drops a parameter, such
/// as `production` of `trace_error` or `production_dupes` of `audit`, fails.
#[test]
fn every_production_param_reaches_the_run() {
    let (_dir, root) = project(None);
    let cwd = tempfile::tempdir().expect("cwd");
    let mut cases = Vec::new();
    for tool in TypedTool::ALL {
        if tool == TypedTool::Combined || !has_production_param(tool) {
            continue;
        }
        for &analysis in tool_analyses(tool) {
            cases.push((tool, "production", analysis));
        }
    }
    for (param, analysis) in [
        ("production_dead_code", Analysis::DeadCode),
        ("production_health", Analysis::Health),
        ("production_dupes", Analysis::Dupes),
    ] {
        cases.push((TypedTool::Audit, param, analysis));
    }
    let mut missing = Vec::new();
    for (tool, param, analysis) in cases {
        let mut params = required_params(tool);
        params["root"] = Value::String(root.display().to_string());
        params[param] = Value::Bool(true);
        let env = EnvSnapshot::from_pairs(&[], Some(cwd.path()));
        let (entry, analysis_options) = tool_call(tool, &params, env, table)
            .unwrap_or_else(|err| panic!("{}: {err}", tool.name()));
        let run = resolve_programmatic_run(&analysis_options, &entry)
            .unwrap_or_else(|err| panic!("{}: {}", tool.name(), err.message));
        let decided = production_of(&run, analysis.view_key());
        if decided != (true, format!("given:{param}")) {
            missing.push(format!("{} {param} {analysis:?}: {decided:?}", tool.name()));
        }
    }
    assert!(missing.is_empty(), "{}", missing.join("\n"));
}

/// A git repository on the branch `work` with two commits. The tag `first`
/// names the first commit. With `with_main`, a local `main` branch also
/// names it, so the audit detects `main` as its base. Without it, the
/// repository has no upstream, no remote and no `main`, so the audit detects
/// no base.
fn base_repo(with_main: bool) -> tempfile::TempDir {
    use super::super::base_root_fixture::git;
    let dir = tempfile::tempdir().expect("repo");
    let root = dir.path();
    let commit = |message: &str| {
        git(root, &["add", "."]);
        git(
            root,
            &[
                "-c",
                "user.email=test@example.com",
                "-c",
                "user.name=Test",
                "-c",
                "commit.gpgsign=false",
                "commit",
                "-q",
                "-m",
                message,
            ],
        );
    };
    std::fs::create_dir_all(root.join("src")).expect("src");
    std::fs::write(
        root.join("package.json"),
        r#"{"name":"base-order","type":"module","main":"src/index.ts"}"#,
    )
    .expect("package.json");
    std::fs::write(root.join("src/index.ts"), "console.log('entry');\n").expect("entry");
    git(root, &["init", "-q", "-b", "work"]);
    commit("first");
    git(root, &["tag", "first"]);
    if with_main {
        git(root, &["branch", "main"]);
    }
    std::fs::write(root.join("src/feature.ts"), "export const unused = 1;\n").expect("feature");
    commit("second");
    dir
}

/// The base ref of a typed `audit` call with `params` under `env`.
fn typed_audit_base(root: &Path, params: &Value, env: &[(&str, &str)]) -> Result<String, String> {
    let mut call = serde_json::json!({
        "root": root.display().to_string(),
        "no_cache": true,
        "gate": "all",
    });
    for (key, value) in params.as_object().expect("params object") {
        call[key] = value.clone();
    }
    let options =
        audit::audit_options_from_params(&parse(&call)?, None, EnvSnapshot::from_pairs(env, None))?;
    fallow_api::run_audit(&options)
        .map(|output| output.base_ref)
        .map_err(|err| err.message)
}

const CHANGED_SINCE_HEAD: (&str, &str) = ("FALLOW_CHANGED_SINCE", "HEAD");

#[test]
fn typed_audit_base_beats_changed_since_env() {
    let repo = base_repo(true);
    let base = typed_audit_base(
        repo.path(),
        &serde_json::json!({ "base": "first" }),
        &[CHANGED_SINCE_HEAD],
    );
    assert_eq!(base.as_deref(), Ok("first"));
}

#[test]
fn typed_audit_env_audit_base_beats_changed_since_env() {
    let repo = base_repo(true);
    let base = typed_audit_base(
        repo.path(),
        &serde_json::json!({}),
        &[("FALLOW_AUDIT_BASE", "first"), CHANGED_SINCE_HEAD],
    );
    assert_eq!(base.as_deref(), Ok("first"));
}

#[test]
fn typed_audit_detected_base_beats_changed_since_env() {
    let repo = base_repo(true);
    let base = typed_audit_base(repo.path(), &serde_json::json!({}), &[CHANGED_SINCE_HEAD]);
    assert_eq!(base.as_deref(), Ok("main"));
}

/// `FALLOW_CHANGED_SINCE` gives the base only when the audit detects none.
#[test]
fn typed_audit_changed_since_env_is_the_last_fallback() {
    let repo = base_repo(false);
    let base = typed_audit_base(
        repo.path(),
        &serde_json::json!({}),
        &[("FALLOW_CHANGED_SINCE", "first")],
    );
    assert_eq!(base.as_deref(), Ok("first"));
    let none = typed_audit_base(repo.path(), &serde_json::json!({}), &[]);
    assert!(
        none.as_ref()
            .is_err_and(|message| message.contains("could not detect base branch")),
        "{none:?}"
    );
}
