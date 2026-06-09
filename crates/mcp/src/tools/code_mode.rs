use std::cell::RefCell;
use std::fs;
use std::io::{Read, Seek, SeekFrom};
use std::process::{Command, Stdio};
use std::rc::Rc;
use std::thread;
use std::time::{Duration, Instant};

use rquickjs::prelude::{Func, MutFn};
use rquickjs::{Context, Ctx, Error as JsError, Exception, FromJs, Object, Runtime, Value};
use serde_json::json;

use crate::params::{
    AnalyzeParams, AuditParams, CheckChangedParams, CheckRuntimeCoverageParams, CodeExecuteParams,
    ExplainParams, FeatureFlagsParams, FindDupesParams, HealthParams, ImpactParams,
    ListBoundariesParams, ProjectInfoParams, SecurityCandidatesParams, TraceCloneParams,
    TraceDependencyParams, TraceExportParams, TraceFileParams,
};

use super::{
    build_analyze_args, build_audit_args, build_check_changed_args,
    build_check_runtime_coverage_args, build_explain_args, build_feature_flags_args,
    build_find_dupes_args, build_get_blast_radius_args, build_get_cleanup_candidates_args,
    build_get_hot_paths_args, build_get_importance_args, build_health_args, build_impact_args,
    build_list_boundaries_args, build_project_info_args, build_security_candidates_args,
    build_trace_clone_args, build_trace_dependency_args, build_trace_export_args,
    build_trace_file_args,
};

const DEFAULT_TIMEOUT_MS: u64 = 5_000;
const MAX_TIMEOUT_MS: u64 = 30_000;
const MAX_CODE_BYTES: usize = 20_000;
const DEFAULT_MAX_OUTPUT_BYTES: usize = 1_000_000;
const MAX_OUTPUT_BYTES: usize = 4_000_000;
const MEMORY_LIMIT_BYTES: usize = 32 * 1024 * 1024;
const MAX_STACK_BYTES: usize = 512 * 1024;
const MAX_HOST_CALLS: usize = 8;
const STDERR_LIMIT_BYTES: usize = 64 * 1024;
const POLL_INTERVAL: Duration = Duration::from_millis(10);

pub fn execute_code_mode(binary: String, params: CodeExecuteParams) -> Result<String, String> {
    let timeout_ms = params
        .timeout_ms
        .unwrap_or(DEFAULT_TIMEOUT_MS)
        .min(MAX_TIMEOUT_MS);
    let max_output_bytes = params
        .max_output_bytes
        .unwrap_or(DEFAULT_MAX_OUTPUT_BYTES)
        .min(MAX_OUTPUT_BYTES);
    if params.code.len() > MAX_CODE_BYTES {
        return Err(json!({
            "schema_version": "mcp-code-execute/v1",
            "ok": false,
            "error": format!("code mode snippet exceeded {MAX_CODE_BYTES} bytes"),
            "calls": [],
            "limits": {
                "timeout_ms": timeout_ms,
                "max_output_bytes": max_output_bytes,
                "max_host_calls": MAX_HOST_CALLS
            }
        })
        .to_string());
    }
    let deadline = Instant::now() + Duration::from_millis(timeout_ms);

    let runtime = Runtime::new().map_err(|err| format!("failed to create JS runtime: {err}"))?;
    runtime.set_memory_limit(MEMORY_LIMIT_BYTES);
    runtime.set_max_stack_size(MAX_STACK_BYTES);
    runtime.set_interrupt_handler(Some(Box::new(move || Instant::now() >= deadline)));

    let context =
        Context::full(&runtime).map_err(|err| format!("failed to create JS context: {err}"))?;
    let state = Rc::new(RefCell::new(CodeModeState {
        binary,
        default_root: params.root,
        deadline,
        max_output_bytes,
        output_bytes: 0,
        calls: Vec::new(),
    }));

    let result = context.with(|ctx| -> Result<String, String> {
        install_globals(&ctx, &state).map_err(|err| js_error_message(&ctx, &err))?;
        let source = user_source(&params.code);
        ctx.eval::<Value, _>(source)
            .and_then(|value| stringify_json(&ctx, value))
            .map_err(|err| js_error_message(&ctx, &err))
    });

    match result {
        Ok(result_json) => {
            let state = state.borrow();
            let output = json!({
                "schema_version": "mcp-code-execute/v1",
                "ok": true,
                "result": serde_json::from_str::<serde_json::Value>(&result_json)
                    .unwrap_or(serde_json::Value::Null),
                "calls": state.calls,
                "limits": {
                    "timeout_ms": timeout_ms,
                    "max_output_bytes": max_output_bytes,
                    "max_host_calls": MAX_HOST_CALLS
                }
            });
            Ok(output.to_string())
        }
        Err(err) => {
            let err = normalize_code_mode_error(&err, deadline);
            let state = state.borrow();
            let output = json!({
                "schema_version": "mcp-code-execute/v1",
                "ok": false,
                "error": err,
                "calls": state.calls,
                "limits": {
                    "timeout_ms": timeout_ms,
                    "max_output_bytes": max_output_bytes,
                    "max_host_calls": MAX_HOST_CALLS
                }
            });
            Err(output.to_string())
        }
    }
}

