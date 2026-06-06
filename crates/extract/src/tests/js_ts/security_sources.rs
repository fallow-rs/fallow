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
