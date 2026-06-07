//! Issue #859: extract-layer capture of sink argument identifiers and
//! tainted-source bindings that feed the analyze-layer source-to-sink trace.

use crate::tests::{parse_ts, parse_tsx};

#[test]
fn sink_captures_arg_idents_for_bare_identifier() {
    // `eval(userInput)` -> the sink argument references `userInput`.
    let info = parse_ts("const userInput = getInput();\neval(userInput);");
    let sink = info
        .security_sinks
        .iter()
        .find(|s| s.callee_path == "eval")
        .expect("eval sink captured");
    assert!(sink.arg_idents.iter().any(|n| n == "userInput"));
}

#[test]
fn sink_captures_arg_idents_through_member_and_concat() {
    // `el.innerHTML = "<b>" + data.value` -> the concatenation references `data`
    // (the member-access root), not the static property name.
    let info = parse_ts("el.innerHTML = \"<b>\" + data.value;");
    let sink = info
        .security_sinks
        .iter()
        .find(|s| s.callee_path == "el.innerHTML")
        .expect("innerHTML sink captured");
    assert!(sink.arg_idents.iter().any(|n| n == "data"));
    assert!(!sink.arg_idents.iter().any(|n| n == "value"));
}

#[test]
fn sink_captures_direct_arg_source_paths() {
    let info = parse_ts("logger.error(process.env.SECRET_KEY);");
    let sink = info
        .security_sinks
        .iter()
        .find(|s| s.callee_path == "logger.error")
        .expect("logger sink captured");
    assert!(
        sink.arg_source_paths
            .iter()
            .any(|path| path == "process.env.SECRET_KEY")
    );
    assert!(
        sink.arg_source_paths
            .iter()
            .any(|path| path == "process.env")
    );
}

#[test]
fn sink_captures_typed_arg_idents_and_source_paths() {
    let info = parse_ts(
        "logger.error(secret as string);\nlogger.warn(process.env.API_TOKEN satisfies string);",
    );
    let error_sink = info
        .security_sinks
        .iter()
        .find(|s| s.callee_path == "logger.error")
        .expect("logger error sink captured");
    assert!(error_sink.arg_idents.iter().any(|n| n == "secret"));

    let warn_sink = info
        .security_sinks
        .iter()
        .find(|s| s.callee_path == "logger.warn")
        .expect("logger warn sink captured");
    assert!(
        warn_sink
            .arg_source_paths
            .iter()
            .any(|path| path == "process.env")
    );
}

#[test]
fn sink_captures_arg_idents_in_call_argument() {
    // `db.query(buildSql(userId))` -> references both the callee `buildSql` and
    // the nested argument `userId`.
    let info = parse_ts("db.query(buildSql(userId));");
    let sink = info
        .security_sinks
        .iter()
        .find(|s| s.callee_path == "db.query")
        .expect("query sink captured");
    assert!(sink.arg_idents.iter().any(|n| n == "buildSql"));
    assert!(sink.arg_idents.iter().any(|n| n == "userId"));
}

#[test]
fn sink_captures_arg_idents_in_tagged_template() {
    // ``sql`SELECT ${id}` `` -> references the substitution `id`.
    let info = parse_ts("const q = sql`SELECT * FROM t WHERE id = ${id}`;");
    let sink = info
        .security_sinks
        .iter()
        .find(|s| s.callee_path == "sql")
        .expect("tagged-template sink captured");
    assert!(sink.arg_idents.iter().any(|n| n == "id"));
}

#[test]
fn sink_captures_arg_idents_in_jsx_attr() {
    let info = parse_tsx("const C = () => <div dangerouslySetInnerHTML={markup} />;");
    let sink = info
        .security_sinks
        .iter()
        .find(|s| s.callee_path == "dangerouslySetInnerHTML")
        .expect("jsx-attr sink captured");
    assert!(sink.arg_idents.iter().any(|n| n == "markup"));
}

#[test]
fn direct_binding_records_object_path_as_source() {
    // `const id = req.query.id` -> { local: "id", source_path: "req.query" }.
    let info = parse_ts("const id = req.query.id;");
    let binding = info
        .tainted_bindings
        .iter()
        .find(|b| b.local == "id")
        .expect("tainted binding for id");
    assert_eq!(binding.source_path, "req.query");
}

#[test]
fn helper_call_binding_records_returned_param_source() {
    let info = parse_ts(
        r"
        function userId(req) {
            return req.query.id;
        }
        const id = userId(request);
        ",
    );
    assert!(
        info.tainted_bindings
            .iter()
            .any(|b| b.local == "id" && b.source_path == "request.query")
    );
}

#[test]
fn helper_call_binding_records_hoisted_function_source() {
    let info = parse_ts(
        r"
        const id = userId(request);
        function userId(req) {
            return req.query.id;
        }
        ",
    );
    assert!(
        info.tainted_bindings
            .iter()
            .any(|b| b.local == "id" && b.source_path == "request.query")
    );
}

#[test]
fn helper_call_binding_records_arrow_helper_source() {
    let info = parse_ts(
        r"
        const userId = (req) => req.query.id;
        const id = userId(request);
        ",
    );
    assert!(
        info.tainted_bindings
            .iter()
            .any(|b| b.local == "id" && b.source_path == "request.query")
    );
}