fn normalize_code_mode_error(err: &str, deadline: Instant) -> String {
    if err == "interrupted" && Instant::now() >= deadline {
        return "code mode execution timed out".to_string();
    }
    err.to_string()
}

fn install_globals(ctx: &Ctx<'_>, state: &Rc<RefCell<CodeModeState>>) -> rquickjs::Result<()> {
    let globals = ctx.globals();
    harden_globals(&globals)?;

    let fallow = Object::new(ctx.clone())?;
    install_host_api(ctx, &fallow, state)?;
    globals.set("fallow", fallow)?;
    ctx.eval::<(), _>("Object.freeze(globalThis.fallow);")?;
    Ok(())
}

fn harden_globals(globals: &Object<'_>) -> rquickjs::Result<()> {
    for name in [
        "eval",
        "Function",
        "AsyncFunction",
        "WebAssembly",
        "fetch",
        "XMLHttpRequest",
        "importScripts",
        "process",
        "require",
        "Deno",
        "Bun",
    ] {
        globals.set(name, Value::new_undefined(globals.ctx().clone()))?;
    }
    Ok(())
}

fn install_host_api<'js>(
    ctx: &Ctx<'js>,
    fallow: &Object<'js>,
    state: &Rc<RefCell<CodeModeState>>,
) -> rquickjs::Result<()> {
    let run_state = Rc::clone(state);
    fallow.set(
        "run",
        Func::from(MutFn::from(
            move |ctx: Ctx<'js>, tool: String, params: Value<'js>| {
                run_host_call(&ctx, &run_state, &tool, params)
            },
        )),
    )?;

    for (alias, tool) in CODE_MODE_ALIASES {
        let alias_state = Rc::clone(state);
        fallow.set(
            *alias,
            Func::from(MutFn::from(move |ctx: Ctx<'js>, params: Value<'js>| {
                run_host_call(&ctx, &alias_state, tool, params)
            })),
        )?;
    }

    let root = state.borrow().default_root.clone();
    if let Some(root) = root {
        ctx.globals().set("root", root)?;
    } else {
        ctx.globals()
            .set("root", Value::new_undefined(ctx.clone()))?;
    }
    Ok(())
}

fn run_host_call<'js>(
    ctx: &Ctx<'js>,
    state: &Rc<RefCell<CodeModeState>>,
    tool: &str,
    params: Value<'js>,
) -> rquickjs::Result<Value<'js>> {
    let params_json = stringify_params(ctx, params)?;
    let stdout = {
        let mut state = state.borrow_mut();
        state.run_tool(tool, &params_json)
    }
    .map_err(|err| Exception::throw_message(ctx, &err))?;

    ctx.json_parse(stdout)
}

