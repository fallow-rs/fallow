use std::cell::RefCell;
use std::collections::BTreeMap;
#[cfg(test)]
use std::fs;
#[cfg(test)]
use std::process::Command;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicU8, AtomicUsize, Ordering};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use rquickjs::prelude::{Func, MutFn};
use rquickjs::{Context, Ctx, Error as JsError, Exception, FromJs, Object, Runtime, Value};
use rustc_hash::{FxHashMap, FxHashSet};
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

/// How many subprocess-backed host calls one `fallow.all` batch runs at once.
/// The goal is overlap, not a thread per element: every worker also owns a
/// child `fallow` process, so the fan-out stays deliberately small.
const MAX_BATCH_CONCURRENCY: usize = 4;

/// How many host calls refused before dispatch are recorded. Rejections cost
/// no analysis and no output, so they do not consume [`MAX_HOST_CALLS`]; this
/// separate bound keeps a snippet that loops over bad tool names from growing
/// the `calls[]` array without limit.
const MAX_REJECTED_HOST_CALLS: usize = 8;

/// Longest tool name echoed back into `calls[]` and into rejection messages.
/// Every real tool name is far shorter, so clamping bounds what an unvalidated
/// `fallow.run(<huge string>)` can push into the response envelope.
const MAX_TOOL_NAME_BYTES: usize = 64;

/// How much of an oversized snippet result is kept as a preview, so the
/// rejection still shows what the snippet was about to return.
const RESULT_PREVIEW_BYTES: usize = 256;

/// Floor for the envelope's `error` string. A thrown message reaches the
/// calling agent the same way a result does, so it is clamped too, but never
/// below a length that keeps a structured programmatic error readable.
const MAX_ERROR_BYTES: usize = 4_096;

#[path = "code_mode_subprocess.rs"]
mod code_mode_subprocess;
#[path = "code_mode_tools.rs"]
mod code_mode_tools;

#[cfg(test)]
use code_mode_subprocess::normalize_output;
use code_mode_subprocess::run_fallow_sync;
#[cfg(test)]
pub use code_mode_tools::code_mode_subprocess_aliases;
use code_mode_tools::{
    CODE_MODE_ALIASES, CodeModeBacking, CodeModeTool, build_tool_args, merge_default_root,
    run_api_tool,
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
    let error_limit = max_output_bytes.max(MAX_ERROR_BYTES);
    if params.code.len() > MAX_CODE_BYTES {
        return Err(error_envelope(
            &format!("code mode snippet exceeded {MAX_CODE_BYTES} bytes"),
            &[],
            &limits,
            error_limit,
        ));
    }
    let deadline = Instant::now() + Duration::from_millis(timeout_ms);

    let runtime = build_code_mode_runtime(deadline)?;
    let context =
        Context::full(&runtime).map_err(|err| format!("failed to create JS context: {err}"))?;
    let state = Rc::new(RefCell::new(CodeModeState::new(
        binary,
        params.root,
        deadline,
        max_output_bytes,
    )));

    let result = run_code_mode_eval(&context, &state, &params.code);

    let calls = &state.borrow().calls;
    match result {
        Ok(result_json) if result_json.len() > max_output_bytes => Err(json!({
            "schema_version": "mcp-code-execute/v1",
            "ok": false,
            "error": format!(
                "code mode result exceeded {max_output_bytes} bytes ({} returned); \
                 return a smaller projection",
                result_json.len()
            ),
            "truncated": true,
            "result_bytes": result_json.len(),
            "result_preview": clamp_utf8(&result_json, RESULT_PREVIEW_BYTES.min(max_output_bytes)),
            "calls": calls,
            "limits": limits
        })
        .to_string()),
        Ok(result_json) => Ok(json!({
            "schema_version": "mcp-code-execute/v1",
            "ok": true,
            "result": serde_json::from_str::<serde_json::Value>(&result_json)
                .unwrap_or(serde_json::Value::Null),
            "calls": calls,
            "limits": limits
        })
        .to_string()),
        Err(err) => Err(error_envelope(
            &normalize_code_mode_error(&err, deadline),
            calls,
            &limits,
            error_limit,
        )),
    }
}

/// Build a failed code-mode response, clamping `error` so a thrown message
/// cannot push megabytes into the calling agent's context the way an oversized
/// result would. The `truncated` and `error_bytes` fields appear only when the
/// clamp actually fired, so an ordinary failure keeps its existing shape.
fn error_envelope(
    error: &str,
    calls: &[CodeModeCall],
    limits: &serde_json::Value,
    max_error_bytes: usize,
) -> String {
    let mut envelope = json!({
        "schema_version": "mcp-code-execute/v1",
        "ok": false,
        "error": clamp_utf8(error, max_error_bytes),
        "calls": calls,
        "limits": limits
    });
    if error.len() > max_error_bytes
        && let Some(fields) = envelope.as_object_mut()
    {
        fields.insert("truncated".to_string(), json!(true));
        fields.insert("error_bytes".to_string(), json!(error.len()));
    }
    envelope.to_string()
}

/// Build the `limits` JSON block echoed on every code-mode response.
fn code_mode_limits(timeout_ms: u64, max_output_bytes: usize) -> serde_json::Value {
    json!({
        "timeout_ms": timeout_ms,
        "max_output_bytes": max_output_bytes,
        "max_host_calls": MAX_HOST_CALLS,
        "max_rejected_host_calls": MAX_REJECTED_HOST_CALLS
    })
}

/// Longest prefix of `value` that fits in `max` bytes without splitting a
/// character.
fn clamp_utf8(value: &str, max: usize) -> &str {
    if value.len() <= max {
        return value;
    }
    let mut end = max;
    while end > 0 && !value.is_char_boundary(end) {
        end -= 1;
    }
    &value[..end]
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
    ctx.eval::<(), _>(HARDEN_INTRINSICS)?;

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

/// Close the prototype route to the function constructors. Undefining the
/// `Function` binding hides the global but not the intrinsic:
/// `(function () {}).constructor` is the same compiler, and the async,
/// generator, and async-generator prototypes each carry their own. Replacing
/// those `constructor` slots with a non-configurable `undefined` removes
/// dynamic code compilation from the realm, which is what the published
/// contract claims. Ordinary snippets are untouched: class and literal
/// prototypes keep their own `constructor` properties.
const HARDEN_INTRINSICS: &str = r#"
    "use strict";
    (() => {
        const seal = (fn) => {
            const prototype = Object.getPrototypeOf(fn);
            if (prototype) {
                Object.defineProperty(prototype, "constructor", {
                    value: undefined,
                    writable: false,
                    enumerable: false,
                    configurable: false
                });
            }
        };
        seal(function () {});
        seal(async function () {});
        seal(function* () {});
        seal(async function* () {});
    })();
"#;

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

    let batch_state = Rc::clone(state);
    fallow.set(
        "all",
        Func::from(MutFn::from(move |ctx: Ctx<'js>, requests: Value<'js>| {
            run_host_batch(&ctx, &batch_state, requests)
        })),
    )?;

    for &(alias, tool) in CODE_MODE_ALIASES.as_slice() {
        let alias_state = Rc::clone(state);
        fallow.set(
            alias,
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

/// Message for every whole-batch shape rejection, so a snippet that passes the
/// wrong argument gets the same guidance wherever the parse fails.
const BATCH_SHAPE_ERROR: &str = "fallow.all expects an array of { tool, params } requests, where `tool` is the same wire \
     tool name fallow.run takes";

/// One `fallow.all` element as the snippet wrote it.
#[derive(serde::Deserialize)]
struct BatchRequest {
    tool: String,
    #[serde(default)]
    params: Option<serde_json::Value>,
}

/// Run a `fallow.all` fan-out. The snippet stays synchronous: this blocks
/// until every element has resolved, and returns them positionally aligned
/// with the request array.
fn run_host_batch<'js>(
    ctx: &Ctx<'js>,
    state: &Rc<RefCell<CodeModeState>>,
    requests: Value<'js>,
) -> rquickjs::Result<Value<'js>> {
    let requests =
        parse_batch_requests(ctx, requests).map_err(|err| Exception::throw_message(ctx, &err))?;
    let elements = {
        let mut state = state.borrow_mut();
        state.run_batch(&requests)
    }
    .map_err(|err| Exception::throw_message(ctx, &err))?;

    ctx.json_parse(elements)
}

/// Normalize the snippet's argument into `(tool name, params JSON)` pairs.
/// Element shape is a whole-batch problem: nothing has run yet, so refusing
/// the call is clearer than returning a batch of identical shape errors.
fn parse_batch_requests<'js>(
    ctx: &Ctx<'js>,
    requests: Value<'js>,
) -> Result<Vec<(String, String)>, String> {
    if !requests.is_array() {
        return Err(BATCH_SHAPE_ERROR.to_string());
    }
    let json = stringify_json(ctx, requests).map_err(|_| BATCH_SHAPE_ERROR.to_string())?;
    let parsed: Vec<BatchRequest> =
        serde_json::from_str(&json).map_err(|err| format!("{BATCH_SHAPE_ERROR}: {err}"))?;

    Ok(parsed
        .into_iter()
        .map(|request| {
            let params = match request.params {
                None | Some(serde_json::Value::Null) => "{}".to_string(),
                Some(params) => params.to_string(),
            };
            (request.tool, params)
        })
        .collect())
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
    rejected_calls: usize,
    /// Host calls that actually reached a backing. This is the only thing the
    /// `max_host_calls` budget is derived from, so entries `calls[]` records
    /// without running an analysis (memo hits, pre-dispatch rejections) cannot
    /// shrink the budget for the calls that do run.
    dispatched: usize,
    /// Host-call results already served in this snippet, keyed by
    /// [`memo_key`]. Bounded by [`MAX_HOST_CALLS`] entries, because only a
    /// dispatch that spent a slot inserts one, and by `max_output_bytes` in
    /// total size. It dies with the snippet: nothing persists between
    /// `code_execute` invocations.
    memo: FxHashMap<String, String>,
}