#[test]
fn helper_call_binding_records_function_expression_source() {
    let info = parse_ts(
        r"
        const userId = function (req) {
            return req.query.id;
        };
        const id = userId(request);
        ",
    );
    assert!(
        info.tainted_bindings
            .iter()
            .any(|b| b.local == "id" && b.source_path == "request.query")
    );
}

#[test]
fn helper_call_binding_does_not_follow_shadowed_helper_name() {
    let info = parse_ts(
        r"
        function userId(req) {
            return req.query.id;
        }
        function route(userId, request) {
            const id = userId(request);
            new RegExp(id);
        }
        ",
    );
    assert!(
        info.tainted_bindings.iter().all(|b| b.local != "id"),
        "a local parameter named userId must shadow the module helper"
    );
}

#[test]
fn helper_call_binding_is_one_hop_only() {
    let info = parse_ts(
        r"
        function userId(req) {
            return req.query.id;
        }
        function wrapped(req) {
            return userId(req);
        }
        const id = wrapped(request);
        ",
    );
    assert!(
        info.tainted_bindings.iter().all(|b| b.local != "id"),
        "helper calls returning another helper call are out of scope"
    );
}

#[test]
fn helper_call_binding_does_not_keep_stale_overridden_function_summary() {
    let info = parse_ts(
        r#"
        function userId(req) {
            return req.query.id;
        }
        function userId(req) {
            return "static";
        }
        const id = userId(request);
        "#,
    );
    assert!(
        info.tainted_bindings.iter().all(|b| b.local != "id"),
        "a later same-module declaration must replace the helper summary"
    );
}

#[test]
fn destructure_binding_records_full_init_path_as_source() {
    // `const { id, name } = req.body` -> both locals map to source_path "req.body".
    let info = parse_ts("const { id, name } = req.body;");
    for local in ["id", "name"] {
        let binding = info
            .tainted_bindings
            .iter()
            .find(|b| b.local == local)
            .unwrap_or_else(|| panic!("tainted binding for {local}"));
        assert_eq!(binding.source_path, "req.body");
    }
}

#[test]
fn await_init_unwraps_to_member_object_path() {
    // `const body = await ctx.req.json()` is a call result (no member-object to
    // drop), so it records nothing: a conservative miss, never a wrong link.
    let info = parse_ts("async function h() { const body = await ctx.req.json(); }");
    assert!(info.tainted_bindings.iter().all(|b| b.local != "body"));
}

#[test]
fn literal_init_records_no_source_binding() {
    let info = parse_ts("const x = 1;\nconst y = \"hello\";");
    assert!(info.tainted_bindings.is_empty());
}

#[test]
fn route_callback_param_records_framework_source() {
    let info = parse_ts("app.post('/run', (req) => { eval(req); });");
    let binding = info
        .tainted_bindings
        .iter()
        .find(|b| b.local == "req")
        .expect("route callback request param source");
    assert_eq!(binding.source_path, "framework.request");
}

#[test]
fn route_callback_destructured_param_records_framework_source() {
    let info = parse_ts("app.post('/run', ({ body }) => { eval(body); });");
    let binding = info
        .tainted_bindings
        .iter()
        .find(|b| b.local == "body")
        .expect("route callback destructured request param source");
    assert_eq!(binding.source_path, "framework.request");
}

#[test]
fn next_route_handler_param_records_next_request_source() {
    let info = parse_ts("export async function POST(request: Request) { eval(request); }");
    let binding = info
        .tainted_bindings
        .iter()
        .find(|b| b.local == "request")
        .expect("Next route request param source");
    assert_eq!(binding.source_path, "next.request");
}

#[test]
fn server_action_form_data_param_records_next_source() {
    let info =
        parse_ts("const action = async (formData: FormData) => { 'use server'; eval(formData); };");
    let binding = info
        .tainted_bindings
        .iter()
        .find(|b| b.local == "formData")
        .expect("server action FormData param source");
    assert_eq!(binding.source_path, "next.form-data");
}

#[test]
fn queue_process_callback_param_records_job_source() {
    let info = parse_ts("queue.process(async (job) => { eval(job); });");
    let binding = info
        .tainted_bindings
        .iter()
        .find(|b| b.local == "job")
        .expect("queue process job param source");
    assert_eq!(binding.source_path, "queue.job");
}

#[test]
fn queue_worker_constructor_param_records_job_source() {
    let info = parse_ts("new Worker('email', async ({ data }) => { eval(data); });");
    let binding = info
        .tainted_bindings
        .iter()
        .find(|b| b.local == "data")
        .expect("BullMQ worker destructured job param source");
    assert_eq!(binding.source_path, "queue.job");
}

#[test]
fn mcp_tool_callback_param_records_input_source() {
    let info = parse_ts("server.tool('lookup', schema, async ({ city }) => { eval(city); });");
    let binding = info
        .tainted_bindings
        .iter()
        .find(|b| b.local == "city")
        .expect("MCP tool input param source");
    assert_eq!(binding.source_path, "mcp.tool-input");
}