fn js_error_message(ctx: &Ctx<'_>, err: &JsError) -> String {
    if err.is_exception() {
        let caught = ctx.catch();
        if let Ok(exception) = Exception::from_js(ctx, caught.clone())
            && let Some(message) = exception.message()
        {
            return message;
        }
        if let Ok(json) = stringify_json(ctx, caught) {
            return json;
        }
    }
    err.to_string()
}

fn stringify_params<'js>(ctx: &Ctx<'js>, params: Value<'js>) -> rquickjs::Result<String> {
    if params.is_undefined() || params.is_null() {
        return Ok("{}".to_string());
    }
    stringify_json(ctx, params)
}

fn stringify_json<'js>(ctx: &Ctx<'js>, value: Value<'js>) -> rquickjs::Result<String> {
    ctx.json_stringify(value)?
        .ok_or_else(|| JsError::new_from_js_message("undefined", "json", "value is not JSON"))
        .and_then(|value| value.to_string())
}

fn user_source(code: &str) -> String {
    let trimmed = code.trim();
    let function_expr = trimmed.starts_with('(')
        || trimmed.starts_with("function")
        || trimmed.starts_with("async ");
    let user = if function_expr {
        format!("({trimmed})")
    } else {
        format!(
            "(({{
                fallow,
                root
            }}) => {{
                {trimmed}
            }})"
        )
    };

    format!(
        r#"
        "use strict";
        const __codeModeUser = {user};
        if (typeof __codeModeUser !== "function") {{
            throw new Error("code must evaluate to a function or function body");
        }}
        const __codeModeResult = __codeModeUser({{ fallow: globalThis.fallow, root: globalThis.root }});
        if (__codeModeResult && typeof __codeModeResult.then === "function") {{
            throw new Error("async Code Mode snippets are not supported; use synchronous fallow host calls");
        }}
        __codeModeResult;
        "#
    )
}

struct CodeModeState {
    binary: String,
    default_root: Option<String>,
    deadline: Instant,
    max_output_bytes: usize,
    output_bytes: usize,
    calls: Vec<CodeModeCall>,
}

impl CodeModeState {
    fn run_tool(&mut self, tool: &str, params_json: &str) -> Result<String, String> {
        if self.calls.len() >= MAX_HOST_CALLS {
            return Err(format!(
                "code mode host call limit exceeded ({MAX_HOST_CALLS})"
            ));
        }
        if Instant::now() >= self.deadline {
            return Err("code mode execution timed out".to_string());
        }

        let started = Instant::now();
        let mut call = CodeModeCall {
            tool: tool.to_string(),
            duration_ms: 0,
            output_bytes: 0,
            ok: false,
            error_kind: None,
        };
        let result = self.run_tool_inner(tool, params_json, &mut call);
        call.duration_ms = started.elapsed().as_millis();

        match result {
            Ok(stdout) => {
                call.ok = true;
                self.calls.push(call);
                Ok(stdout)
            }
            Err(err) => {
                call.error_kind = Some(classify_host_error(&err));
                self.calls.push(call);
                Err(err)
            }
        }
    }

    fn run_tool_inner(
        &mut self,
        tool: &str,
        params_json: &str,
        call: &mut CodeModeCall,
    ) -> Result<String, String> {
        let tool = CodeModeTool::from_name(tool)?;
        call.tool = tool.name().to_string();
        let params = merge_default_root(params_json, self.default_root.as_deref())?;
        let args = build_tool_args(tool, params)?;
        let remaining_output_bytes = self.max_output_bytes.saturating_sub(self.output_bytes);
        if remaining_output_bytes == 0 {
            return Err(format!(
                "code mode host output exceeded {} bytes",
                self.max_output_bytes
            ));
        }
        let stdout = run_fallow_sync(
            &self.binary,
            "code_execute",
            &args,
            self.deadline,
            remaining_output_bytes,
        )?;
        call.output_bytes = stdout.len();
        self.output_bytes = self
            .output_bytes
            .checked_add(call.output_bytes)
            .ok_or_else(|| "code mode output byte counter overflowed".to_string())?;
        if self.output_bytes > self.max_output_bytes {
            return Err(format!(
                "code mode host output exceeded {} bytes",
                self.max_output_bytes
            ));
        }
        Ok(stdout)
    }
}