/// A host call that passed validation: the tool, its merged params, and the
/// key that identifies the pair for the snippet's memo.
struct ResolvedHostCall {
    tool: CodeModeTool,
    params: serde_json::Value,
    key: String,
}

impl CodeModeState {
    fn new(
        binary: String,
        default_root: Option<String>,
        deadline: Instant,
        max_output_bytes: usize,
    ) -> Self {
        Self {
            binary,
            default_root,
            deadline,
            max_output_bytes,
            output_bytes: 0,
            calls: Vec::new(),
            rejected_calls: 0,
            dispatched: 0,
            memo: FxHashMap::default(),
        }
    }

    /// Validate a host call before it can cost anything: resolve the tool
    /// name, merge the default root, and derive the memo key.
    fn resolve(&self, tool: &str, params_json: &str) -> Result<ResolvedHostCall, String> {
        let tool = CodeModeTool::from_name(tool)?;
        let params = merge_default_root(params_json, self.default_root.as_deref())?;
        let key = memo_key(tool, &params);
        Ok(ResolvedHostCall { tool, params, key })
    }

    /// Host-call slots left in the analysis budget. Only a call that reached a
    /// backing spends one, so pre-dispatch rejections and memo hits leave the
    /// budget for later distinct calls untouched even though `calls[]` records
    /// them.
    fn remaining_host_calls(&self) -> usize {
        MAX_HOST_CALLS.saturating_sub(self.dispatched)
    }

    fn remaining_output_bytes(&self) -> usize {
        self.max_output_bytes.saturating_sub(self.output_bytes)
    }

    /// Charge freshly read fallow JSON against the shared output budget. A
    /// memo hit reads nothing new, so it never calls this.
    fn charge_output(&mut self, bytes: usize) -> Result<(), String> {
        self.output_bytes = self
            .output_bytes
            .checked_add(bytes)
            .ok_or_else(|| "code mode output byte counter overflowed".to_string())?;
        if self.output_bytes > self.max_output_bytes {
            return Err(format!(
                "code mode host output exceeded {} bytes",
                self.max_output_bytes
            ));
        }
        Ok(())
    }

    fn record_success(&mut self, mut call: CodeModeCall) {
        call.ok = true;
        self.calls.push(call);
    }

    /// Record a failed host call and return the message the snippet sees.
    /// When the rejection budget is spent the call is not recorded at all and
    /// the budget error replaces the original message.
    fn record_failure(&mut self, call: CodeModeCall, error: String) -> String {
        let refused = rejected_before_dispatch(classify_host_error(&error));
        self.record_error(call, error, refused)
    }

    /// Record a host call the state itself refused before dispatch, whatever
    /// the message says. An exhausted output budget is such a refusal: it runs
    /// nothing, so it spends no `max_host_calls` slot, and it therefore has to
    /// be charged to the rejection budget instead or a loop of refusals could
    /// grow `calls[]` without bound.
    fn record_refusal(&mut self, call: CodeModeCall, error: String) -> String {
        self.record_error(call, error, true)
    }

    /// [`record_failure`](Self::record_failure) for a batch element, keeping a
    /// refusal the batch made on the element's behalf out of the analysis
    /// budget it never spent.
    fn record_element_failure(
        &mut self,
        call: CodeModeCall,
        error: String,
        outcome: Option<&BatchOutcome>,
    ) -> String {
        if outcome.is_some_and(|outcome| outcome.refused) {
            return self.record_refusal(call, error);
        }
        self.record_failure(call, error)
    }

    fn record_error(&mut self, mut call: CodeModeCall, error: String, refused: bool) -> String {
        if refused {
            if self.rejected_calls >= MAX_REJECTED_HOST_CALLS {
                return format!(
                    "code mode rejected host call limit exceeded ({MAX_REJECTED_HOST_CALLS})"
                );
            }
            self.rejected_calls += 1;
        }
        call.error_kind = Some(classify_host_error(&error));
        self.calls.push(call);
        error
    }

    fn run_tool(&mut self, tool: &str, params_json: &str) -> Result<String, String> {
        let name = clamp_utf8(tool, MAX_TOOL_NAME_BYTES);
        let started = Instant::now();
        let mut call = CodeModeCall::new(name);

        let resolved = match self.resolve(name, params_json) {
            Ok(resolved) => resolved,
            Err(err) => {
                call.duration_ms = started.elapsed().as_millis();
                return Err(self.record_failure(call, err));
            }
        };
        call.tool = resolved.tool.name().to_string();

        if let Some(cached) = self.memo.get(&resolved.key).cloned() {
            call.cache_hit = true;
            call.output_bytes = cached.len();
            call.duration_ms = started.elapsed().as_millis();
            self.record_success(call);
            return Ok(cached);
        }

        if self.remaining_host_calls() == 0 {
            return Err(format!(
                "code mode host call limit exceeded ({MAX_HOST_CALLS})"
            ));
        }
        if Instant::now() >= self.deadline {
            return Err("code mode execution timed out".to_string());
        }
        let budget = self.remaining_output_bytes();
        if budget == 0 {
            call.duration_ms = started.elapsed().as_millis();
            let error = format!(
                "code mode host output exceeded {} bytes",
                self.max_output_bytes
            );
            return Err(self.record_refusal(call, error));
        }

        self.dispatched += 1;
        let outcome = dispatch_host_call(
            &self.binary,
            resolved.tool,
            resolved.params,
            self.deadline,
            budget,
        );
        call.duration_ms = started.elapsed().as_millis();
        match outcome {
            Ok(stdout) => {
                call.output_bytes = stdout.len();
                if let Err(err) = self.charge_output(stdout.len()) {
                    return Err(self.record_failure(call, err));
                }
                self.memo.insert(resolved.key, stdout.clone());
                self.record_success(call);
                Ok(stdout)
            }
            Err(err) => Err(self.record_failure(call, err)),
        }
    }

    /// Run a `fallow.all` batch and return its per-element JSON array.
    ///
    /// Failure model: the batch itself fails only for problems that belong to
    /// the whole call (more elements than the analysis budget can pay for, a
    /// deadline that has already passed). Anything one element can get wrong
    /// is reported in that element's own slot as
    /// `{ "ok": false, "error": ... }`, so a single bad element never hides
    /// the results the other elements produced.
    fn run_batch(&mut self, requests: &[(String, String)]) -> Result<String, String> {
        if requests.is_empty() {
            return Ok("[]".to_string());
        }
        if requests.len() > MAX_HOST_CALLS {
            return Err(format!(
                "code mode batch of {} elements exceeds max_host_calls ({MAX_HOST_CALLS})",
                requests.len()
            ));
        }
        if Instant::now() >= self.deadline {
            return Err("code mode execution timed out".to_string());
        }

        let plan = self.plan_batch(requests);
        let remaining = self.remaining_host_calls();
        if plan.pending.len() > remaining {
            return Err(format!(
                "code mode batch needs {} host calls but only {remaining} of the \
                 max_host_calls ({MAX_HOST_CALLS}) budget remain",
                plan.pending.len()
            ));
        }

        let outcomes = self.dispatch_batch(plan.pending);
        Ok(self.finish_batch(plan.elements, &plan.repeated, &outcomes))
    }

    /// Resolve every element before anything runs, so the budget check sees
    /// the real number of dispatches: memo hits and repeats inside one batch
    /// are free.
    fn plan_batch(&self, requests: &[(String, String)]) -> BatchPlan {
        let mut plan = BatchPlan {
            elements: Vec::with_capacity(requests.len()),
            pending: Vec::new(),
            repeated: FxHashSet::default(),
        };

        for (tool, params_json) in requests {
            let name = clamp_utf8(tool, MAX_TOOL_NAME_BYTES);
            match self.resolve(name, params_json) {
                Err(error) => plan.elements.push(BatchElementPlan::Rejected {
                    tool: name.to_string(),
                    error,
                }),
                Ok(resolved) => {
                    let tool = resolved.tool;
                    if let Some(cached) = self.memo.get(&resolved.key) {
                        plan.elements.push(BatchElementPlan::Cached {
                            tool,
                            output: cached.clone(),
                        });
                    } else if plan.pending.iter().any(|call| call.key == resolved.key) {
                        plan.repeated.insert(resolved.key.clone());
                        plan.elements.push(BatchElementPlan::Repeated {
                            tool,
                            key: resolved.key,
                        });
                    } else {
                        plan.pending.push(PendingHostCall {
                            key: resolved.key.clone(),
                            tool,
                            params: resolved.params,
                        });
                        plan.elements.push(BatchElementPlan::Dispatch {
                            tool,
                            key: resolved.key,
                        });
                    }
                }
            }
        }

        plan
    }

    /// Run the batch's dispatches under a shared output budget.
    ///
    /// The elements overlap, so their caps have to be decided before any of
    /// them finishes: the remaining output budget is divided evenly across the
    /// dispatches, which keeps `max_output_bytes` a real total cap without
    /// making the outcome depend on which worker happened to report first.
    fn dispatch_batch(&mut self, pending: Vec<PendingHostCall>) -> FxHashMap<String, BatchOutcome> {
        if pending.is_empty() {
            return FxHashMap::default();
        }
        let share = self.remaining_output_bytes() / pending.len();
        if share == 0 {
            let error = format!(
                "code mode host output exceeded {} bytes",
                self.max_output_bytes
            );
            return pending
                .into_iter()
                .map(|call| {
                    (
                        call.key,
                        BatchOutcome {
                            duration_ms: 0,
                            result: Err(error.clone()),
                            refused: true,
                        },
                    )
                })
                .collect();
        }
        self.dispatched += pending.len();
        run_batch_dispatches(&self.binary, self.deadline, share, pending)
    }

