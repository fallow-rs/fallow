use std::cell::RefCell;
#[cfg(test)]
use std::fs;
#[cfg(test)]
use std::process::Command;
use std::rc::Rc;
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use rquickjs::prelude::{Func, MutFn};
use rquickjs::{Context, Ctx, Error as JsError, Exception, FromJs, Object, Runtime, Value};
use serde_json::json;

use crate::params::CodeExecuteParams;

const DEFAULT_TIMEOUT_MS: u64 = 5_000;
const MAX_TIMEOUT_MS: u64 = 30_000;
const MAX_CODE_BYTES: usize = 20_000;
const DEFAULT_MAX_OUTPUT_BYTES: usize = 1_000_000;
const MAX_OUTPUT_BYTES: usize = 4_000_000;
const MEMORY_LIMIT_BYTES: usize = 32 * 1024 * 1024;
const MAX_STACK_BYTES: usize = 512 * 1024;
const MAX_HOST_CALLS: usize = 8;

#[path = "code_mode_subprocess.rs"]
mod code_mode_subprocess;
#[path = "code_mode_tools.rs"]
mod code_mode_tools;

#[cfg(test)]
use code_mode_subprocess::normalize_output;
use code_mode_subprocess::run_fallow_sync;
use code_mode_tools::{
    CODE_MODE_ALIASES, CodeModeTool, build_tool_args, merge_default_root, run_api_tool,
};

pub fn execute_code_mode(binary: String, params: CodeExecuteParams) -> Result<String, String> {
    let timeout_ms = params
        .timeout_ms
        .unwrap_or(DEFAULT_TIMEOUT_MS)
        .min(MAX_TIMEOUT_MS);
    let max_output_bytes = params
        .max_output_bytes
        .unwrap_or(DEFAULT_MAX_OUTPUT_BYTES)
        .min(MAX_OUTPUT_BYTES);
    let limits = code_mode_limits(timeout_ms, max_output_bytes);
    if params.code.len() > MAX_CODE_BYTES {
        return Err(json!({
            "schema_version": "mcp-code-execute/v1",
            "ok": false,
            "error": format!("code mode snippet exceeded {MAX_CODE_BYTES} bytes"),
            "calls": [],
            "limits": limits
        })
        .to_string());
    }
    let deadline = Instant::now() + Duration::from_millis(timeout_ms);

    let runtime = build_code_mode_runtime(deadline)?;
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

    let result = run_code_mode_eval(&context, &state, &params.code);

    let calls = &state.borrow().calls;
    match result {
        Ok(result_json) => Ok(json!({
            "schema_version": "mcp-code-execute/v1",
            "ok": true,
            "result": serde_json::from_str::<serde_json::Value>(&result_json)
                .unwrap_or(serde_json::Value::Null),
            "calls": calls,
            "limits": limits
        })
        .to_string()),
        Err(err) => Err(json!({
            "schema_version": "mcp-code-execute/v1",
            "ok": false,
            "error": normalize_code_mode_error(&err, deadline),
            "calls": calls,
            "limits": limits
        })
        .to_string()),
    }
}

/// Build the `limits` JSON block echoed on every code-mode response.
fn code_mode_limits(timeout_ms: u64, max_output_bytes: usize) -> serde_json::Value {
    json!({
        "timeout_ms": timeout_ms,
        "max_output_bytes": max_output_bytes,
        "max_host_calls": MAX_HOST_CALLS
    })
}

/// Install the host API into `context` and evaluate the user snippet, returning
/// the JSON-stringified result or a normalized error message.
fn run_code_mode_eval(
    context: &Context,
    state: &Rc<RefCell<CodeModeState>>,
    code: &str,
) -> Result<String, String> {
    context.with(|ctx| -> Result<String, String> {
        install_globals(&ctx, state).map_err(|err| js_error_message(&ctx, &err))?;
        let source = user_source(code);
        ctx.eval::<Value, _>(source)
            .and_then(|value| stringify_json(&ctx, value))
            .map_err(|err| js_error_message(&ctx, &err))
    })
}