#[derive(serde::Serialize)]
struct CodeModeCall {
    tool: String,
    duration_ms: u128,
    output_bytes: usize,
    ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    error_kind: Option<&'static str>,
}

fn classify_host_error(message: &str) -> &'static str {
    if message.contains("does not expose fix tools")
        || message.contains("unsupported code mode fallow tool")
    {
        return "unsupported_tool";
    }
    if message.contains("timed out") {
        return "timeout";
    }
    if message.contains("host output exceeded") || message.contains("output byte counter") {
        return "output_limit";
    }
    if message.contains("invalid params JSON")
        || message.contains("params must be an object")
        || message.contains("invalid tool params")
    {
        return "invalid_params";
    }
    "subprocess"
}

#[derive(Clone, Copy)]
enum CodeModeTool {
    Analyze,
    CheckChanged,
    SecurityCandidates,
    FindDupes,
    ProjectInfo,
    TraceExport,
    TraceFile,
    TraceDependency,
    TraceClone,
    CheckHealth,
    Audit,
    FallowExplain,
    ListBoundaries,
    FeatureFlags,
    Impact,
    CheckRuntimeCoverage,
    GetHotPaths,
    GetBlastRadius,
    GetImportance,
    GetCleanupCandidates,
}

impl CodeModeTool {
    fn from_name(name: &str) -> Result<Self, String> {
        match name {
            "analyze" => Ok(Self::Analyze),
            "check_changed" => Ok(Self::CheckChanged),
            "security_candidates" => Ok(Self::SecurityCandidates),
            "find_dupes" => Ok(Self::FindDupes),
            "project_info" => Ok(Self::ProjectInfo),
            "trace_export" => Ok(Self::TraceExport),
            "trace_file" => Ok(Self::TraceFile),
            "trace_dependency" => Ok(Self::TraceDependency),
            "trace_clone" => Ok(Self::TraceClone),
            "check_health" => Ok(Self::CheckHealth),
            "audit" => Ok(Self::Audit),
            "fallow_explain" => Ok(Self::FallowExplain),
            "list_boundaries" => Ok(Self::ListBoundaries),
            "feature_flags" => Ok(Self::FeatureFlags),
            "impact" => Ok(Self::Impact),
            "check_runtime_coverage" => Ok(Self::CheckRuntimeCoverage),
            "get_hot_paths" => Ok(Self::GetHotPaths),
            "get_blast_radius" => Ok(Self::GetBlastRadius),
            "get_importance" => Ok(Self::GetImportance),
            "get_cleanup_candidates" => Ok(Self::GetCleanupCandidates),
            "fix_preview" | "fix_apply" => Err(
                "code mode does not expose fix tools; use standalone MCP tools for previews"
                    .to_string(),
            ),
            _ => Err(format!("unsupported code mode fallow tool '{name}'")),
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::Analyze => "analyze",
            Self::CheckChanged => "check_changed",
            Self::SecurityCandidates => "security_candidates",
            Self::FindDupes => "find_dupes",
            Self::ProjectInfo => "project_info",
            Self::TraceExport => "trace_export",
            Self::TraceFile => "trace_file",
            Self::TraceDependency => "trace_dependency",
            Self::TraceClone => "trace_clone",
            Self::CheckHealth => "check_health",
            Self::Audit => "audit",
            Self::FallowExplain => "fallow_explain",
            Self::ListBoundaries => "list_boundaries",
            Self::FeatureFlags => "feature_flags",
            Self::Impact => "impact",
            Self::CheckRuntimeCoverage => "check_runtime_coverage",
            Self::GetHotPaths => "get_hot_paths",
            Self::GetBlastRadius => "get_blast_radius",
            Self::GetImportance => "get_importance",
            Self::GetCleanupCandidates => "get_cleanup_candidates",
        }
    }
}