    /// Charge, memoize, and record the batch in element order, then render the
    /// per-element array. Ordering the accounting by element position rather
    /// than by completion keeps the response identical however the fan-out
    /// happened to interleave.
    fn finish_batch(
        &mut self,
        plans: Vec<BatchElementPlan>,
        repeated: &FxHashSet<String>,
        outcomes: &FxHashMap<String, BatchOutcome>,
    ) -> String {
        let mut served: FxHashMap<String, serde_json::Value> = FxHashMap::default();
        let mut elements = Vec::with_capacity(plans.len());

        for plan in plans {
            let element = match plan {
                BatchElementPlan::Rejected { tool, error } => {
                    let call = CodeModeCall::new(&tool);
                    error_element(&self.record_failure(call, error))
                }
                BatchElementPlan::Cached { tool, output } => {
                    let mut call = CodeModeCall::new(tool.name());
                    call.cache_hit = true;
                    call.output_bytes = output.len();
                    match parse_host_output(&output) {
                        Ok(value) => {
                            self.record_success(call);
                            ok_element(value)
                        }
                        Err(error) => error_element(&self.record_failure(call, error)),
                    }
                }
                BatchElementPlan::Repeated { tool, key } => {
                    let mut call = CodeModeCall::new(tool.name());
                    call.cache_hit = true;
                    match served.get(&key) {
                        Some(value) => {
                            self.record_success(call);
                            ok_element(value.clone())
                        }
                        None => {
                            let outcome = outcomes.get(&key);
                            let error = failed_outcome_message(outcome);
                            error_element(&self.record_element_failure(call, error, outcome))
                        }
                    }
                }
                BatchElementPlan::Dispatch { tool, key } => {
                    let mut call = CodeModeCall::new(tool.name());
                    match outcomes.get(&key) {
                        Some(BatchOutcome {
                            duration_ms,
                            result: Ok(stdout),
                            ..
                        }) => {
                            call.duration_ms = *duration_ms;
                            call.output_bytes = stdout.len();
                            self.finish_dispatched_element(call, key, stdout, repeated, &mut served)
                        }
                        outcome => {
                            call.duration_ms = outcome.map_or(0, |outcome| outcome.duration_ms);
                            let error = failed_outcome_message(outcome);
                            error_element(&self.record_element_failure(call, error, outcome))
                        }
                    }
                }
            };
            elements.push(element);
        }

        serde_json::Value::Array(elements).to_string()
    }

    /// Charge, memoize, and render the one element that paid for a dispatch.
    /// Its parsed value is held back for reuse only when a later element of
    /// the same batch asks for the same work.
    fn finish_dispatched_element(
        &mut self,
        call: CodeModeCall,
        key: String,
        stdout: &str,
        repeated: &FxHashSet<String>,
        served: &mut FxHashMap<String, serde_json::Value>,
    ) -> serde_json::Value {
        if let Err(error) = self.charge_output(stdout.len()) {
            return error_element(&self.record_failure(call, error));
        }
        match parse_host_output(stdout) {
            Ok(value) => {
                self.memo.insert(key.clone(), stdout.to_string());
                if repeated.contains(&key) {
                    served.insert(key, value.clone());
                }
                self.record_success(call);
                ok_element(value)
            }
            Err(error) => error_element(&self.record_failure(call, error)),
        }
    }
}

/// Parse fallow JSON that a host call already read. Stdout that is not JSON
/// fails only its own element, the way a single call's `JSON.parse` fails only
/// that call.
fn parse_host_output(stdout: &str) -> Result<serde_json::Value, String> {
    serde_json::from_str(stdout)
        .map_err(|err| format!("fallow host call returned invalid JSON: {err}"))
}

/// Everything a batch needs to know before it runs: the per-element plan, the
/// dispatches it has to pay for, and the keys more than one element wants.
struct BatchPlan {
    elements: Vec<BatchElementPlan>,
    pending: Vec<PendingHostCall>,
    repeated: FxHashSet<String>,
}

/// What one `fallow.all` element needs before its result can be reported.
enum BatchElementPlan {
    /// Refused before dispatch: unknown tool name or malformed params.
    Rejected { tool: String, error: String },
    /// Already in the snippet's memo when the batch was planned.
    Cached { tool: CodeModeTool, output: String },
    /// The same `(tool, params)` as an earlier element of this batch, so it
    /// shares that element's single dispatch.
    Repeated { tool: CodeModeTool, key: String },
    /// The element that actually spends the host-call slot for its key.
    Dispatch { tool: CodeModeTool, key: String },
}

/// One host call a batch still has to run.
struct PendingHostCall {
    key: String,
    tool: CodeModeTool,
    params: serde_json::Value,
}

/// The result of one batch element, with the time it really took rather than
/// the time the assembly loop reached it. `refused` marks an element the batch
/// turned away before dispatch, which spends no `max_host_calls` slot and is
/// therefore charged to the rejection budget instead.
struct BatchOutcome {
    duration_ms: u128,
    result: Result<String, String>,
    refused: bool,
}

fn ok_element(value: serde_json::Value) -> serde_json::Value {
    let mut element = serde_json::Map::new();
    element.insert("ok".to_string(), serde_json::Value::Bool(true));
    element.insert("value".to_string(), value);
    serde_json::Value::Object(element)
}

fn error_element(error: &str) -> serde_json::Value {
    json!({ "ok": false, "error": error })
}

/// The message for an element whose dispatch did not produce output. A worker
/// that never reported at all is a bug rather than an analysis failure, so it
/// gets its own message instead of a misleading empty result.
fn failed_outcome_message(outcome: Option<&BatchOutcome>) -> String {
    match outcome {
        Some(BatchOutcome {
            result: Err(error), ..
        }) => error.clone(),
        _ => "code mode batch host call did not complete".to_string(),
    }
}

/// Run one resolved host call on its backing. Single calls and every
/// `fallow.all` element go through this function, so a batch cannot bypass
/// the deadline, the output cap, or the typed-params validation a single call
/// obeys.
fn dispatch_host_call(
    binary: &str,
    tool: CodeModeTool,
    params: serde_json::Value,
    deadline: Instant,
    max_output_bytes: usize,
) -> Result<String, String> {
    if let Some(value) = run_api_tool_with_deadline(tool, params.clone(), deadline)? {
        return Ok(serde_json::to_string(&value).unwrap_or_else(|_| "{}".to_string()));
    }
    let args = build_tool_args(tool, params)?;
    run_fallow_sync(binary, "code_execute", &args, deadline, max_output_bytes)
}

fn timed_dispatch(
    binary: &str,
    tool: CodeModeTool,
    params: serde_json::Value,
    deadline: Instant,
    max_output_bytes: usize,
) -> BatchOutcome {
    let started = Instant::now();
    let result = dispatch_host_call(binary, tool, params, deadline, max_output_bytes);
    BatchOutcome {
        duration_ms: started.elapsed().as_millis(),
        result,
        refused: false,
    }
}

/// Run a batch's dispatches, overlapping the subprocess-backed elements while
/// the in-process ones run one at a time on this thread. Each element reads at
/// most `element_output_bytes`, the shared output budget's per-dispatch share.
///
/// The split is not an optimization detail, it is what keeps the fan-out from
/// stacking uncancellable work: `fallow-api` has no cancellation, so two
/// in-process analyses running at once would be exactly the pile-up the
/// abandoned-call accounting exists to prevent. Subprocess-backed calls are
/// both the expensive ones and the only ones a deadline can actually kill, so
/// they are what the workers overlap. Every worker reaches `fallow-api`'s
/// decline path immediately, so no worker ever starts in-process work.
fn run_batch_dispatches(
    binary: &str,
    deadline: Instant,
    element_output_bytes: usize,
    pending: Vec<PendingHostCall>,
) -> FxHashMap<String, BatchOutcome> {
    let (spawned, in_process): (Vec<PendingHostCall>, Vec<PendingHostCall>) = pending
        .into_iter()
        .partition(|call| call.tool.backing() == CodeModeBacking::Subprocess);

    let mut outcomes = FxHashMap::default();
    let cursor = AtomicUsize::new(0);
    let (tx, rx) = mpsc::channel::<(String, BatchOutcome)>();

    thread::scope(|scope| {
        for _ in 0..MAX_BATCH_CONCURRENCY.min(spawned.len()) {
            let tx = tx.clone();
            let cursor = &cursor;
            let spawned = &spawned;
            scope.spawn(move || {
                while let Some(call) = spawned.get(cursor.fetch_add(1, Ordering::SeqCst)) {
                    let outcome = timed_dispatch(
                        binary,
                        call.tool,
                        call.params.clone(),
                        deadline,
                        element_output_bytes,
                    );
                    if tx.send((call.key.clone(), outcome)).is_err() {
                        break;
                    }
                }
            });
        }
        drop(tx);

        for call in in_process {
            let outcome = timed_dispatch(
                binary,
                call.tool,
                call.params,
                deadline,
                element_output_bytes,
            );
            outcomes.insert(call.key, outcome);
        }
        for (key, outcome) in rx {
            outcomes.insert(key, outcome);
        }
    });

    outcomes
}

/// Cache key for one host call: the wire tool name plus its merged params in
/// canonical form, so `{ a: 1, b: 2 }` and `{ b: 2, a: 1 }` are one entry.
fn memo_key(tool: CodeModeTool, params: &serde_json::Value) -> String {
    let mut key = tool.name().to_string();
    key.push('\u{1}');
    write_canonical_json(params, &mut key);
    key
}

/// Serialize `value` with object keys in sorted order. This crate builds
/// `serde_json` with `preserve_order`, so its maps keep the snippet's own key
/// order: the canonical ordering has to be imposed here rather than inherited
/// from the map type. Nesting depth is already bounded by `serde_json`'s
/// recursion limit, which rejects deeper params in `merge_default_root`.
fn write_canonical_json(value: &serde_json::Value, out: &mut String) {
    match value {
        serde_json::Value::Object(fields) => {
            let sorted: BTreeMap<&str, &serde_json::Value> = fields
                .iter()
                .map(|(key, value)| (key.as_str(), value))
                .collect();
            out.push('{');
            for (index, (key, value)) in sorted.into_iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                out.push_str(&serde_json::Value::String(key.to_string()).to_string());
                out.push(':');
                write_canonical_json(value, out);
            }
            out.push('}');
        }
        serde_json::Value::Array(items) => {
            out.push('[');
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                write_canonical_json(item, out);
            }
            out.push(']');
        }
        scalar => out.push_str(&scalar.to_string()),
    }
}

