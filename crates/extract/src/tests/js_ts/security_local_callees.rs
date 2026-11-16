use crate::tests::parse_ts as parse_source;

fn sink_paths(source: &str) -> Vec<String> {
    parse_source(source)
        .security_sinks
        .iter()
        .map(|sink| sink.callee_path.clone())
        .collect()
}

fn assert_no_sink(source: &str, path: &str) {
    let paths = sink_paths(source);
    assert!(
        !paths.iter().any(|observed| observed == path),
        "unexpected {path}, observed {paths:?}"
    );
}

fn assert_call_argument_sink(source: &str, path: &str, arg_index: u32) {
    let sinks = parse_source(source).security_sinks;
    assert!(
        sinks
            .iter()
            .any(|sink| sink.callee_path == path && sink.arg_index == arg_index),
        "expected {path} argument {arg_index}"
    );
}

fn assert_sink(source: &str, path: &str) {
    let paths = sink_paths(source);
    assert!(
        paths.iter().any(|observed| observed == path),
        "expected {path}, observed {paths:?}"
    );
}

#[test]
fn a_call_of_a_local_function_declaration_is_not_a_sink() {
    assert_no_sink(
        "function request(lines: number): number { return lines + 1; }\nconst lines = Number('20');\nconsole.log(request(lines));\n",
        "request",
    );
}

#[test]
fn a_call_before_a_hoisted_local_function_is_not_a_sink() {
    assert_no_sink(
        "export const run = (n: number) => request(n);\nfunction request(n: number) { return n; }\n",
        "request",
    );
}

#[test]
fn a_call_of_a_local_arrow_function_is_not_a_sink() {
    assert_no_sink(
        "const request = (n: number) => n + 1;\nexport const run = (n: number) => request(n);\n",
        "request",
    );
}

#[test]
fn an_inline_method_of_a_local_object_is_not_a_sink() {
    assert_no_sink(
        "const controller = { merge(action: string, branch: { name: string }) { return `${action} ${branch.name}`; } };\nconst branch = { name: 'feature' };\nconsole.log(controller.merge('merge', branch));\n",
        "controller.merge",
    );
}

#[test]
fn a_local_function_that_runs_a_call_stays_a_sink() {
    let source = "function request(url: string) { return fetch(url); }\nexport const run = (url: string) => request(url);\n";
    assert_sink(source, "request");
    assert_sink(source, "fetch");
}

#[test]
fn a_local_method_that_writes_a_member_stays_a_sink() {
    assert_sink(
        "const utils = { merge(target: any, source: any) { target.value = source; return target; } };\nexport const run = (a: object, b: object) => utils.merge(a, b);\n",
        "utils.merge",
    );
}

#[test]
fn a_function_overload_without_a_body_is_not_inert() {
    assert_sink(
        "declare function request(url: string): void;\nexport const run = (url: string) => request(url);\n",
        "request",
    );
}

#[test]
fn a_parameter_hides_a_local_function() {
    assert_sink(
        "function request(n: number) { return n; }\nexport const run = (request: (u: string) => void, url: string) => request(url);\n",
        "request",
    );
}

#[test]
fn a_method_that_is_not_inline_stays_a_sink() {
    assert_sink(
        "import _ from 'lodash';\nconst utils = { merge: _.merge };\nexport const run = (a: object, b: object) => utils.merge(a, b);\n",
        "utils.merge",
    );
}

#[test]
fn a_spread_after_a_method_keeps_the_sink() {
    assert_sink(
        "import _ from 'lodash';\nconst utils = { merge() { return 1; }, ..._ };\nexport const run = (a: object, b: object) => utils.merge(a, b);\n",
        "utils.merge",
    );
}

#[test]
fn a_reassigned_local_function_stays_a_sink() {
    assert_sink(
        "import got from 'got';\nlet request = (n: string) => n;\nrequest = got;\nexport const run = (url: string) => request(url);\n",
        "request",
    );
}

#[test]
fn an_unresolved_request_stays_a_sink() {
    assert_sink(
        "import request from 'request';\nexport const run = (url: string) => request(url);\n",
        "request",
    );
}

#[test]
fn a_local_function_reassigned_later_stays_a_sink() {
    assert_sink(
        "import got from 'got';\nlet request = (u: string) => u;\nexport const run = (u: string) => request(u);\nexport function configure() { request = got; }\n",
        "request",
    );
}

#[test]
fn a_method_replaced_by_a_member_write_stays_a_sink() {
    assert_call_argument_sink(
        "import _ from 'lodash';\nconst utils = { merge(a: object, b: object): object { return a; } };\nutils.merge = _.merge;\napp.post('/b', (req, res) => { utils.merge({}, req.body); });\n",
        "utils.merge",
        1,
    );
}

#[test]
fn a_method_of_an_object_passed_to_a_call_stays_a_sink() {
    assert_call_argument_sink(
        "import _ from 'lodash';\nconst utils = { merge(a: object, b: object) { return a; } };\nObject.assign(utils, { merge: _.merge });\nexport const run = (body: object) => utils.merge({}, body);\n",
        "utils.merge",
        1,
    );
}