const CODE_MODE_ALIASES: &[(&str, &str)] = &[
    ("analyze", "analyze"),
    ("checkChanged", "check_changed"),
    ("securityCandidates", "security_candidates"),
    ("findDupes", "find_dupes"),
    ("projectInfo", "project_info"),
    ("traceExport", "trace_export"),
    ("traceFile", "trace_file"),
    ("traceDependency", "trace_dependency"),
    ("traceClone", "trace_clone"),
    ("checkHealth", "check_health"),
    ("audit", "audit"),
    ("explain", "fallow_explain"),
    ("listBoundaries", "list_boundaries"),
    ("featureFlags", "feature_flags"),
    ("impact", "impact"),
    ("checkRuntimeCoverage", "check_runtime_coverage"),
    ("getHotPaths", "get_hot_paths"),
    ("getBlastRadius", "get_blast_radius"),
    ("getImportance", "get_importance"),
    ("getCleanupCandidates", "get_cleanup_candidates"),
];

fn merge_default_root(
    params_json: &str,
    default_root: Option<&str>,
) -> Result<serde_json::Value, String> {
    let mut params: serde_json::Value =
        serde_json::from_str(params_json).map_err(|err| format!("invalid params JSON: {err}"))?;
    if !params.is_object() {
        return Err("fallow host call params must be an object".to_string());
    }
    if let Some(root) = default_root
        && params.get("root").is_none()
        && let Some(object) = params.as_object_mut()
    {
        object.insert(
            "root".to_string(),
            serde_json::Value::String(root.to_string()),
        );
    }
    Ok(params)
}

fn build_tool_args(tool: CodeModeTool, params: serde_json::Value) -> Result<Vec<String>, String> {
    match tool {
        CodeModeTool::Analyze => {
            let params: AnalyzeParams = parse_params(params)?;
            build_analyze_args(&params)
        }
        CodeModeTool::CheckChanged => {
            let params: CheckChangedParams = parse_params(params)?;
            Ok(build_check_changed_args(params))
        }
        CodeModeTool::SecurityCandidates => {
            let params: SecurityCandidatesParams = parse_params(params)?;
            build_security_candidates_args(&params)
        }
        CodeModeTool::FindDupes => {
            let params: FindDupesParams = parse_params(params)?;
            build_find_dupes_args(&params)
        }
        CodeModeTool::ProjectInfo => {
            let params: ProjectInfoParams = parse_params(params)?;
            Ok(build_project_info_args(&params))
        }
        CodeModeTool::TraceExport => {
            let params: TraceExportParams = parse_params(params)?;
            build_trace_export_args(&params)
        }
        CodeModeTool::TraceFile => {
            let params: TraceFileParams = parse_params(params)?;
            build_trace_file_args(&params)
        }
        CodeModeTool::TraceDependency => {
            let params: TraceDependencyParams = parse_params(params)?;
            build_trace_dependency_args(&params)
        }
        CodeModeTool::TraceClone => {
            let params: TraceCloneParams = parse_params(params)?;
            build_trace_clone_args(&params)
        }
        CodeModeTool::CheckHealth => {
            let params: HealthParams = parse_params(params)?;
            Ok(build_health_args(&params))
        }
        CodeModeTool::Audit => {
            let params: AuditParams = parse_params(params)?;
            build_audit_args(&params)
        }
        CodeModeTool::FallowExplain => {
            let params: ExplainParams = parse_params(params)?;
            Ok(build_explain_args(&params))
        }
        CodeModeTool::ListBoundaries => {
            let params: ListBoundariesParams = parse_params(params)?;
            Ok(build_list_boundaries_args(&params))
        }
        CodeModeTool::FeatureFlags => {
            let params: FeatureFlagsParams = parse_params(params)?;
            Ok(build_feature_flags_args(&params))
        }
        CodeModeTool::Impact => {
            let params: ImpactParams = parse_params(params)?;
            Ok(build_impact_args(&params))
        }
        CodeModeTool::CheckRuntimeCoverage => {
            let params: CheckRuntimeCoverageParams = parse_params(params)?;
            Ok(build_check_runtime_coverage_args(&params))
        }
        CodeModeTool::GetHotPaths => {
            let params: CheckRuntimeCoverageParams = parse_params(params)?;
            Ok(build_get_hot_paths_args(&params))
        }
        CodeModeTool::GetBlastRadius => {
            let params: CheckRuntimeCoverageParams = parse_params(params)?;
            Ok(build_get_blast_radius_args(&params))
        }
        CodeModeTool::GetImportance => {
            let params: CheckRuntimeCoverageParams = parse_params(params)?;
            Ok(build_get_importance_args(&params))
        }
        CodeModeTool::GetCleanupCandidates => {
            let params: CheckRuntimeCoverageParams = parse_params(params)?;
            Ok(build_get_cleanup_candidates_args(&params))
        }
    }
}