fn run_api_tool_with_deadline(
    tool: CodeModeTool,
    params: serde_json::Value,
    deadline: Instant,
) -> Result<Option<serde_json::Value>, String> {
    run_api_tool_with_deadline_and_runner(
        tool,
        params,
        deadline,
        run_api_tool,
        &ABANDONED_API_HOST_CALLS,
    )
}

/// Run an in-process host call under the sandbox deadline, or decline it so the
/// caller falls back to the killable subprocess.
///
/// `fallow-api` cannot be cancelled, so a timed-out call returns while its
/// analysis keeps running. `abandoned` bounds how much of that orphaned work a
/// long-lived server can stack up: once the bound is reached, further host
/// calls take the subprocess path until the abandoned analyses drain.
fn run_api_tool_with_deadline_and_runner<F>(
    tool: CodeModeTool,
    params: serde_json::Value,
    deadline: Instant,
    runner: F,
    abandoned: &'static AbandonedHostCalls,
) -> Result<Option<serde_json::Value>, String>
where
    F: FnOnce(CodeModeTool, serde_json::Value) -> Result<Option<serde_json::Value>, String>
        + Send
        + 'static,
{
    if tool.backing() == CodeModeBacking::Subprocess {
        return Ok(None);
    }
    let Some(remaining) = deadline.checked_duration_since(Instant::now()) else {
        return Err("code mode execution timed out".to_string());
    };
    if abandoned.saturated() {
        return Ok(None);
    }
    let state = Arc::new(AtomicU8::new(HOST_CALL_RUNNING));
    let completion = HostCallCompletion {
        state: Arc::clone(&state),
        abandoned,
    };
    let (tx, rx) = mpsc::channel();
    thread::Builder::new()
        .name("fallow-code-mode-api".to_string())
        .spawn(move || {
            let result = runner(tool, params);
            drop(completion);
            let _ = tx.send(result);
        })
        .map_err(|err| format!("failed to start code mode API host call: {err}"))?;

    match rx.recv_timeout(remaining) {
        Ok(result) => result,
        Err(mpsc::RecvTimeoutError::Timeout) => {
            abandoned.record(&state);
            Err("code mode execution timed out while running fallow".to_string())
        }
        Err(mpsc::RecvTimeoutError::Disconnected) => {
            Err("code mode API host call failed".to_string())
        }
    }
}

/// In-process host calls that outlived their deadline and are still running.
static ABANDONED_API_HOST_CALLS: AbandonedHostCalls = AbandonedHostCalls::new();

const HOST_CALL_RUNNING: u8 = 0;
const HOST_CALL_FINISHED: u8 = 1;
const HOST_CALL_ABANDONED: u8 = 2;

/// How many abandoned in-process analyses Code Mode carries before it stops
/// starting new ones. One is the whole point: the next host call takes the
/// killable subprocess instead of stacking a second uncancellable analysis on
/// top of work nobody can stop.
const MAX_ABANDONED_API_HOST_CALLS: usize = 1;

/// Running total of in-process host calls nobody is waiting for any more.
struct AbandonedHostCalls(AtomicUsize);

impl AbandonedHostCalls {
    const fn new() -> Self {
        Self(AtomicUsize::new(0))
    }

    fn saturated(&self) -> bool {
        self.0.load(Ordering::SeqCst) >= MAX_ABANDONED_API_HOST_CALLS
    }

