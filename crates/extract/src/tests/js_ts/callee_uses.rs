use crate::tests::parse_ts as parse_source;

#[test]
fn captures_bare_identifier_call() {
    let info = parse_source("import { execSync } from 'node:child_process';\nexecSync('ls');");
    assert!(
        info.callee_uses.iter().any(|u| u.callee_path == "execSync"),
        "bare identifier call should be captured"
    );
}

#[test]
fn captures_static_member_chain_as_dotted_path() {
    let info = parse_source("import * as cp from 'child_process';\ncp.exec('ls');");
    assert!(
        info.callee_uses.iter().any(|u| u.callee_path == "cp.exec"),
        "static member call should flatten to a dotted path"
    );
}

#[test]
fn captures_global_callee_without_import() {
    let info = parse_source("console.log('hi');\nconsole.table([]);");
    assert!(
        info.callee_uses
            .iter()
            .any(|u| u.callee_path == "console.log")
    );
    assert!(
        info.callee_uses
            .iter()
            .any(|u| u.callee_path == "console.table")
    );
}

#[test]
fn dedupes_repeated_callee_to_first_span() {
    let source = "console.log('a');\nconsole.log('b');\nconsole.log('c');";
    let info = parse_source(source);
    let uses: Vec<_> = info
        .callee_uses
        .iter()
        .filter(|u| u.callee_path == "console.log")
        .collect();
    assert_eq!(uses.len(), 1, "duplicate callee paths should dedup");
    assert_eq!(uses[0].span_start, 0, "first occurrence span should win");
}

#[test]
fn unwraps_parenthesized_callee() {
    let info = parse_source("(console.log)('hi');");
    assert!(
        info.callee_uses
            .iter()
            .any(|u| u.callee_path == "console.log"),
        "parenthesized callee should unwrap"
    );
}

#[test]
fn skips_computed_member_callee() {
    let info = parse_source("const m = 'log';\nconsole[m]('hi');");
    assert!(
        !info
            .callee_uses
            .iter()
            .any(|u| u.callee_path.starts_with("console")),
        "computed member callee is not statically flattenable"
    );
}

#[test]
fn skips_dynamic_dispatch_callee() {
    // `factory()` itself is a captured identifier call; the OUTER call whose
    // callee is the call result is dynamic dispatch and stays uncaptured.
    let info = parse_source("factory()();");
    assert_eq!(info.callee_uses.len(), 1);
    assert_eq!(info.callee_uses[0].callee_path, "factory");
}

#[test]
fn coexists_with_security_sink_capture() {
    let info = parse_source(
        "import { exec } from 'node:child_process';\nconst cmd = userInput();\nexec(cmd);",
    );
    assert!(
        info.callee_uses.iter().any(|u| u.callee_path == "exec"),
        "callee-use capture should coexist with sink capture"
    );
    assert!(
        info.security_sinks.iter().any(|s| s.callee_path == "exec"),
        "security sink capture should be unaffected"
    );
}

#[test]
fn zero_arg_and_fully_literal_calls_are_captured() {
    // The security sink channel skips fully-literal and zero-arg calls; the
    // banned-call policy is about WHO calls WHAT, so both must be captured.
    let info = parse_source(
        "import { execSync } from 'child_process';\nexecSync('ls');\nconsole.table();",
    );
    assert!(info.callee_uses.iter().any(|u| u.callee_path == "execSync"));
    assert!(
        info.callee_uses
            .iter()
            .any(|u| u.callee_path == "console.table")
    );
}