fn parse_params<T>(params: serde_json::Value) -> Result<T, String>
where
    T: serde::de::DeserializeOwned,
{
    serde_json::from_value(params).map_err(|err| format!("invalid tool params: {err}"))
}

fn run_fallow_sync(
    binary: &str,
    tool: &'static str,
    args: &[String],
    deadline: Instant,
    max_output_bytes: usize,
) -> Result<String, String> {
    let mut stdout_file = tempfile::NamedTempFile::new()
        .map_err(|err| format!("failed to create stdout temp file: {err}"))?;
    let mut stderr_file = tempfile::NamedTempFile::new()
        .map_err(|err| format!("failed to create stderr temp file: {err}"))?;
    let mut child = Command::new(binary)
        .args(args)
        .stdout(Stdio::from(
            stdout_file
                .reopen()
                .map_err(|err| format!("failed to reopen stdout temp file: {err}"))?,
        ))
        .stderr(Stdio::from(
            stderr_file
                .reopen()
                .map_err(|err| format!("failed to reopen stderr temp file: {err}"))?,
        ))
        .env("FALLOW_INTEGRATION_SURFACE", "mcp")
        .env("FALLOW_MCP_TOOL", tool)
        .spawn()
        .map_err(|err| {
            format!(
                "failed to execute fallow binary '{binary}': {err}. Ensure fallow is installed and available in PATH, or set FALLOW_BIN."
            )
        })?;

    loop {
        if let Some(status) = child
            .try_wait()
            .map_err(|err| format!("failed to wait for fallow subprocess: {err}"))?
        {
            let stdout_len = file_len(stdout_file.as_file())?;
            if stdout_len > max_output_bytes as u64 {
                return Err(format!(
                    "code mode host output exceeded {max_output_bytes} bytes"
                ));
            }

            let stdout = read_file(stdout_file.as_file_mut(), "stdout")?;
            let stderr = read_limited_file(stderr_file.as_file_mut(), STDERR_LIMIT_BYTES)?;
            return normalize_output(status.code().unwrap_or(-1), &stdout, &stderr);
        }

        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err("code mode execution timed out while running fallow".to_string());
        }
        if file_len(stdout_file.as_file())? > max_output_bytes as u64 {
            let _ = child.kill();
            let _ = child.wait();
            return Err(format!(
                "code mode host output exceeded {max_output_bytes} bytes"
            ));
        }

        thread::sleep(POLL_INTERVAL);
    }
}

fn file_len(file: &fs::File) -> Result<u64, String> {
    file.metadata()
        .map(|metadata| metadata.len())
        .map_err(|err| format!("failed to inspect fallow output file: {err}"))
}

fn read_file(file: &mut fs::File, label: &str) -> Result<Vec<u8>, String> {
    file.seek(SeekFrom::Start(0))
        .map_err(|err| format!("failed to rewind fallow {label}: {err}"))?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)
        .map_err(|err| format!("failed to read fallow {label}: {err}"))?;
    Ok(bytes)
}

fn read_limited_file(file: &mut fs::File, limit: usize) -> Result<Vec<u8>, String> {
    let len = file_len(file)?;
    if len > limit as u64 {
        return Ok(format!("stderr exceeded {limit} bytes").into_bytes());
    }
    read_file(file, "stderr")
}