    /// Claim `state`'s call as abandoned. The swap decides the race with the
    /// worker: whoever gets there first owns the accounting, so a call that
    /// finished just after the deadline is never counted.
    fn record(&self, state: &AtomicU8) {
        if state.swap(HOST_CALL_ABANDONED, Ordering::SeqCst) == HOST_CALL_RUNNING {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }
}

/// Worker-side half of the accounting. Dropping it publishes the call as
/// finished and releases the slot when the deadline already gave up on it, so a
/// panicking host call cannot strand the count.
struct HostCallCompletion {
    state: Arc<AtomicU8>,
    abandoned: &'static AbandonedHostCalls,
}

impl Drop for HostCallCompletion {
    fn drop(&mut self) {
        if self.state.swap(HOST_CALL_FINISHED, Ordering::SeqCst) == HOST_CALL_ABANDONED {
            self.abandoned.0.fetch_sub(1, Ordering::SeqCst);
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
    /// Served from the snippet's memo instead of a fresh analysis. Emitted
    /// only when it is true, so a response without caching keeps the exact
    /// shape existing consumers already parse.
    #[serde(skip_serializing_if = "is_false")]
    cache_hit: bool,
}

impl CodeModeCall {
    fn new(tool: &str) -> Self {
        Self {
            tool: tool.to_string(),
            duration_ms: 0,
            output_bytes: 0,
            ok: false,
            error_kind: None,
            cache_hit: false,
        }
    }
}

#[expect(
    clippy::trivially_copy_pass_by_ref,
    reason = "serde requires the skip_serializing_if predicate to take a reference"
)]
fn is_false(value: &bool) -> bool {
    !*value
}

/// Whether a failed host call was refused before any analysis ran. Such a
/// call spends no time and produces no output, so a snippet that mistypes a
/// tool name or passes malformed params keeps its full analysis budget.
fn rejected_before_dispatch(error_kind: &str) -> bool {
    matches!(error_kind, "unsupported_tool" | "invalid_params")
}

fn classify_host_error(message: &str) -> &'static str {
    if message.contains("does not expose fix tools")
        || message.contains("unsupported code mode fallow tool")
        || message.contains("similar-code is not exposed through Code Mode")
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
    if serde_json::from_str::<serde_json::Value>(message).is_ok_and(|error| {
        error.get("error").and_then(serde_json::Value::as_bool) == Some(true)
            && error.get("code").is_some_and(serde_json::Value::is_string)
    }) {
        return "programmatic";
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
    fn similar_code_routes_to_standalone_mcp_tools() {
        for tool in ["find_similar_code", "inspect_similar_code"] {
            let error =
                CodeModeTool::from_name(tool).expect_err("Code Mode must reject similar-code");
            assert!(error.contains("standalone MCP find_similar_code or inspect_similar_code"));
            assert!(error.contains("dedicated 15-minute timeout"));
        }
        assert!(
            !CODE_MODE_ALIASES
                .iter()
                .any(|(alias, _)| { matches!(*alias, "findSimilarCode" | "inspectSimilarCode") })
        );
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
    fn api_backed_combined_preserves_structured_programmatic_errors() {
        let temp = tempfile::tempdir().expect("tempdir");
        fs::write(
            temp.path().join("package.json"),
            r#"{"name":"code-mode-combined-error-test","type":"module"}"#,
        )
        .expect("package json");

        let output = execute_code_mode(
            "/definitely/not/fallow".to_string(),
            CodeExecuteParams {
                code: "return fallow.combined({ coverage: 'missing-coverage.json' });".to_string(),
                root: Some(temp.path().display().to_string()),
                timeout_ms: Some(5_000),
                max_output_bytes: Some(200_000),
            },
        )
        .expect_err("missing coverage should stay a structured API error");

        let envelope: serde_json::Value =
            serde_json::from_str(&output).expect("code mode error envelope");
        let error: serde_json::Value = serde_json::from_str(
            envelope["error"]
                .as_str()
                .expect("programmatic error should remain encoded as JSON"),
        )
        .expect("structured programmatic error");
        assert_eq!(error["error"], true);
        assert_eq!(error["exit_code"], 2);
        assert_eq!(error["code"], "FALLOW_INVALID_COVERAGE_PATH");
        assert_eq!(error["context"], "health.coverage");
        assert_eq!(envelope["calls"][0]["tool"], "combined");
        assert_eq!(envelope["calls"][0]["error_kind"], "programmatic");
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
        static ABANDONED: AbandonedHostCalls = AbandonedHostCalls::new();

        let result = run_api_tool_with_deadline_and_runner(
            CodeModeTool::ProjectInfo,
            serde_json::json!({}),
            Instant::now() + Duration::from_millis(1),
            |_tool, _params| {
                std::thread::sleep(Duration::from_millis(50));
                Ok(Some(serde_json::json!({"ok": true})))
            },
            &ABANDONED,
        );

        let err = result.expect_err("slow API host call must hit the external timeout");
        assert_eq!(err, "code mode execution timed out while running fallow");
    }

    /// A timed-out in-process call keeps running, so the next host call must
    /// not stack a second uncancellable analysis on top of it.
    #[test]
    fn abandoned_in_process_work_pushes_later_host_calls_to_the_subprocess() {
        static ABANDONED: AbandonedHostCalls = AbandonedHostCalls::new();

        let (release_tx, release_rx) = mpsc::channel::<()>();
        let timed_out = run_api_tool_with_deadline_and_runner(
            CodeModeTool::ProjectInfo,
            serde_json::json!({}),
            Instant::now() + Duration::from_millis(1),
            move |_tool, _params| {
                let _ = release_rx.recv();
                Ok(Some(serde_json::json!({"ok": true})))
            },
            &ABANDONED,
        );
        assert_eq!(
            timed_out.expect_err("the slow host call must hit the deadline"),
            "code mode execution timed out while running fallow"
        );
        assert!(ABANDONED.saturated(), "the abandoned call must be counted");

        let declined = run_api_tool_with_deadline_and_runner(
            CodeModeTool::ProjectInfo,
            serde_json::json!({}),
            Instant::now() + Duration::from_secs(30),
            |_tool, _params| panic!("a saturated host must not start in-process work"),
            &ABANDONED,
        );
        assert_eq!(
            declined.expect("a saturated host falls back instead of failing"),
            None
        );

        drop(release_tx);
        let drained = Instant::now() + Duration::from_secs(10);
        while ABANDONED.saturated() && Instant::now() < drained {
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(
            !ABANDONED.saturated(),
            "the finished worker must release its slot"
        );
    }

    /// The accounting only fires for calls that really outlived the deadline;
    /// a host call whose result arrives in time leaves the count untouched.
    #[test]
    fn completed_in_process_calls_are_never_counted_as_abandoned() {
        static ABANDONED: AbandonedHostCalls = AbandonedHostCalls::new();

        let result = run_api_tool_with_deadline_and_runner(
            CodeModeTool::ProjectInfo,
            serde_json::json!({}),
            Instant::now() + Duration::from_secs(30),
            |_tool, _params| Ok(Some(serde_json::json!({"ok": true}))),
            &ABANDONED,
        );

        assert_eq!(
            result.expect("host call"),
            Some(serde_json::json!({"ok": true}))
        );
        assert!(!ABANDONED.saturated());
    }

    // ---- CodeModeTool::from_name round-trip --------------------------------

    #[test]
    fn all_valid_tool_names_parse_successfully() {
        for (alias, name) in fallow_types::mcp_manifest::code_mode_allowlist() {
            assert!(
                CodeModeTool::from_name(name).is_ok(),
                "manifest exposes '{alias}' but '{name}' does not parse"
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
    fn code_mode_returns_standalone_guidance_for_similar_code_dispatch() {
        let output = execute_code_mode(
            "fallow".to_string(),
            CodeExecuteParams {
                code: "return fallow.run('find_similar_code', {});".to_string(),
                root: None,
                timeout_ms: Some(5_000),
                max_output_bytes: Some(10_000),
            },
        )
        .expect_err("Code Mode must reject similar-code dispatch");

        let json: serde_json::Value = serde_json::from_str(&output).expect("code mode JSON");
        assert_eq!(json["ok"], false);
        assert!(
            json["error"]
                .as_str()
                .is_some_and(|error| error.contains("standalone MCP find_similar_code"))
        );
        assert_eq!(json["calls"][0]["tool"], "find_similar_code");
        assert_eq!(json["calls"][0]["error_kind"], "unsupported_tool");
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
        for (alias, name) in fallow_types::mcp_manifest::code_mode_allowlist() {
            let tool = CodeModeTool::from_name(name)
                .unwrap_or_else(|err| panic!("alias {alias} maps to unknown tool {name}: {err}"));
            assert_eq!(tool.name(), name, "name() mismatch for input '{name}'");
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
    fn classify_similar_code_guidance_as_unsupported_tool() {
        assert_eq!(
            classify_host_error(
                "similar-code is not exposed through Code Mode's 30-second window; use the standalone MCP tools"
            ),
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
        assert_eq!(
            limits["max_rejected_host_calls"],
            MAX_REJECTED_HOST_CALLS as u64
        );
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
        assert_eq!(
            json["limits"]["max_rejected_host_calls"],
            MAX_REJECTED_HOST_CALLS as u64
        );
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

    // ---- result cap ---------------------------------------------------------

    fn run_snippet(code: &str, max_output_bytes: usize) -> (bool, serde_json::Value, usize) {
        run_snippet_with_binary("fallow", code, max_output_bytes)
    }

    fn run_snippet_with_binary(
        binary: &str,
        code: &str,
        max_output_bytes: usize,
    ) -> (bool, serde_json::Value, usize) {
        let (ok, output) = match execute_code_mode(
            binary.to_string(),
            CodeExecuteParams {
                code: code.to_string(),
                root: None,
                timeout_ms: Some(5_000),
                max_output_bytes: Some(max_output_bytes),
            },
        ) {
            Ok(output) => (true, output),
            Err(output) => (false, output),
        };
        let bytes = output.len();
        let json = serde_json::from_str(&output)
            .unwrap_or_else(|err| panic!("code mode response must be JSON: {err}\n{output}"));
        (ok, json, bytes)
    }

    /// The snippet result is the one part of the response that enters the
    /// calling agent's context, so `max_output_bytes` has to bound it.
    #[test]
    fn oversized_snippet_result_is_rejected_within_the_output_cap() {
        let (ok, json, bytes) = run_snippet(r#"return "x".repeat(3_000_000);"#, 1_000);

        assert!(!ok, "an oversized result must not be reported as success");
        assert_eq!(json["ok"], false);
        assert_eq!(json["truncated"], true);
        assert_eq!(json["result_bytes"], 3_000_002_u64);
        assert!(json["result"].is_null(), "the value must not be returned");
        assert!(
            json["error"]
                .as_str()
                .is_some_and(|error| error.contains("result exceeded 1000 bytes")),
            "error was: {}",
            json["error"]
        );
        assert!(
            json["result_preview"].as_str().map(str::len) == Some(RESULT_PREVIEW_BYTES),
            "preview was: {}",
            json["result_preview"]
        );
        assert!(
            bytes < 2_000,
            "the whole response must stay bounded, got {bytes} bytes"
        );
        assert_eq!(json["schema_version"], "mcp-code-execute/v1");
    }

    /// A result that fits is returned unchanged, so the cap does not turn into
    /// a silent ceiling below its own value.
    #[test]
    fn snippet_result_at_the_output_cap_is_returned() {
        let (ok, json, _) = run_snippet(r#"return "x".repeat(98);"#, 100);

        assert!(ok, "a result at the cap must succeed: {json}");
        assert_eq!(json["result"], "x".repeat(98));
        assert!(json.get("truncated").is_none());
    }

    /// A thrown message reaches the agent's context the same way a result
    /// does, so the envelope's `error` string is clamped as well.
    #[test]
    fn oversized_thrown_errors_are_clamped_in_the_envelope() {
        let (ok, json, bytes) = run_snippet(r#"throw new Error("x".repeat(3_000_000));"#, 1_000);

        assert!(!ok);
        assert_eq!(json["truncated"], true);
        assert_eq!(json["error_bytes"], 3_000_000_u64);
        assert_eq!(
            json["error"].as_str().map(str::len),
            Some(MAX_ERROR_BYTES),
            "the error must be clamped to the error floor"
        );
        assert!(
            bytes < 6_000,
            "a thrown message must not inflate the response, got {bytes} bytes"
        );
    }

    #[test]
    fn ordinary_errors_keep_their_envelope_shape() {
        let (ok, json, _) = run_snippet(r#"throw new Error("small");"#, 1_000);

        assert!(!ok);
        assert_eq!(json["error"], "small");
        assert!(json.get("truncated").is_none());
        assert!(json.get("error_bytes").is_none());
    }

    #[test]
    fn result_preview_never_splits_a_character() {
        let (_, json, _) = run_snippet(r#"return "é".repeat(3_000);"#, 11);

        assert_eq!(json["truncated"], true);
        assert_eq!(
            json["result_preview"].as_str(),
            Some(r#""ééééé"#),
            "preview must stop on a character boundary"
        );
    }

    // ---- sandbox hardening --------------------------------------------------

    /// Undefining the `Function` global hides the binding, not the intrinsic.
    /// Every prototype route to a function constructor must be closed, or the
    /// published "no Function" contract is false.
    #[test]
    fn function_constructors_are_unreachable_through_prototypes() {
        for route in [
            "(function () {}).constructor",
            "({}).constructor.constructor",
            "Object.getPrototypeOf(function () {}).constructor",
            "(async function () {}).constructor",
            "(function* () {}).constructor",
            "(async function* () {}).constructor",
            "[].map.constructor",
            "(function () {}).bind(null).constructor",
        ] {
            let (_, json, _) = run_snippet(&format!("return typeof {route};"), 10_000);
            assert_eq!(
                json["result"], "undefined",
                "{route} still resolves to a function constructor"
            );

            let (_, json, _) = run_snippet(
                &format!(
                    "try {{ return {route}('return 1+1')(); }} catch (error) {{ return 'threw'; }}"
                ),
                10_000,
            );
            assert_eq!(json["result"], "threw", "{route} still compiles code");
        }
    }

    #[test]
    fn neutralized_function_constructor_cannot_be_restored() {
        let (_, json, _) = run_snippet(
            "const proto = Object.getPrototypeOf(function () {});
             try {
                 Object.defineProperty(proto, 'constructor', { value: 1 });
                 return 'redefined';
             } catch (error) {
                 return 'threw';
             }",
            10_000,
        );

        assert_eq!(json["result"], "threw");
    }

    /// Hardening must not cost ordinary snippets anything: the constructors
    /// Code Mode removes are the dynamic-compilation ones, not the ones every
    /// literal, class, and built-in relies on.
    #[test]
    fn ordinary_javascript_still_works_after_hardening() {
        let (ok, json, _) = run_snippet(
            r"
            class Box {
                constructor(value) { this.value = value; }
                get doubled() { return this.value * 2; }
            }
            const add = (a, b) => a + b;
            const source = { a: 1, b: 2 };
            const { a, ...rest } = source;
            const numbers = [1, 2, 3].map((n) => add(n, 1)).filter((n) => n > 2);
            const generate = function* () { yield 1; yield 2; };
            return {
                boxed: new Box(21).doubled,
                reduced: numbers.reduce(add, 0),
                spread: [...numbers, 4],
                entries: Object.entries(source),
                rest,
                template: `a=${a}`,
                json: JSON.stringify(source),
                generated: [...generate()],
                literalConstructor: source.constructor === Object,
            };
            ",
            10_000,
        );

        assert!(ok, "ordinary snippets must keep working: {json}");
        assert_eq!(json["result"]["boxed"], 42);
        assert_eq!(json["result"]["reduced"], 7);
        assert_eq!(json["result"]["spread"], serde_json::json!([3, 4, 4]));
        assert_eq!(
            json["result"]["entries"],
            serde_json::json!([["a", 1], ["b", 2]])
        );
        assert_eq!(json["result"]["rest"], serde_json::json!({ "b": 2 }));
        assert_eq!(json["result"]["template"], "a=1");
        assert_eq!(json["result"]["json"], r#"{"a":1,"b":2}"#);
        assert_eq!(json["result"]["generated"], serde_json::json!([1, 2]));
        assert_eq!(json["result"]["literalConstructor"], true);
    }

    /// `harden_globals` is a denylist over a fixed name list, so a new global
    /// from a runtime upgrade or a feature flag would widen the sandbox
    /// silently. Enumerating the realm turns that into a build failure and a
    /// deliberate decision about the new binding.
    #[test]
    fn sandbox_globals_stay_within_the_reviewed_allowlist() {
        const EXPECTED: &[&str] = &[
            "AggregateError",
            "Array",
            "ArrayBuffer",
            "AsyncDisposableStack",
            "AsyncFunction",
            "Atomics",
            "BigInt",
            "BigInt64Array",
            "BigUint64Array",
            "Boolean",
            "Bun",
            "DOMException",
            "DataView",
            "Date",
            "Deno",
            "DisposableStack",
            "Error",
            "EvalError",
            "FinalizationRegistry",
            "Float16Array",
            "Float32Array",
            "Float64Array",
            "Function",
            "Infinity",
            "Int16Array",
            "Int32Array",
            "Int8Array",
            "InternalError",
            "Iterator",
            "JSON",
            "Map",
            "Math",
            "NaN",
            "Number",
            "Object",
            "Promise",
            "Proxy",
            "RangeError",
            "ReferenceError",
            "Reflect",
            "RegExp",
            "Set",
            "SharedArrayBuffer",
            "String",
            "SuppressedError",
            "Symbol",
            "SyntaxError",
            "TypeError",
            "URIError",
            "Uint16Array",
            "Uint32Array",
            "Uint8Array",
            "Uint8ClampedArray",
            "WeakMap",
            "WeakRef",
            "WeakSet",
            "WebAssembly",
            "XMLHttpRequest",
            "atob",
            "btoa",
            "decodeURI",
            "decodeURIComponent",
            "encodeURI",
            "encodeURIComponent",
            "escape",
            "eval",
            "fallow",
            "fetch",
            "globalThis",
            "importScripts",
            "isFinite",
            "isNaN",
            "parseFloat",
            "parseInt",
            "performance",
            "process",
            "queueMicrotask",
            "require",
            "root",
            "undefined",
            "unescape",
        ];

        let (ok, json, _) = run_snippet(
            "return Object.getOwnPropertyNames(globalThis).sort();",
            100_000,
        );
        assert!(ok, "global enumeration must succeed: {json}");
        let names = json["result"].as_array().expect("global names");
        assert!(!names.is_empty());
        for name in names {
            let name = name.as_str().expect("global name");
            assert!(
                EXPECTED.contains(&name),
                "the sandbox gained the global '{name}'; decide whether to deny it in \
                 harden_globals before adding it to this list"
            );
        }
    }

    /// Every denied global keeps a value of `undefined`, including the ones a
    /// snippet cannot see any other way.
    #[test]
    fn denied_globals_stay_undefined() {
        let (_, json, _) = run_snippet(
            "return ['eval', 'Function', 'AsyncFunction', 'WebAssembly', 'fetch',
                     'XMLHttpRequest', 'importScripts', 'process', 'require', 'Deno', 'Bun']
                 .map((name) => typeof globalThis[name])
                 .every((kind) => kind === 'undefined');",
            10_000,
        );

        assert_eq!(json["result"], true);
    }

    // ---- host call name clamping and rejection budget ----------------------

    #[test]
    fn oversized_tool_names_are_clamped_in_the_response() {
        let (ok, json, bytes) = run_snippet(r#"return fallow.run("z".repeat(5_000), {});"#, 10_000);

        assert!(!ok);
        let recorded = json["calls"][0]["tool"].as_str().expect("recorded tool");
        assert_eq!(recorded.len(), MAX_TOOL_NAME_BYTES);
        assert!(
            bytes < 1_000,
            "an unvalidated tool name must not inflate the response, got {bytes} bytes"
        );
        assert_eq!(json["calls"][0]["error_kind"], "unsupported_tool");
    }

    /// A mistyped tool name runs no analysis and reads no output, so it must
    /// not spend the analysis budget the snippet still needs.
    #[test]
    fn rejected_host_calls_keep_the_analysis_budget_intact() {
        let (ok, json, _) = run_snippet(
            r#"
            let rejected = 0;
            for (let index = 0; index < 8; index += 1) {
                try {
                    fallow.run("nope_" + index, {});
                } catch (error) {
                    rejected += 1;
                }
            }
            return { rejected, kind: fallow.explain({ issue_type: "unused-export" }).kind };
            "#,
            200_000,
        );

        assert!(ok, "the real host call must still run: {json}");
        assert_eq!(json["result"]["rejected"], 8);
        assert_eq!(json["result"]["kind"], "explain");
        assert_eq!(json["calls"].as_array().map(Vec::len), Some(9));
        assert_eq!(json["calls"][8]["tool"], "fallow_explain");
        assert_eq!(json["calls"][8]["ok"], true);
    }

    /// Rejections are free but not unlimited: `calls[]` has to stay bounded
    /// even for a snippet that loops over bad names.
    #[test]
    fn rejected_host_calls_have_their_own_bound() {
        let (ok, json, _) = run_snippet(
            r#"
            const errors = [];
            for (let index = 0; index < 12; index += 1) {
                try {
                    fallow.run("nope_" + index, {});
                } catch (error) {
                    errors.push(String(error));
                }
            }
            return errors[errors.length - 1];
            "#,
            200_000,
        );

        assert!(ok, "the snippet itself still returns: {json}");
        assert!(
            json["result"]
                .as_str()
                .is_some_and(|error| error.contains("rejected host call limit exceeded (8)")),
            "result was: {}",
            json["result"]
        );
        assert_eq!(
            json["calls"].as_array().map(Vec::len),
            Some(MAX_REJECTED_HOST_CALLS)
        );
    }

    #[test]
    fn malformed_params_are_rejected_without_spending_the_budget() {
        let (ok, json, _) = run_snippet(
            r#"
            let rejected = 0;
            for (let index = 0; index < 8; index += 1) {
                try {
                    fallow.explain([]);
                } catch (error) {
                    rejected += 1;
                }
            }
            return { rejected, kind: fallow.explain({ issue_type: "unused-export" }).kind };
            "#,
            200_000,
        );

        assert!(ok, "the real host call must still run: {json}");
        assert_eq!(json["result"]["rejected"], 8);
        assert_eq!(json["calls"][0]["error_kind"], "invalid_params");
        assert_eq!(json["calls"][8]["ok"], true);
    }

    /// Distinct host calls each spend a slot. The params vary so the memo
    /// cannot serve them: the budget is about analyses that actually run.
    #[test]
    fn dispatched_host_calls_still_consume_the_call_budget() {
        let (ok, json, _) = run_snippet(
            r#"
            const errors = [];
            for (let index = 0; index < 9; index += 1) {
                try {
                    fallow.explain({ issue_type: "unused-export", nonce: index });
                } catch (error) {
                    errors.push(String(error));
                }
            }
            return errors[0];
            "#,
            2_000_000,
        );

        assert!(ok, "the snippet itself still returns: {json}");
        assert!(
            json["result"]
                .as_str()
                .is_some_and(|error| error.contains("host call limit exceeded (8)")),
            "result was: {}",
            json["result"]
        );
        assert_eq!(json["calls"].as_array().map(Vec::len), Some(MAX_HOST_CALLS));
    }

    #[test]
    fn clamp_utf8_keeps_short_values_intact() {
        assert_eq!(clamp_utf8("analyze", MAX_TOOL_NAME_BYTES), "analyze");
        assert_eq!(clamp_utf8("", 0), "");
    }

    #[test]
    fn clamp_utf8_stops_on_a_character_boundary() {
        assert_eq!(clamp_utf8("éé", 3), "é");
        assert_eq!(clamp_utf8("éé", 1), "");
    }

    #[test]
    fn rejected_before_dispatch_covers_only_pre_dispatch_failures() {
        assert!(rejected_before_dispatch("unsupported_tool"));
        assert!(rejected_before_dispatch("invalid_params"));
        for spent in ["timeout", "output_limit", "programmatic", "subprocess"] {
            assert!(!rejected_before_dispatch(spent), "{spent} did real work");
        }
    }

    // ---- per-snippet host-call memo ----------------------------------------

    /// Repeating a host call re-ran the whole pipeline and burned a second
    /// slot, so composition lost to separate MCP round-trips exactly where it
    /// should have won. The output cap here is smaller than two payloads, so
    /// the test also fails if a memo hit charges its bytes twice.
    #[test]
    fn memo_hits_spend_neither_a_host_call_slot_nor_the_output_budget() {
        let (ok, json, _) = run_snippet(
            r#"
            const kinds = [];
            for (let index = 0; index < 12; index += 1) {
                kinds.push(fallow.explain({ issue_type: "unused-export" }).kind);
            }
            return { count: kinds.length, kind: kinds[11] };
            "#,
            2_000,
        );

        assert!(
            ok,
            "12 identical host calls must survive an 8-call budget: {json}"
        );
        assert_eq!(json["result"]["count"], 12);
        assert_eq!(json["result"]["kind"], "explain");
        let calls = json["calls"].as_array().expect("calls");
        assert_eq!(calls.len(), 12, "every call stays in the trace");
        assert!(
            calls[0].get("cache_hit").is_none(),
            "the first call is the one that actually ran: {}",
            calls[0]
        );
        for call in &calls[1..] {
            assert_eq!(
                call["cache_hit"], true,
                "call was not served from the memo: {call}"
            );
            assert_eq!(call["ok"], true);
        }
    }

    /// An exhausted output budget refuses later calls before dispatch, so they
    /// spend no `max_host_calls` slot. They have to be charged to the rejection
    /// budget instead, or a loop of them would grow `calls[]` without bound.
    #[test]
    fn output_budget_refusals_are_bounded_by_the_rejection_budget() {
        let (ok, json, _) = run_snippet(
            r#"
            let refused = 0;
            for (let index = 0; index < 40; index += 1) {
                try {
                    fallow.explain({ issue_type: "unused-export", nonce: index });
                } catch (error) {
                    refused += 1;
                }
            }
            return refused;
            "#,
            600,
        );

        assert!(ok, "the snippet itself still returns: {json}");
        assert_eq!(json["result"], 40);
        let calls = json["calls"].as_array().expect("calls");
        assert!(
            calls.len() <= MAX_HOST_CALLS + MAX_REJECTED_HOST_CALLS,
            "a loop of refusals grew the trace without bound: {}",
            calls.len()
        );
    }

    /// A memo hit is recorded in `calls[]` but runs nothing, so it must not
    /// shrink the analysis budget. Deriving the budget from `calls.len()` made
    /// eight repeats of one call exhaust `max_host_calls`, and the first
    /// distinct call after them was refused.
    #[test]
    fn memo_hits_leave_the_call_budget_for_distinct_calls() {
        let (ok, json, _) = run_snippet(
            r#"
            for (let index = 0; index < 8; index += 1) {
                fallow.explain({ issue_type: "unused-export" });
            }
            return fallow.explain({ issue_type: "unused-export", nonce: 1 }).kind;
            "#,
            2_000_000,
        );

        assert!(
            ok,
            "seven memo hits must leave the budget for a distinct call: {json}"
        );
        assert_eq!(json["result"], "explain");
        let calls = json["calls"].as_array().expect("calls");
        assert_eq!(calls.len(), 9);
        assert!(
            calls[8].get("cache_hit").is_none(),
            "the distinct call had to dispatch for itself: {}",
            calls[8]
        );
        assert_eq!(calls[8]["ok"], true);
    }

    /// The memo is keyed on the meaning of the params, not on the order the
    /// snippet happened to write them in.
    #[test]
    fn memo_keys_ignore_object_key_order() {
        let ordered = merge_default_root(r#"{"a":1,"b":{"c":2,"d":3}}"#, None).unwrap();
        let shuffled = merge_default_root(r#"{"b":{"d":3,"c":2},"a":1}"#, None).unwrap();
        assert_eq!(
            memo_key(CodeModeTool::FallowExplain, &ordered),
            memo_key(CodeModeTool::FallowExplain, &shuffled)
        );

        let different_value = merge_default_root(r#"{"a":2,"b":{"c":2,"d":3}}"#, None).unwrap();
        assert_ne!(
            memo_key(CodeModeTool::FallowExplain, &ordered),
            memo_key(CodeModeTool::FallowExplain, &different_value)
        );
        assert_ne!(
            memo_key(CodeModeTool::FallowExplain, &ordered),
            memo_key(CodeModeTool::Audit, &ordered),
            "the tool is part of the key"
        );

        let ascending = merge_default_root(r#"{"a":[1,2]}"#, None).unwrap();
        let descending = merge_default_root(r#"{"a":[2,1]}"#, None).unwrap();
        assert_ne!(
            memo_key(CodeModeTool::FallowExplain, &ascending),
            memo_key(CodeModeTool::FallowExplain, &descending),
            "array order is meaning, not formatting"
        );
    }

    #[test]
    fn reordered_params_hit_the_same_memo_entry_in_the_sandbox() {
        let (ok, json, _) = run_snippet(
            r#"
            const first = fallow.explain({ issue_type: "unused-export", detail: true });
            const second = fallow.explain({ detail: true, issue_type: "unused-export" });
            return { same: first.id === second.id };
            "#,
            200_000,
        );

        assert!(ok, "both calls must succeed: {json}");
        assert_eq!(json["result"]["same"], true);
        assert_eq!(json["calls"].as_array().map(Vec::len), Some(2));
        assert!(json["calls"][0].get("cache_hit").is_none());
        assert_eq!(json["calls"][1]["cache_hit"], true);
    }

    /// Different params are different work: the memo must not collapse them.
    #[test]
    fn different_params_are_separate_memo_entries() {
        let (ok, json, _) = run_snippet(
            r#"
            const first = fallow.explain({ issue_type: "unused-export" });
            const second = fallow.explain({ issue_type: "unused-file" });
            return { first: first.id, second: second.id };
            "#,
            200_000,
        );

        assert!(ok, "both calls must succeed: {json}");
        assert_eq!(json["result"]["first"], "fallow/unused-export");
        assert_eq!(json["result"]["second"], "fallow/unused-file");
        for index in 0..2 {
            assert!(
                json["calls"][index].get("cache_hit").is_none(),
                "call {index} was wrongly served from the memo"
            );
        }
    }

    // ---- fallow.all batching -----------------------------------------------

    #[test]
    fn batch_results_are_positionally_aligned_with_their_requests() {
        let (ok, json, _) = run_snippet(
            r#"
            return fallow.all([
                { tool: "fallow_explain", params: { issue_type: "unused-export" } },
                { tool: "definitely_not_a_tool" },
                { tool: "fallow_explain", params: { issue_type: "unused-file" } }
            ]);
            "#,
            200_000,
        );

        assert!(ok, "a failing element must not fail the batch: {json}");
        let elements = json["result"].as_array().expect("batch elements");
        assert_eq!(elements.len(), 3);
        assert_eq!(elements[0]["ok"], true);
        assert_eq!(elements[0]["value"]["id"], "fallow/unused-export");
        assert_eq!(elements[1]["ok"], false);
        assert!(
            elements[1]["error"]
                .as_str()
                .is_some_and(|error| error.contains("unsupported code mode fallow tool")),
            "error was: {}",
            elements[1]["error"]
        );
        assert!(elements[1].get("value").is_none());
        assert_eq!(elements[2]["ok"], true);
        assert_eq!(elements[2]["value"]["id"], "fallow/unused-file");

        assert_eq!(json["calls"].as_array().map(Vec::len), Some(3));
        assert_eq!(json["calls"][1]["error_kind"], "unsupported_tool");
    }

    /// A batch mixes both backings, so one element failing in its subprocess
    /// must still leave the in-process element's result usable.
    #[test]
    fn a_failing_batch_element_leaves_the_others_intact() {
        let (ok, json, _) = run_snippet_with_binary(
            "/definitely/not/fallow",
            r#"
            return fallow.all([
                { tool: "fallow_explain", params: { issue_type: "unused-export" } },
                { tool: "impact", params: {} }
            ]);
            "#,
            200_000,
        );

        assert!(ok, "the batch itself must succeed: {json}");
        let elements = json["result"].as_array().expect("batch elements");
        assert_eq!(elements[0]["ok"], true);
        assert_eq!(elements[0]["value"]["kind"], "explain");
        assert_eq!(elements[1]["ok"], false);
        assert!(
            elements[1]["error"]
                .as_str()
                .is_some_and(|error| error.contains("failed to execute fallow binary")),
            "error was: {}",
            elements[1]["error"]
        );
        assert_eq!(json["calls"][1]["tool"], "impact");
        assert_eq!(json["calls"][1]["error_kind"], "subprocess");
    }

    #[test]
    fn batches_larger_than_the_call_budget_are_refused() {
        let (ok, json, _) = run_snippet(
            r#"
            const requests = [];
            for (let index = 0; index < 9; index += 1) {
                requests.push({ tool: "fallow_explain", params: { issue_type: "unused-export", nonce: index } });
            }
            try {
                fallow.all(requests);
                return "no error";
            } catch (error) {
                return String(error);
            }
            "#,
            200_000,
        );

        assert!(ok, "the snippet itself still returns: {json}");
        assert!(
            json["result"]
                .as_str()
                .is_some_and(|error| error.contains("exceeds max_host_calls (8)")),
            "result was: {}",
            json["result"]
        );
        assert_eq!(
            json["calls"].as_array().map(Vec::len),
            Some(0),
            "a refused batch must run nothing"
        );
    }

    /// The batch is refused before any element runs, so a snippet cannot spend
    /// its last slots on a fan-out it could never finish.
    #[test]
    fn batches_over_the_remaining_budget_are_refused_up_front() {
        let (ok, json, _) = run_snippet(
            r#"
            for (let index = 0; index < 6; index += 1) {
                fallow.explain({ issue_type: "unused-export", nonce: index });
            }
            try {
                fallow.all([
                    { tool: "fallow_explain", params: { issue_type: "unused-export", nonce: 100 } },
                    { tool: "fallow_explain", params: { issue_type: "unused-export", nonce: 101 } },
                    { tool: "fallow_explain", params: { issue_type: "unused-export", nonce: 102 } }
                ]);
                return "no error";
            } catch (error) {
                return String(error);
            }
            "#,
            2_000_000,
        );

        assert!(ok, "the snippet itself still returns: {json}");
        assert!(
            json["result"].as_str().is_some_and(|error| {
                error.contains(
                    "needs 3 host calls but only 2 of the max_host_calls (8) budget remain",
                )
            }),
            "result was: {}",
            json["result"]
        );
        assert_eq!(
            json["calls"].as_array().map(Vec::len),
            Some(6),
            "the refused batch must not appear in the trace"
        );
    }

    /// Memo hits are free, so a batch of them is not measured against the
    /// budget even when nothing is left to dispatch.
    #[test]
    fn batches_of_memo_hits_do_not_need_budget() {
        let (ok, json, _) = run_snippet(
            r#"
            for (let index = 0; index < 8; index += 1) {
                fallow.explain({ issue_type: "unused-export", nonce: index });
            }
            const batch = fallow.all([
                { tool: "fallow_explain", params: { issue_type: "unused-export", nonce: 0 } },
                { tool: "fallow_explain", params: { nonce: 1, issue_type: "unused-export" } }
            ]);
            return { first: batch[0].ok, second: batch[1].ok };
            "#,
            2_000_000,
        );

        assert!(
            ok,
            "an all-cached batch must succeed on a spent budget: {json}"
        );
        assert_eq!(json["result"]["first"], true);
        assert_eq!(json["result"]["second"], true);
        assert_eq!(json["calls"][8]["cache_hit"], true);
        assert_eq!(json["calls"][9]["cache_hit"], true);
    }

    /// One dispatch serves every element that asks for the same work, inside
    /// the batch as well as across it.
    #[test]
    fn repeated_elements_inside_one_batch_share_a_single_dispatch() {
        let (ok, json, _) = run_snippet(
            r#"
            const batch = fallow.all([
                { tool: "fallow_explain", params: { issue_type: "unused-export", detail: true } },
                { tool: "fallow_explain", params: { detail: true, issue_type: "unused-export" } }
            ]);
            return { same: batch[0].value.id === batch[1].value.id, ok: batch[1].ok };
            "#,
            200_000,
        );

        assert!(ok, "the batch must succeed: {json}");
        assert_eq!(json["result"]["same"], true);
        assert_eq!(json["result"]["ok"], true);
        assert!(json["calls"][0].get("cache_hit").is_none());
        assert_eq!(json["calls"][1]["cache_hit"], true);
    }

    #[test]
    fn batch_elements_share_the_snippet_memo_with_single_calls() {
        let (ok, json, _) = run_snippet(
            r#"
            const single = fallow.explain({ issue_type: "unused-export" });
            const batch = fallow.all([
                { tool: "fallow_explain", params: { issue_type: "unused-export" } }
            ]);
            return { same: batch[0].value.id === single.id };
            "#,
            200_000,
        );

        assert!(ok, "the batch must reuse the earlier call: {json}");
        assert_eq!(json["result"]["same"], true);
        assert_eq!(json["calls"].as_array().map(Vec::len), Some(2));
        assert_eq!(json["calls"][1]["cache_hit"], true);
    }

    #[test]
    fn empty_batches_return_an_empty_array_and_cost_nothing() {
        let (ok, json, _) = run_snippet("return fallow.all([]);", 10_000);

        assert!(ok, "an empty batch must succeed: {json}");
        assert_eq!(json["result"], serde_json::json!([]));
        assert_eq!(json["calls"].as_array().map(Vec::len), Some(0));
    }

    #[test]
    fn non_array_batch_arguments_are_refused_with_guidance() {
        for argument in [
            "{ tool: 'fallow_explain' }",
            "'fallow_explain'",
            "undefined",
        ] {
            let (ok, json, _) = run_snippet(
                &format!(
                    "try {{ fallow.all({argument}); return 'no error'; }} \
                     catch (error) {{ return String(error); }}"
                ),
                10_000,
            );

            assert!(ok, "the snippet itself still returns: {json}");
            assert!(
                json["result"]
                    .as_str()
                    .is_some_and(|error| error.contains("fallow.all expects an array")),
                "argument {argument} gave: {}",
                json["result"]
            );
        }
    }

    #[test]
    fn malformed_batch_elements_are_refused_as_a_whole() {
        let (ok, json, _) = run_snippet(
            "try { fallow.all([{ params: {} }]); return 'no error'; } \
             catch (error) { return String(error); }",
            10_000,
        );

        assert!(ok, "the snippet itself still returns: {json}");
        assert!(
            json["result"]
                .as_str()
                .is_some_and(|error| error.contains("fallow.all expects an array")),
            "result was: {}",
            json["result"]
        );
    }

    #[test]
    fn batches_are_refused_after_the_deadline() {
        let mut state = CodeModeState::new(
            "fallow".to_string(),
            None,
            Instant::now()
                .checked_sub(Duration::from_millis(1))
                .expect("past instant"),
            1_000_000,
        );

        let err = state
            .run_batch(&[(
                "fallow_explain".to_string(),
                r#"{"issue_type":"unused-export"}"#.to_string(),
            )])
            .expect_err("an expired deadline must refuse the batch");

        assert_eq!(err, "code mode execution timed out");
        assert!(state.calls.is_empty(), "nothing ran, so nothing is traced");
    }

    /// Every element carries the batch's own deadline into its backing, so an
    /// expired deadline stops in-process work before it starts.
    #[test]
    fn batch_elements_inherit_the_shared_deadline() {
        let outcomes = run_batch_dispatches(
            "/definitely/not/fallow",
            Instant::now()
                .checked_sub(Duration::from_millis(1))
                .expect("past instant"),
            1_000_000,
            vec![
                PendingHostCall {
                    key: "first".to_string(),
                    tool: CodeModeTool::ProjectInfo,
                    params: serde_json::json!({}),
                },
                PendingHostCall {
                    key: "second".to_string(),
                    tool: CodeModeTool::ProjectInfo,
                    params: serde_json::json!({ "files": true }),
                },
            ],
        );

        assert_eq!(outcomes.len(), 2);
        for key in ["first", "second"] {
            let outcome = outcomes.get(key).expect("outcome");
            assert_eq!(
                outcome.result.as_ref().expect_err("expired deadline"),
                "code mode execution timed out"
            );
        }
    }

    /// A stand-in `fallow` that behaves as `run_fallow_sync` expects: it
    /// ignores the analysis arguments, so only its timing matters.
    #[cfg(unix)]
    fn stub_binary(dir: &std::path::Path, name: &str, script: &str) -> String {
        use std::os::unix::fs::PermissionsExt;

        let path = dir.join(name);
        fs::write(&path, script).expect("stub binary");
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).expect("executable");
        path.display().to_string()
    }

    #[cfg(unix)]
    fn subprocess_batch(keys: &[(&str, CodeModeTool)]) -> Vec<PendingHostCall> {
        keys.iter()
            .map(|(key, tool)| PendingHostCall {
                key: (*key).to_string(),
                tool: *tool,
                params: serde_json::json!({}),
            })
            .collect()
    }

    #[cfg(unix)]
    const SUBPROCESS_BATCH: &[(&str, CodeModeTool)] = &[
        ("impact", CodeModeTool::Impact),
        ("audit", CodeModeTool::Audit),
        ("health", CodeModeTool::CheckHealth),
        ("analyze", CodeModeTool::Analyze),
    ];

    /// Overlapping the subprocess-backed elements is the entire point of
    /// `fallow.all`. Each stub brackets its own run in a shared marker file, so
    /// the proof is that one element started before another finished rather
    /// than a wall-clock bound, which a loaded machine can miss for reasons
    /// that have nothing to do with the fan-out.
    #[cfg(unix)]
    #[test]
    fn subprocess_batch_elements_overlap() {
        let temp = tempfile::tempdir().expect("tempdir");
        let marker = temp.path().join("marker");
        let binary = stub_binary(
            temp.path(),
            "slow-fallow",
            &format!(
                "#!/bin/sh\nprintf 's' >> '{marker}'\nsleep 0.4\nprintf 'e' >> '{marker}'\n\
                 printf '{{}}'\n",
                marker = marker.display()
            ),
        );

        let outcomes = run_batch_dispatches(
            &binary,
            Instant::now() + Duration::from_secs(30),
            1_000_000,
            subprocess_batch(SUBPROCESS_BATCH),
        );

        assert_eq!(outcomes.len(), SUBPROCESS_BATCH.len());
        for (key, outcome) in &outcomes {
            assert_eq!(
                outcome.result.as_ref().expect("element result"),
                "{}",
                "element {key} did not run"
            );
        }
        let bracketed = fs::read_to_string(&marker).expect("marker file");
        let started_before_the_first_finished = bracketed.find('e').unwrap_or(bracketed.len());
        assert!(
            started_before_the_first_finished >= 2,
            "elements ran sequentially instead of overlapping: {bracketed}"
        );
    }

    /// `max_output_bytes` is a total cap, so a fan-out cannot spend the whole
    /// budget once per element. Each element here fits the full budget but not
    /// its share of it, so every one of them has to be refused.
    #[cfg(unix)]
    #[test]
    fn batch_dispatches_share_the_output_budget() {
        let temp = tempfile::tempdir().expect("tempdir");
        let binary = stub_binary(
            temp.path(),
            "chatty-fallow",
            "#!/bin/sh\nprintf '%01000d' 0\n",
        );

        let mut state = CodeModeState::new(
            binary,
            None,
            Instant::now() + Duration::from_secs(30),
            2_000,
        );
        let outcomes = state.dispatch_batch(subprocess_batch(SUBPROCESS_BATCH));

        assert_eq!(outcomes.len(), SUBPROCESS_BATCH.len());
        for (key, outcome) in &outcomes {
            assert_eq!(
                outcome
                    .result
                    .as_ref()
                    .expect_err("the shared budget must be enforced"),
                "code mode host output exceeded 500 bytes",
                "element {key} read past its share of the output budget"
            );
        }
        assert_eq!(
            state.remaining_host_calls(),
            MAX_HOST_CALLS - SUBPROCESS_BATCH.len(),
            "every dispatched element spends one slot"
        );
    }

    /// The deadline is shared, and it has to kill the children rather than
    /// only stop waiting for them.
    #[cfg(unix)]
    #[test]
    fn subprocess_batch_elements_die_at_the_shared_deadline() {
        let temp = tempfile::tempdir().expect("tempdir");
        let binary = stub_binary(temp.path(), "hanging-fallow", "#!/bin/sh\nsleep 30\n");

        let outcomes = run_batch_dispatches(
            &binary,
            Instant::now() + Duration::from_millis(400),
            1_000_000,
            subprocess_batch(SUBPROCESS_BATCH),
        );

        assert_eq!(outcomes.len(), SUBPROCESS_BATCH.len());
        for (key, outcome) in &outcomes {
            assert_eq!(
                outcome
                    .result
                    .as_ref()
                    .expect_err("the deadline must kill it"),
                "code mode execution timed out while running fallow",
                "element {key} outlived the shared deadline"
            );
        }
    }
}