/// Build the sandboxed QuickJS runtime with memory, stack, and deadline limits.
fn build_code_mode_runtime(deadline: Instant) -> Result<Runtime, String> {
    let runtime = Runtime::new().map_err(|err| format!("failed to create JS runtime: {err}"))?;
    runtime.set_memory_limit(MEMORY_LIMIT_BYTES);
    runtime.set_max_stack_size(MAX_STACK_BYTES);
    runtime.set_interrupt_handler(Some(Box::new(move || Instant::now() >= deadline)));
    Ok(runtime)
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
        let remaining_output_bytes = self.max_output_bytes.saturating_sub(self.output_bytes);
        if remaining_output_bytes == 0 {
            return Err(format!(
                "code mode host output exceeded {} bytes",
                self.max_output_bytes
            ));
        }
        let stdout =
            if let Some(value) = run_api_tool_with_deadline(tool, params.clone(), self.deadline)? {
                serde_json::to_string(&value).unwrap_or_else(|_| "{}".to_string())
            } else {
                let args = build_tool_args(tool, params)?;
                run_fallow_sync(
                    &self.binary,
                    "code_execute",
                    &args,
                    self.deadline,
                    remaining_output_bytes,
                )?
            };
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

fn run_api_tool_with_deadline(
    tool: CodeModeTool,
    params: serde_json::Value,
    deadline: Instant,
) -> Result<Option<serde_json::Value>, String> {
    run_api_tool_with_deadline_and_runner(tool, params, deadline, run_api_tool)
}

fn run_api_tool_with_deadline_and_runner<F>(
    tool: CodeModeTool,
    params: serde_json::Value,
    deadline: Instant,
    runner: F,
) -> Result<Option<serde_json::Value>, String>
where
    F: FnOnce(CodeModeTool, serde_json::Value) -> Result<Option<serde_json::Value>, String>
        + Send
        + 'static,
{
    if !tool.is_code_mode_api_backed() {
        return Ok(None);
    }
    let Some(remaining) = deadline.checked_duration_since(Instant::now()) else {
        return Err("code mode execution timed out".to_string());
    };
    let (tx, rx) = mpsc::channel();
    thread::Builder::new()
        .name("fallow-code-mode-api".to_string())
        .spawn(move || {
            let result = runner(tool, params);
            let _ = tx.send(result);
        })
        .map_err(|err| format!("failed to start code mode API host call: {err}"))?;

    match rx.recv_timeout(remaining) {
        Ok(result) => result,
        Err(mpsc::RecvTimeoutError::Timeout) => {
            Err("code mode execution timed out while running fallow".to_string())
        }
        Err(mpsc::RecvTimeoutError::Disconnected) => {
            Err("code mode API host call failed".to_string())
        }
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
    fn heavy_code_mode_analyze_uses_cancellable_subprocess_path() {
        let temp = tempfile::tempdir().expect("tempdir");
        fs::create_dir(temp.path().join("src")).expect("src dir");
        fs::write(
            temp.path().join("package.json"),
            r#"{"name":"code-mode-api-test","type":"module"}"#,
        )
        .expect("package json");
        fs::write(
            temp.path().join("src/index.ts"),
            "export const unused = 1;\n",
        )
        .expect("source");

        let output = execute_code_mode(
            "/definitely/not/fallow".to_string(),
            CodeExecuteParams {
                code:
                    r#"return fallow.analyze({ issue_types: ["unused-exports"], no_cache: true });"#
                        .to_string(),
                root: Some(temp.path().display().to_string()),
                timeout_ms: Some(5_000),
                max_output_bytes: Some(200_000),
            },
        )
        .expect_err("heavy analyze should use the cancellable subprocess path");

        let json: serde_json::Value = serde_json::from_str(&output).expect("code mode json");
        assert_eq!(json["ok"].as_bool(), Some(false));
        assert_eq!(json["calls"][0]["tool"].as_str(), Some("analyze"));
        assert_eq!(json["calls"][0]["ok"].as_bool(), Some(false));
        assert_eq!(json["calls"][0]["error_kind"].as_str(), Some("subprocess"));
    }

    #[test]
    fn api_backed_combined_does_not_spawn_binary() {
        let temp = tempfile::tempdir().expect("tempdir");
        fs::create_dir(temp.path().join("src")).expect("src dir");
        fs::write(
            temp.path().join("package.json"),
            r#"{"name":"code-mode-combined-test","type":"module","main":"src/index.ts"}"#,
        )
        .expect("package json");
        fs::write(
            temp.path().join("src/index.ts"),
            "export const unused = 1;\n",
        )
        .expect("source");

        let output = execute_code_mode(
            "/definitely/not/fallow".to_string(),
            CodeExecuteParams {
                code: "return fallow.combined({ no_cache: true, score: true });".to_string(),
                root: Some(temp.path().display().to_string()),
                timeout_ms: Some(5_000),
                max_output_bytes: Some(200_000),
            },
        )
        .expect("api-backed combined should not need the binary");

        let json: serde_json::Value = serde_json::from_str(&output).expect("code mode json");
        assert_eq!(json["ok"].as_bool(), Some(true));
        assert_eq!(json["result"]["kind"].as_str(), Some("combined"));
        assert!(json["result"]["check"]["summary"].is_object());
        assert!(json["result"]["check"]["unused_exports"].is_array());
        assert!(json["result"]["dupes"]["stats"].is_object());
        assert!(json["result"]["health"]["summary"].is_object());
        assert_eq!(json["calls"][0]["tool"].as_str(), Some("combined"));
        assert_eq!(json["calls"][0]["ok"].as_bool(), Some(true));
    }

    #[test]
    fn api_backed_check_changed_does_not_spawn_binary() {
        let temp = tempfile::tempdir().expect("tempdir");
        fs::create_dir(temp.path().join("src")).expect("src dir");
        fs::write(
            temp.path().join("package.json"),
            r#"{"name":"code-mode-changed-test","type":"module","main":"src/index.ts"}"#,
        )
        .expect("package json");
        fs::write(temp.path().join("src/index.ts"), "console.log('entry');\n").expect("source");
        fs::write(
            temp.path().join("src/feature.ts"),
            "export const used = 1;\n",
        )
        .expect("feature source");
        git(temp.path(), &["init"]);
        git(temp.path(), &["add", "."]);
        git(
            temp.path(),
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
        fs::write(
            temp.path().join("src/feature.ts"),
            "export const unused = 1;\n",
        )
        .expect("changed source");

        let output = execute_code_mode(
            "/definitely/not/fallow".to_string(),
            CodeExecuteParams {
                code: r#"return fallow.checkChanged({ since: "HEAD", no_cache: true });"#
                    .to_string(),
                root: Some(temp.path().display().to_string()),
                timeout_ms: Some(5_000),
                max_output_bytes: Some(200_000),
            },
        )
        .expect("api-backed checkChanged should not need the binary");

        let json: serde_json::Value = serde_json::from_str(&output).expect("code mode json");
        assert_eq!(json["ok"].as_bool(), Some(true));
        assert_eq!(json["result"]["kind"].as_str(), Some("dead-code"));
        assert!(json["result"]["summary"].is_object());
        assert_eq!(json["calls"][0]["tool"].as_str(), Some("check_changed"));
        assert_eq!(json["calls"][0]["ok"].as_bool(), Some(true));
    }

    #[test]
    fn api_backed_feature_flags_does_not_spawn_binary() {
        let temp = tempfile::tempdir().expect("tempdir");
        fs::create_dir(temp.path().join("src")).expect("src dir");
        fs::write(
            temp.path().join("package.json"),
            r#"{"name":"code-mode-flags-test","type":"module","main":"src/index.ts"}"#,
        )
        .expect("package json");
        fs::write(
            temp.path().join("src/index.ts"),
            "if (process.env.FEATURE_ALPHA) {\n  console.log('on');\n}\n",
        )
        .expect("source");

        let output = execute_code_mode(
            "/definitely/not/fallow".to_string(),
            CodeExecuteParams {
                code: "return fallow.featureFlags({ no_cache: true });".to_string(),
                root: Some(temp.path().display().to_string()),
                timeout_ms: Some(5_000),
                max_output_bytes: Some(200_000),
            },
        )
        .expect("api-backed feature flags should not need the binary");

        let json: serde_json::Value = serde_json::from_str(&output).expect("code mode json");
        assert_eq!(json["ok"].as_bool(), Some(true));
        assert_eq!(json["result"]["kind"].as_str(), Some("feature-flags"));
        assert_eq!(
            json["result"]["feature_flags"][0]["flag_name"].as_str(),
            Some("FEATURE_ALPHA")
        );
        assert_eq!(json["calls"][0]["tool"].as_str(), Some("feature_flags"));
        assert_eq!(json["calls"][0]["ok"].as_bool(), Some(true));
    }

    #[test]
    fn api_backed_list_boundaries_does_not_spawn_binary() {
        let temp = tempfile::tempdir().expect("tempdir");
        fs::create_dir_all(temp.path().join("src/app")).expect("app dir");
        fs::create_dir_all(temp.path().join("src/shared")).expect("shared dir");
        fs::write(
            temp.path().join("package.json"),
            r#"{"name":"code-mode-boundaries-test","type":"module","main":"src/app/index.ts"}"#,
        )
        .expect("package json");
        fs::write(
            temp.path().join(".fallowrc.json"),
            r#"{
                "boundaries": {
                    "zones": [
                        { "name": "app", "patterns": ["src/app/**"] },
                        { "name": "shared", "patterns": ["src/shared/**"] }
                    ],
                    "rules": [
                        { "from": "app", "allow": ["shared"] }
                    ]
                }
            }"#,
        )
        .expect("config");
        fs::write(
            temp.path().join("src/app/index.ts"),
            "export const app = 1;\n",
        )
        .expect("app source");
        fs::write(
            temp.path().join("src/shared/index.ts"),
            "export const shared = 1;\n",
        )
        .expect("shared source");

        let output = execute_code_mode(
            "/definitely/not/fallow".to_string(),
            CodeExecuteParams {
                code: "return fallow.listBoundaries({ no_cache: true });".to_string(),
                root: Some(temp.path().display().to_string()),
                timeout_ms: Some(5_000),
                max_output_bytes: Some(200_000),
            },
        )
        .expect("api-backed list boundaries should not need the binary");

        let json: serde_json::Value = serde_json::from_str(&output).expect("code mode json");
        assert_eq!(json["ok"].as_bool(), Some(true));
        assert_eq!(json["result"]["kind"].as_str(), Some("list-boundaries"));
        assert_eq!(json["result"]["boundaries"]["zone_count"], 2);
        assert_eq!(json["result"]["boundaries"]["rule_count"], 1);
        assert_eq!(json["calls"][0]["tool"].as_str(), Some("list_boundaries"));
        assert_eq!(json["calls"][0]["ok"].as_bool(), Some(true));
    }

    #[test]
    fn api_backed_explain_does_not_spawn_binary() {
        let output = execute_code_mode(
            "/definitely/not/fallow".to_string(),
            CodeExecuteParams {
                code: "return fallow.explain({ issue_type: 'unused-export' });".to_string(),
                root: None,
                timeout_ms: Some(5_000),
                max_output_bytes: Some(200_000),
            },
        )
        .expect("api-backed explain should not need the binary");

        let json: serde_json::Value = serde_json::from_str(&output).expect("code mode json");
        assert_eq!(json["ok"].as_bool(), Some(true));
        assert_eq!(json["result"]["kind"].as_str(), Some("explain"));
        assert_eq!(json["result"]["id"].as_str(), Some("fallow/unused-export"));
        assert_eq!(json["calls"][0]["tool"].as_str(), Some("fallow_explain"));
        assert_eq!(json["calls"][0]["ok"].as_bool(), Some(true));
    }

    fn git(root: &std::path::Path, args: &[&str]) {
        let status = Command::new("git")
            .args(args)
            .current_dir(root)
            .status()
            .expect("git command starts");
        assert!(status.success(), "git command failed: {args:?}");
    }

    #[test]
    fn api_backed_project_info_does_not_spawn_binary() {
        let temp = tempfile::tempdir().expect("tempdir");
        fs::create_dir(temp.path().join("src")).expect("src dir");
        fs::write(
            temp.path().join("package.json"),
            r#"{"name":"code-mode-project-info-test","type":"module","main":"src/index.ts"}"#,
        )
        .expect("package json");
        fs::write(
            temp.path().join("src/index.ts"),
            "export const value = 1;\n",
        )
        .expect("source");

        let output = execute_code_mode(
            "/definitely/not/fallow".to_string(),
            CodeExecuteParams {
                code: "return fallow.projectInfo({ files: true, no_cache: true });".to_string(),
                root: Some(temp.path().display().to_string()),
                timeout_ms: Some(5_000),
                max_output_bytes: Some(200_000),
            },
        )
        .expect("api-backed projectInfo should not need the binary");

        let json: serde_json::Value = serde_json::from_str(&output).expect("code mode json");
        assert_eq!(json["ok"].as_bool(), Some(true));
        assert_eq!(json["result"]["file_count"], 1);
        assert_eq!(json["result"]["files"][0], "src/index.ts");
        assert_eq!(json["calls"][0]["tool"].as_str(), Some("project_info"));
        assert_eq!(json["calls"][0]["ok"].as_bool(), Some(true));
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

    #[test]
    fn api_host_calls_check_deadline_before_starting() {
        let result = run_api_tool_with_deadline(
            CodeModeTool::ProjectInfo,
            serde_json::json!({}),
            Instant::now()
                .checked_sub(Duration::from_millis(1))
                .expect("past instant"),
        );

        let err = result.expect_err("expired deadline must reject API host calls");
        assert_eq!(err, "code mode execution timed out");
    }

    #[test]
    fn api_host_calls_time_out_while_running() {
        let result = run_api_tool_with_deadline_and_runner(
            CodeModeTool::ProjectInfo,
            serde_json::json!({}),
            Instant::now() + Duration::from_millis(1),
            |_tool, _params| {
                std::thread::sleep(Duration::from_millis(50));
                Ok(Some(serde_json::json!({"ok": true})))
            },
        );

        let err = result.expect_err("slow API host call must hit the external timeout");
        assert_eq!(err, "code mode execution timed out while running fallow");
    }

    // ---- CodeModeTool::from_name round-trip --------------------------------

    #[test]
    fn all_valid_tool_names_parse_successfully() {
        let valid = [
            "analyze",
            "combined",
            "check_changed",
            "security_candidates",
            "find_dupes",
            "project_info",
            "trace_export",
            "trace_file",
            "impact_closure",
            "trace_dependency",
            "trace_clone",
            "check_health",
            "audit",
            "fallow_explain",
            "list_boundaries",
            "feature_flags",
            "impact",
            "check_runtime_coverage",
            "get_hot_paths",
            "get_blast_radius",
            "get_importance",
            "get_cleanup_candidates",
        ];
        for name in valid {
            assert!(
                CodeModeTool::from_name(name).is_ok(),
                "expected '{name}' to parse"
            );
        }
    }

    #[test]
    fn unknown_tool_name_returns_unsupported_error() {
        let Err(err) = CodeModeTool::from_name("nonexistent_tool") else {
            panic!("expected Err for unknown tool")
        };
        assert!(
            err.contains("unsupported code mode fallow tool"),
            "error was: {err}"
        );
        assert!(err.contains("nonexistent_tool"), "error was: {err}");
    }

    #[test]
    fn fix_preview_returns_no_fix_tools_error() {
        let Err(err) = CodeModeTool::from_name("fix_preview") else {
            panic!("expected Err for fix_preview")
        };
        assert!(
            err.contains("does not expose fix tools"),
            "error was: {err}"
        );
    }

    #[test]
    fn fix_apply_returns_no_fix_tools_error() {
        let Err(err) = CodeModeTool::from_name("fix_apply") else {
            panic!("expected Err for fix_apply")
        };
        assert!(
            err.contains("does not expose fix tools"),
            "error was: {err}"
        );
    }

    #[test]
    fn tool_name_round_trips_through_from_name_and_name() {
        let pairs: &[(&str, &str)] = &[
            ("analyze", "analyze"),
            ("combined", "combined"),
            ("check_changed", "check_changed"),
            ("security_candidates", "security_candidates"),
            ("find_dupes", "find_dupes"),
            ("project_info", "project_info"),
            ("trace_export", "trace_export"),
            ("trace_file", "trace_file"),
            ("impact_closure", "impact_closure"),
            ("trace_dependency", "trace_dependency"),
            ("trace_clone", "trace_clone"),
            ("check_health", "check_health"),
            ("audit", "audit"),
            ("fallow_explain", "fallow_explain"),
            ("list_boundaries", "list_boundaries"),
            ("feature_flags", "feature_flags"),
            ("impact", "impact"),
            ("check_runtime_coverage", "check_runtime_coverage"),
            ("get_hot_paths", "get_hot_paths"),
            ("get_blast_radius", "get_blast_radius"),
            ("get_importance", "get_importance"),
            ("get_cleanup_candidates", "get_cleanup_candidates"),
        ];
        for (input, expected_name) in pairs {
            let tool = CodeModeTool::from_name(input).unwrap();
            assert_eq!(
                tool.name(),
                *expected_name,
                "name() mismatch for input '{input}'"
            );
        }
    }

    // ---- classify_host_error -----------------------------------------------

    #[test]
    fn classify_unsupported_tool_via_does_not_expose() {
        assert_eq!(
            classify_host_error("code mode does not expose fix tools; use standalone MCP tools"),
            "unsupported_tool"
        );
    }

    #[test]
    fn classify_unsupported_tool_via_unsupported_code_mode() {
        assert_eq!(
            classify_host_error("unsupported code mode fallow tool 'bad_name'"),
            "unsupported_tool"
        );
    }

    #[test]
    fn classify_timeout_error() {
        assert_eq!(
            classify_host_error("code mode execution timed out"),
            "timeout"
        );
    }

    #[test]
    fn classify_output_limit_via_host_output_exceeded() {
        assert_eq!(
            classify_host_error("code mode host output exceeded 1000000 bytes"),
            "output_limit"
        );
    }

    #[test]
    fn classify_output_limit_via_output_byte_counter() {
        assert_eq!(
            classify_host_error("code mode output byte counter overflowed"),
            "output_limit"
        );
    }

    #[test]
    fn classify_invalid_params_via_invalid_params_json() {
        assert_eq!(
            classify_host_error("invalid params JSON: unexpected end of input"),
            "invalid_params"
        );
    }

    #[test]
    fn classify_invalid_params_via_params_must_be_object() {
        assert_eq!(
            classify_host_error("fallow host call params must be an object"),
            "invalid_params"
        );
    }

    #[test]
    fn classify_invalid_params_via_invalid_tool_params() {
        assert_eq!(
            classify_host_error("invalid tool params: missing field `file`"),
            "invalid_params"
        );
    }

    #[test]
    fn classify_unknown_error_falls_back_to_subprocess() {
        assert_eq!(
            classify_host_error("failed to execute fallow binary 'fallow': No such file"),
            "subprocess"
        );
    }

    // ---- merge_default_root ------------------------------------------------

    #[test]
    fn merge_default_root_no_default_leaves_params_unchanged() {
        let params = merge_default_root(r#"{"files":true}"#, None).unwrap();
        assert_eq!(params["files"], true);
        assert!(params.get("root").is_none());
    }

    #[test]
    fn merge_default_root_invalid_json_returns_error() {
        let err = merge_default_root("{invalid", Some("/tmp/p")).unwrap_err();
        assert!(err.contains("invalid params JSON"), "error was: {err}");
    }

    #[test]
    fn merge_default_root_numeric_value_is_rejected() {
        let err = merge_default_root("42", Some("/tmp/p")).unwrap_err();
        assert!(err.contains("params must be an object"), "error was: {err}");
    }

    #[test]
    fn merge_default_root_string_value_is_rejected() {
        let err = merge_default_root(r#""hello""#, Some("/tmp/p")).unwrap_err();
        assert!(err.contains("params must be an object"), "error was: {err}");
    }

    #[test]
    fn merge_default_root_boolean_value_is_rejected() {
        let err = merge_default_root("true", Some("/tmp/p")).unwrap_err();
        assert!(err.contains("params must be an object"), "error was: {err}");
    }

    #[test]
    fn merge_default_root_empty_object_gets_root_injected() {
        let params = merge_default_root("{}", Some("/repo")).unwrap();
        assert_eq!(params["root"], "/repo");
    }

    // ---- normalize_code_mode_error -----------------------------------------

    #[test]
    fn interrupted_before_deadline_is_not_timeout() {
        let future_deadline = std::time::Instant::now() + std::time::Duration::from_mins(1);
        let result = normalize_code_mode_error("interrupted", future_deadline);
        assert_eq!(result, "interrupted");
    }

    #[test]
    fn interrupted_after_deadline_becomes_timeout_message() {
        let past_deadline = std::time::Instant::now()
            .checked_sub(std::time::Duration::from_millis(1))
            .unwrap();
        let result = normalize_code_mode_error("interrupted", past_deadline);
        assert_eq!(result, "code mode execution timed out");
    }

    #[test]
    fn non_interrupted_error_is_passed_through() {
        let past_deadline = std::time::Instant::now()
            .checked_sub(std::time::Duration::from_millis(1))
            .unwrap();
        let result = normalize_code_mode_error("some other error", past_deadline);
        assert_eq!(result, "some other error");
    }

    // ---- code_mode_limits --------------------------------------------------

    #[test]
    fn code_mode_limits_contains_expected_fields() {
        let limits = code_mode_limits(5_000, 1_000_000);
        assert_eq!(limits["timeout_ms"], 5_000_u64);
        assert_eq!(limits["max_output_bytes"], 1_000_000_u64);
        assert_eq!(limits["max_host_calls"], MAX_HOST_CALLS as u64);
    }

    // ---- user_source -------------------------------------------------------

    #[test]
    fn function_keyword_expression_is_preserved() {
        let source = user_source("function myFn() { return 42; }");
        assert!(source.contains("function myFn()"), "source was: {source}");
    }

    #[test]
    fn parenthesized_expression_is_preserved() {
        let source = user_source("({ fallow }) => ({ ok: true })");
        assert!(
            source.contains("({ fallow }) => ({ ok: true })"),
            "source was: {source}"
        );
    }

    #[test]
    fn user_source_always_includes_use_strict() {
        let source = user_source("return 1;");
        assert!(source.contains("\"use strict\""), "source was: {source}");
    }

    #[test]
    fn user_source_wraps_non_function_check() {
        let source = user_source("return 1;");
        assert!(
            source.contains("code must evaluate to a function or function body"),
            "source was: {source}"
        );
    }

    // ---- normalize_output --------------------------------------------------

    #[test]
    fn exit_code_zero_with_stdout_returns_stdout() {
        let result = normalize_output(0, b"{ \"ok\": true }", b"");
        assert_eq!(result.unwrap(), "{ \"ok\": true }");
    }

    #[test]
    fn exit_code_one_with_stdout_returns_stdout() {
        let result = normalize_output(1, b"{ \"findings\": [] }", b"");
        assert_eq!(result.unwrap(), "{ \"findings\": [] }");
    }

    #[test]
    fn exit_code_zero_with_empty_stdout_returns_empty_object() {
        let result = normalize_output(0, b"", b"");
        assert_eq!(result.unwrap(), "{}");
    }

    #[test]
    fn exit_code_one_with_empty_stdout_returns_empty_object() {
        let result = normalize_output(1, b"", b"");
        assert_eq!(result.unwrap(), "{}");
    }

    #[test]
    fn nonzero_exit_with_valid_json_stdout_returns_err_with_stdout() {
        let json_stdout = b"{ \"error\": true, \"message\": \"config error\" }";
        let err = normalize_output(2, json_stdout, b"").unwrap_err();
        assert_eq!(err, String::from_utf8_lossy(json_stdout));
    }

    #[test]
    fn nonzero_exit_with_empty_stdout_returns_err_with_exit_code() {
        let err = normalize_output(2, b"", b"").unwrap_err();
        let parsed: serde_json::Value = serde_json::from_str(&err).unwrap();
        assert_eq!(parsed["error"], true);
        assert_eq!(parsed["exit_code"], 2);
        assert!(
            parsed["message"]
                .as_str()
                .is_some_and(|m| m.contains("exit")),
            "message was: {}",
            parsed["message"]
        );
    }

    #[test]
    fn nonzero_exit_with_stderr_uses_stderr_as_message() {
        let err = normalize_output(3, b"", b"  some stderr text  ").unwrap_err();
        let parsed: serde_json::Value = serde_json::from_str(&err).unwrap();
        assert_eq!(parsed["error"], true);
        assert_eq!(parsed["exit_code"], 3);
        assert_eq!(parsed["message"], "some stderr text");
    }

    #[test]
    fn nonzero_exit_with_invalid_json_stdout_and_empty_stderr_returns_exit_code_message() {
        let err = normalize_output(5, b"not-json", b"").unwrap_err();
        let parsed: serde_json::Value = serde_json::from_str(&err).unwrap();
        assert_eq!(parsed["error"], true);
        assert_eq!(parsed["exit_code"], 5);
        assert!(
            parsed["message"].as_str().is_some_and(|m| m.contains('5')),
            "message was: {}",
            parsed["message"]
        );
    }

    #[test]
    fn nonzero_exit_negative_one_with_stderr_uses_stderr() {
        let err = normalize_output(-1, b"", b"process killed by signal").unwrap_err();
        let parsed: serde_json::Value = serde_json::from_str(&err).unwrap();
        assert_eq!(parsed["exit_code"], -1);
        assert_eq!(parsed["message"], "process killed by signal");
    }

    // ---- build_tool_args dispatch ------------------------------------------

    #[test]
    fn build_tool_args_analyze_includes_dead_code_subcommand() {
        let params = serde_json::json!({ "root": "/tmp/proj" });
        let args =
            build_tool_args(CodeModeTool::Analyze, params).expect("analyze args should build");
        assert!(args.contains(&"dead-code".to_string()));
        assert!(args.contains(&"--format".to_string()));
        assert!(args.contains(&"json".to_string()));
    }

    #[test]
    fn build_tool_args_combined_uses_bare_command_flags() {
        let params = serde_json::json!({ "root": "/tmp/proj", "dupes_mode": "semantic" });
        let args =
            build_tool_args(CodeModeTool::Combined, params).expect("combined args should build");
        assert!(!args.contains(&"dead-code".to_string()));
        assert!(args.contains(&"--format".to_string()));
        assert!(args.contains(&"json".to_string()));
        assert!(args.contains(&"--dupes-mode".to_string()));
        assert!(args.contains(&"semantic".to_string()));
    }

    #[test]
    fn build_tool_args_find_dupes_includes_dupes_subcommand() {
        let params = serde_json::json!({ "root": "/tmp/proj" });
        let args =
            build_tool_args(CodeModeTool::FindDupes, params).expect("find_dupes args should build");
        assert!(args.contains(&"dupes".to_string()));
    }

    #[test]
    fn build_tool_args_project_info_includes_list_subcommand() {
        let params = serde_json::json!({});
        let args = build_tool_args(CodeModeTool::ProjectInfo, params)
            .expect("project_info args should build");
        assert!(args.contains(&"list".to_string()));
    }

    #[test]
    fn build_tool_args_check_changed_includes_changed_since_flag() {
        let params = serde_json::json!({ "since": "main" });
        let args = build_tool_args(CodeModeTool::CheckChanged, params)
            .expect("check_changed args should build");
        assert!(args.contains(&"--changed-since".to_string()));
        assert!(args.contains(&"main".to_string()));
    }

    #[test]
    fn build_tool_args_security_candidates_includes_security_subcommand() {
        let params = serde_json::json!({});
        let args = build_tool_args(CodeModeTool::SecurityCandidates, params)
            .expect("security_candidates args should build");
        assert!(args.contains(&"security".to_string()));
    }

    #[test]
    fn build_tool_args_trace_export_includes_trace_flag() {
        let params = serde_json::json!({
            "file": "src/index.ts",
            "export_name": "MyFn"
        });
        let args = build_tool_args(CodeModeTool::TraceExport, params)
            .expect("trace_export args should build");
        assert!(args.contains(&"--trace".to_string()));
        assert!(args.iter().any(|a| a.contains("src/index.ts")));
    }

    #[test]
    fn build_tool_args_trace_file_includes_trace_file_flag() {
        let params = serde_json::json!({ "file": "src/utils.ts" });
        let args =
            build_tool_args(CodeModeTool::TraceFile, params).expect("trace_file args should build");
        assert!(args.contains(&"--trace-file".to_string()));
        assert!(args.contains(&"src/utils.ts".to_string()));
    }

    #[test]
    fn build_tool_args_impact_closure_includes_impact_closure_flag() {
        let params = serde_json::json!({ "path": "src/utils.ts" });
        let args = build_tool_args(CodeModeTool::ImpactClosure, params)
            .expect("impact_closure args should build");
        assert!(args.contains(&"--impact-closure".to_string()));
        assert!(args.contains(&"src/utils.ts".to_string()));
    }

    #[test]
    fn build_tool_args_trace_dependency_includes_trace_dependency_flag() {
        let params = serde_json::json!({ "package_name": "lodash" });
        let args = build_tool_args(CodeModeTool::TraceDependency, params)
            .expect("trace_dependency args should build");
        assert!(args.contains(&"--trace-dependency".to_string()));
        assert!(args.contains(&"lodash".to_string()));
    }

    #[test]
    fn build_tool_args_trace_clone_with_fingerprint_includes_trace_flag() {
        let params = serde_json::json!({ "fingerprint": "dup:abcd1234" });
        let args = build_tool_args(CodeModeTool::TraceClone, params)
            .expect("trace_clone args should build");
        assert!(args.contains(&"--trace".to_string()));
        assert!(args.contains(&"dup:abcd1234".to_string()));
    }

    #[test]
    fn build_tool_args_check_health_includes_health_subcommand() {
        let params = serde_json::json!({});
        let args = build_tool_args(CodeModeTool::CheckHealth, params)
            .expect("check_health args should build");
        assert!(args.contains(&"health".to_string()));
    }

    #[test]
    fn build_tool_args_audit_includes_audit_subcommand() {
        let params = serde_json::json!({});
        let args = build_tool_args(CodeModeTool::Audit, params).expect("audit args should build");
        assert!(args.contains(&"audit".to_string()));
    }

    #[test]
    fn build_tool_args_fallow_explain_includes_explain_subcommand() {
        let params = serde_json::json!({ "issue_type": "unused-export" });
        let args = build_tool_args(CodeModeTool::FallowExplain, params)
            .expect("fallow_explain args should build");
        assert!(args.contains(&"explain".to_string()));
    }

    #[test]
    fn build_tool_args_list_boundaries_includes_boundaries_flag() {
        let params = serde_json::json!({});
        let args = build_tool_args(CodeModeTool::ListBoundaries, params)
            .expect("list_boundaries args should build");
        assert!(args.contains(&"--boundaries".to_string()));
    }

    #[test]
    fn build_tool_args_feature_flags_includes_flags_subcommand() {
        let params = serde_json::json!({});
        let args = build_tool_args(CodeModeTool::FeatureFlags, params)
            .expect("feature_flags args should build");
        assert!(args.contains(&"flags".to_string()));
    }

    #[test]
    fn build_tool_args_impact_includes_impact_subcommand() {
        let params = serde_json::json!({});
        let args = build_tool_args(CodeModeTool::Impact, params).expect("impact args should build");
        assert!(args.contains(&"impact".to_string()));
    }

    #[test]
    fn build_tool_args_check_runtime_coverage_includes_runtime_coverage_flag() {
        let params = serde_json::json!({ "coverage": "./coverage" });
        let args = build_tool_args(CodeModeTool::CheckRuntimeCoverage, params)
            .expect("check_runtime_coverage args should build");
        assert!(args.contains(&"--runtime-coverage".to_string()));
        assert!(args.contains(&"./coverage".to_string()));
    }

    #[test]
    fn build_tool_args_get_hot_paths_includes_runtime_coverage_flag() {
        let params = serde_json::json!({ "coverage": "./cov" });
        let args = build_tool_args(CodeModeTool::GetHotPaths, params)
            .expect("get_hot_paths args should build");
        assert!(args.contains(&"--runtime-coverage".to_string()));
    }

    #[test]
    fn build_tool_args_get_blast_radius_includes_runtime_coverage_flag() {
        let params = serde_json::json!({ "coverage": "./cov" });
        let args = build_tool_args(CodeModeTool::GetBlastRadius, params)
            .expect("get_blast_radius args should build");
        assert!(args.contains(&"--runtime-coverage".to_string()));
    }

    #[test]
    fn build_tool_args_get_importance_includes_runtime_coverage_flag() {
        let params = serde_json::json!({ "coverage": "./cov" });
        let args = build_tool_args(CodeModeTool::GetImportance, params)
            .expect("get_importance args should build");
        assert!(args.contains(&"--runtime-coverage".to_string()));
    }

    #[test]
    fn build_tool_args_get_cleanup_candidates_includes_runtime_coverage_flag() {
        let params = serde_json::json!({ "coverage": "./cov" });
        let args = build_tool_args(CodeModeTool::GetCleanupCandidates, params)
            .expect("get_cleanup_candidates args should build");
        assert!(args.contains(&"--runtime-coverage".to_string()));
    }

    // ---- build_tool_args invalid-params rejection --------------------------

    #[test]
    fn build_tool_args_check_changed_missing_since_returns_error() {
        let params = serde_json::json!({});
        let err = build_tool_args(CodeModeTool::CheckChanged, params).unwrap_err();
        assert!(err.contains("invalid tool params"), "error was: {err}");
    }

    #[test]
    fn build_tool_args_trace_export_missing_file_returns_error() {
        let params = serde_json::json!({ "export_name": "MyFn" });
        let err = build_tool_args(CodeModeTool::TraceExport, params).unwrap_err();
        assert!(
            err.contains("invalid tool params") || err.contains("must not be empty"),
            "error was: {err}"
        );
    }

    #[test]
    fn build_tool_args_trace_file_missing_file_returns_error() {
        let params = serde_json::json!({});
        let err = build_tool_args(CodeModeTool::TraceFile, params).unwrap_err();
        assert!(
            err.contains("invalid tool params") || err.contains("must not be empty"),
            "error was: {err}"
        );
    }

    #[test]
    fn build_tool_args_trace_dependency_missing_package_name_returns_error() {
        let params = serde_json::json!({});
        let err = build_tool_args(CodeModeTool::TraceDependency, params).unwrap_err();
        assert!(
            err.contains("invalid tool params") || err.contains("must not be empty"),
            "error was: {err}"
        );
    }

    // ---- execute_code_mode: sandbox behavior (no real fallow binary) -------

    #[test]
    fn snippet_that_is_not_a_function_is_rejected() {
        // A string literal like "hello" parses as a paren-expression that wraps
        // to a non-function value, triggering the type-check throw.
        let output = execute_code_mode(
            "fallow".to_string(),
            CodeExecuteParams {
                code: r#"("hello")"#.to_string(),
                root: None,
                timeout_ms: Some(5_000),
                max_output_bytes: Some(10_000),
            },
        )
        .expect_err("non-function snippet should be rejected");

        let json: serde_json::Value = serde_json::from_str(&output).unwrap();
        assert_eq!(json["ok"].as_bool(), Some(false));
        assert!(
            json["error"]
                .as_str()
                .is_some_and(|e| e.contains("code must evaluate to a function")),
            "error was: {}",
            json["error"]
        );
    }

    #[test]
    fn snippet_returning_json_value_succeeds() {
        let output = execute_code_mode(
            "fallow".to_string(),
            CodeExecuteParams {
                code: "return { status: \"ok\", count: 3 };".to_string(),
                root: None,
                timeout_ms: Some(5_000),
                max_output_bytes: Some(10_000),
            },
        )
        .expect("returning a plain object should succeed");

        let json: serde_json::Value = serde_json::from_str(&output).unwrap();
        assert_eq!(json["ok"].as_bool(), Some(true));
        assert_eq!(json["result"]["status"], "ok");
        assert_eq!(json["result"]["count"], 3);
        assert_eq!(json["schema_version"], "mcp-code-execute/v1");
    }

    #[test]
    fn snippet_can_access_root_from_params() {
        let output = execute_code_mode(
            "fallow".to_string(),
            CodeExecuteParams {
                code: "return root;".to_string(),
                root: Some("/my/project".to_string()),
                timeout_ms: Some(5_000),
                max_output_bytes: Some(10_000),
            },
        )
        .expect("root access should succeed");

        let json: serde_json::Value = serde_json::from_str(&output).unwrap();
        assert_eq!(json["ok"].as_bool(), Some(true));
        assert_eq!(json["result"], "/my/project");
    }

    #[test]
    fn snippet_returning_null_produces_null_result() {
        let output = execute_code_mode(
            "fallow".to_string(),
            CodeExecuteParams {
                code: "return null;".to_string(),
                root: None,
                timeout_ms: Some(5_000),
                max_output_bytes: Some(10_000),
            },
        )
        .expect("null return should succeed");

        let json: serde_json::Value = serde_json::from_str(&output).unwrap();
        assert_eq!(json["ok"].as_bool(), Some(true));
        assert_eq!(json["result"], serde_json::Value::Null);
    }

    #[test]
    fn snippet_throwing_error_populates_error_field() {
        let output = execute_code_mode(
            "fallow".to_string(),
            CodeExecuteParams {
                code: r#"throw new Error("intentional test error");"#.to_string(),
                root: None,
                timeout_ms: Some(5_000),
                max_output_bytes: Some(10_000),
            },
        )
        .expect_err("throwing should produce Err");

        let json: serde_json::Value = serde_json::from_str(&output).unwrap();
        assert_eq!(json["ok"].as_bool(), Some(false));
        assert!(
            json["error"]
                .as_str()
                .is_some_and(|e| e.contains("intentional test error")),
            "error was: {}",
            json["error"]
        );
    }

    #[test]
    fn response_always_includes_limits_block() {
        let output = execute_code_mode(
            "fallow".to_string(),
            CodeExecuteParams {
                code: "return 1;".to_string(),
                root: None,
                timeout_ms: Some(2_000),
                max_output_bytes: Some(50_000),
            },
        )
        .expect("should succeed");

        let json: serde_json::Value = serde_json::from_str(&output).unwrap();
        assert_eq!(json["limits"]["timeout_ms"], 2_000_u64);
        assert_eq!(json["limits"]["max_output_bytes"], 50_000_u64);
        assert_eq!(json["limits"]["max_host_calls"], MAX_HOST_CALLS as u64);
    }

    #[test]
    fn timeout_is_capped_at_max_timeout_ms() {
        let output = execute_code_mode(
            "fallow".to_string(),
            CodeExecuteParams {
                code: "return 1;".to_string(),
                root: None,
                timeout_ms: Some(MAX_TIMEOUT_MS + 99_999),
                max_output_bytes: Some(10_000),
            },
        )
        .expect("should succeed with capped timeout");

        let json: serde_json::Value = serde_json::from_str(&output).unwrap();
        assert_eq!(json["limits"]["timeout_ms"], MAX_TIMEOUT_MS);
    }

    #[test]
    fn max_output_bytes_is_capped_at_max_output_bytes_constant() {
        let output = execute_code_mode(
            "fallow".to_string(),
            CodeExecuteParams {
                code: "return 1;".to_string(),
                root: None,
                timeout_ms: Some(5_000),
                max_output_bytes: Some(MAX_OUTPUT_BYTES + 1),
            },
        )
        .expect("should succeed with capped output limit");

        let json: serde_json::Value = serde_json::from_str(&output).unwrap();
        assert_eq!(json["limits"]["max_output_bytes"], MAX_OUTPUT_BYTES as u64);
    }

    #[test]
    fn missing_timeout_uses_default() {
        let output = execute_code_mode(
            "fallow".to_string(),
            CodeExecuteParams {
                code: "return 1;".to_string(),
                root: None,
                timeout_ms: None,
                max_output_bytes: None,
            },
        )
        .expect("should succeed with defaults");

        let json: serde_json::Value = serde_json::from_str(&output).unwrap();
        assert_eq!(json["limits"]["timeout_ms"], DEFAULT_TIMEOUT_MS);
        assert_eq!(
            json["limits"]["max_output_bytes"],
            DEFAULT_MAX_OUTPUT_BYTES as u64
        );
    }

    #[test]
    fn hardened_globals_are_inaccessible_in_snippet() {
        for blocked in ["fetch", "process", "require", "Deno", "Bun"] {
            let output = execute_code_mode(
                "fallow".to_string(),
                CodeExecuteParams {
                    code: format!("return typeof {blocked};"),
                    root: None,
                    timeout_ms: Some(5_000),
                    max_output_bytes: Some(10_000),
                },
            )
            .expect("typeof check should not throw");

            let json: serde_json::Value = serde_json::from_str(&output).unwrap();
            assert_eq!(
                json["result"], "undefined",
                "{blocked} should be undefined in sandbox"
            );
        }
    }

    #[test]
    fn fallow_object_is_accessible_in_snippet() {
        let output = execute_code_mode(
            "fallow".to_string(),
            CodeExecuteParams {
                code: "return typeof fallow;".to_string(),
                root: None,
                timeout_ms: Some(5_000),
                max_output_bytes: Some(10_000),
            },
        )
        .expect("fallow typeof should succeed");

        let json: serde_json::Value = serde_json::from_str(&output).unwrap();
        assert_eq!(json["result"], "object");
    }

    #[test]
    fn fallow_run_is_callable_and_fails_fast_on_missing_binary() {
        let output = execute_code_mode(
            "nonexistent-binary-xyz-12345".to_string(),
            CodeExecuteParams {
                code: r#"return fallow.run("impact", {});"#.to_string(),
                root: Some("/tmp".to_string()),
                timeout_ms: Some(5_000),
                max_output_bytes: Some(10_000),
            },
        )
        .expect_err("missing binary should produce Err");

        let json: serde_json::Value = serde_json::from_str(&output).unwrap();
        assert_eq!(json["ok"].as_bool(), Some(false));
        assert_eq!(json["calls"].as_array().map(Vec::len), Some(1));
        let call = &json["calls"][0];
        assert_eq!(call["tool"], "impact");
        assert_eq!(call["ok"], false);
        assert_eq!(call["error_kind"], "subprocess");
    }

    #[test]
    fn fallow_run_with_unsupported_tool_records_unsupported_tool_error_kind() {
        let output = execute_code_mode(
            "fallow".to_string(),
            CodeExecuteParams {
                code: r#"return fallow.run("fix_apply", {});"#.to_string(),
                root: None,
                timeout_ms: Some(5_000),
                max_output_bytes: Some(10_000),
            },
        )
        .expect_err("fix_apply should be rejected");

        let json: serde_json::Value = serde_json::from_str(&output).unwrap();
        assert_eq!(json["ok"].as_bool(), Some(false));
        assert_eq!(json["calls"].as_array().map(Vec::len), Some(1));
        let call = &json["calls"][0];
        assert_eq!(call["error_kind"], "unsupported_tool");
        assert_eq!(call["ok"], false);
    }

    #[test]
    fn successful_response_has_empty_calls_array_when_no_host_calls_made() {
        let output = execute_code_mode(
            "fallow".to_string(),
            CodeExecuteParams {
                code: "return { computed: 1 + 2 };".to_string(),
                root: None,
                timeout_ms: Some(5_000),
                max_output_bytes: Some(10_000),
            },
        )
        .expect("pure computation should succeed");

        let json: serde_json::Value = serde_json::from_str(&output).unwrap();
        assert_eq!(json["ok"], true);
        assert_eq!(json["calls"].as_array().map(Vec::len), Some(0));
    }
}