fn normalize_output(exit_code: i32, stdout: &[u8], stderr: &[u8]) -> Result<String, String> {
    let stdout = String::from_utf8_lossy(stdout).to_string();
    let stderr = String::from_utf8_lossy(stderr).trim().to_string();

    match exit_code {
        0 | 1 => Ok(if stdout.is_empty() {
            "{}".to_string()
        } else {
            stdout
        }),
        _ if !stdout.is_empty() && serde_json::from_str::<serde_json::Value>(&stdout).is_ok() => {
            Err(stdout)
        }
        _ => Err(json!({
            "error": true,
            "message": if stderr.is_empty() {
                format!("fallow exited with code {exit_code}")
            } else {
                stderr
            },
            "exit_code": exit_code
        })
        .to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fix_tools_are_not_allowed_in_code_mode() {
        assert!(CodeModeTool::from_name("fix_apply").is_err());
        assert!(CodeModeTool::from_name("fix_preview").is_err());
    }

    #[test]
    fn default_root_is_injected_into_object_params() {
        let params = merge_default_root(r#"{"files":true}"#, Some("/tmp/project")).unwrap();
        assert_eq!(params["root"], "/tmp/project");
        assert_eq!(params["files"], true);
    }

    #[test]
    fn explicit_root_wins_over_default_root() {
        let params = merge_default_root(r#"{"root":"/tmp/other"}"#, Some("/tmp/project")).unwrap();
        assert_eq!(params["root"], "/tmp/other");
    }

    #[test]
    fn non_object_params_are_rejected() {
        let err = merge_default_root("[]", Some("/tmp/project")).unwrap_err();
        assert!(err.contains("params must be an object"));
    }

    #[test]
    fn statement_body_is_wrapped_as_function_body() {
        let source = user_source("return { ok: true };");
        assert!(source.contains("return { ok: true };"));
        assert!(source.contains("__codeModeUser({ fallow: globalThis.fallow"));
    }

    #[test]
    fn function_expression_is_preserved() {
        let source = user_source("({ fallow }) => fallow.projectInfo({ files: true })");
        assert!(source.contains("({ fallow }) => fallow.projectInfo({ files: true })"));
    }

    #[test]
    fn statement_body_allows_nested_arrow_callbacks() {
        let source = user_source("const pick = () => 1; return { value: pick() };");
        assert!(source.contains("const pick = () => 1; return { value: pick() };"));
        assert!(!source.contains("const __codeModeUser = (const pick"));
    }

    #[test]
    fn async_snippets_are_rejected_explicitly() {
        let source = user_source("async ({ fallow }) => fallow.projectInfo({ files: true })");
        assert!(source.contains("async Code Mode snippets are not supported"));
    }

    #[test]
    fn oversized_code_is_rejected_before_runtime() {
        let output = execute_code_mode(
            "fallow".to_string(),
            CodeExecuteParams {
                code: "x".repeat(MAX_CODE_BYTES + 1),
                root: None,
                timeout_ms: Some(1_000),
                max_output_bytes: Some(10_000),
            },
        )
        .expect_err("oversized snippets should be rejected");

        let json: serde_json::Value = serde_json::from_str(&output).unwrap();
        assert_eq!(json["ok"].as_bool(), Some(false));
        assert!(
            json["error"]
                .as_str()
                .is_some_and(|error| error.contains("exceeded 20000 bytes"))
        );
        assert_eq!(json["calls"].as_array().map(Vec::len), Some(0));
    }

    #[test]
    fn cpu_bound_snippets_report_timeout() {
        let output = execute_code_mode(
            "fallow".to_string(),
            CodeExecuteParams {
                code: "while (true) {}".to_string(),
                root: None,
                timeout_ms: Some(1),
                max_output_bytes: Some(10_000),
            },
        )
        .expect_err("cpu-bound snippets should time out");

        let json: serde_json::Value = serde_json::from_str(&output).unwrap();
        assert_eq!(json["ok"].as_bool(), Some(false));
        assert_eq!(
            json["error"].as_str(),
            Some("code mode execution timed out")
        );
        assert_eq!(json["calls"].as_array().map(Vec::len), Some(0));
    }
}
