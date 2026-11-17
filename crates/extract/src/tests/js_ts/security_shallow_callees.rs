use crate::tests::parse_ts as parse_source;

const SHALLOW_COPY: &str = "function merge(obj: object, subset: object) {\n  const copy = Object.assign({}, obj);\n  for (const k in subset) {\n    copy[k] = subset[k];\n  }\n  return copy;\n}\n";

fn with_call(function: &str) -> String {
    format!("{function}export const run = (a: object, b: object) => merge(a, b);\n")
}

fn has_merge_sink(source: &str) -> bool {
    parse_source(source)
        .security_sinks
        .iter()
        .any(|sink| sink.callee_path == "merge")
}

fn assert_no_merge_sink(function: &str) {
    assert!(
        !has_merge_sink(&with_call(function)),
        "unexpected merge sink for {function}"
    );
}

fn assert_merge_sink(function: &str) {
    assert!(
        has_merge_sink(&with_call(function)),
        "expected a merge sink for {function}"
    );
}

fn inert_exports(source: &str) -> Vec<String> {
    let mut names = parse_source(source).inert_function_exports;
    names.sort();
    names
}

#[test]
fn a_shallow_copy_into_a_fresh_local_is_inert() {
    assert_no_merge_sink(SHALLOW_COPY);
}

#[test]
fn a_spread_copy_with_object_builtins_is_inert() {
    assert_no_merge_sink(
        "function merge(base: object, extra: object) {\n  const copy = { ...base };\n  for (const [key, value] of Object.entries(extra)) { copy[key] = value; }\n  return Object.freeze(copy);\n}\n",
    );
}

#[test]
fn a_write_to_an_object_create_null_local_is_inert() {
    assert_no_merge_sink(
        "function merge(a: object, b: object) {\n  const out = Object.create(null);\n  for (const k of Object.keys(b)) { out[k] = b[k]; }\n  return out;\n}\n",
    );
}

#[test]
fn a_recursive_deep_merge_stays_a_sink() {
    assert_merge_sink(
        "function merge(target: any, source: any) {\n  for (const key of Object.keys(source)) {\n    if (typeof source[key] === 'object') { target[key] = merge(target[key] ?? {}, source[key]); }\n    else { target[key] = source[key]; }\n  }\n  return target;\n}\n",
    );
}

#[test]
fn a_recursive_merge_into_a_fresh_local_stays_a_sink() {
    assert_merge_sink(
        "function merge(a: any, b: any) {\n  const out = {};\n  for (const k in b) { out[k] = merge(a[k], b[k]); }\n  return out;\n}\n",
    );
}

#[test]
fn a_write_two_levels_deep_stays_a_sink() {
    assert_merge_sink(
        "function merge(a: string, b: string, v: unknown) {\n  const out = {};\n  out[a][b] = v;\n  return out;\n}\n",
    );
}

#[test]
fn a_write_to_a_parameter_stays_a_sink() {
    assert_merge_sink(
        "function merge(target: any, source: any) {\n  for (const k in source) { target[k] = source[k]; }\n  return target;\n}\n",
    );
}

#[test]
fn a_write_to_an_outer_binding_stays_a_sink() {
    assert_merge_sink(
        "const store = {};\nfunction merge(key: string, value: unknown) {\n  store[key] = value;\n  return store;\n}\n",
    );
}

#[test]
fn an_assign_into_a_parameter_stays_a_sink() {
    assert_merge_sink(
        "function merge(target: object, source: object) {\n  return Object.assign(target, source);\n}\n",
    );
}

#[test]
fn a_fresh_local_reassigned_to_a_parameter_stays_a_sink() {
    assert_merge_sink(
        "function merge(target: any, source: any) {\n  let out = {};\n  out = target;\n  for (const k in source) { out[k] = source[k]; }\n  return out;\n}\n",
    );
}

#[test]
fn a_local_from_a_parameter_stays_a_sink() {
    assert_merge_sink(
        "function merge(target: any, source: any) {\n  const out = target;\n  for (const k in source) { out[k] = source[k]; }\n  return out;\n}\n",
    );
}

#[test]
fn an_object_builtin_outside_the_list_stays_a_sink() {
    assert_merge_sink(
        "function merge(a: object, key: string, value: unknown) {\n  const out = {};\n  Object.defineProperty(out, key, { value });\n  return out;\n}\n",
    );
}

#[test]
fn a_fetch_in_the_body_stays_a_sink() {
    assert_merge_sink(
        "function merge(a: string, b: string) {\n  const out = {};\n  out.r = fetch(a + b);\n  return out;\n}\n",
    );
}

#[test]
fn exported_inert_functions_are_recorded() {
    let source = format!(
        "export {SHALLOW_COPY}export const request = (url: string) => url.length;\nconst assignAll = (a: object, b: object) => Object.assign({{}}, a, b);\nexport {{ assignAll, assignAll as combine }};\n"
    );
    assert_eq!(
        inert_exports(&source),
        vec!["assignAll", "combine", "merge", "request"]
    );
}

#[test]
fn an_inert_default_export_is_recorded() {
    assert_eq!(
        inert_exports("export default function (a: number) { return a + 1; }\n"),
        vec!["default"]
    );
    assert_eq!(
        inert_exports("export default (a: number) => a + 1;\n"),
        vec!["default"]
    );
    assert_eq!(
        inert_exports("function merge(a: number) { return a; }\nexport default merge;\n"),
        vec!["default"]
    );
}

#[test]
fn exports_with_effects_or_without_a_local_body_are_not_recorded() {
    assert!(
        inert_exports("export function request(url: string) { return fetch(url); }\n").is_empty()
    );
    assert!(inert_exports("export { merge } from './merge';\n").is_empty());
    assert!(inert_exports("export declare function merge(a: object): object;\n").is_empty());
    assert_eq!(
        inert_exports(
            "import _ from 'lodash';\nexport let merge = (a: object) => a;\nexport function configure() { merge = _.merge; }\n"
        ),
        vec!["configure"]
    );
}

#[test]
fn a_fresh_local_in_an_inner_block_does_not_free_an_outer_write() {
    assert_merge_sink(
        "const target: Record<string, unknown> = {};\nfunction merge(key: string, value: unknown) {\n  {\n    const target = {};\n    void target;\n  }\n  target[key] = value;\n  return target;\n}\n",
    );
}

#[test]
fn a_fresh_local_in_a_nested_function_does_not_free_an_outer_write() {
    assert_merge_sink(
        "const target: Record<string, unknown> = {};\nfunction merge(key: string, value: unknown) {\n  const make = () => { const target = {}; return target; };\n  target[key] = value;\n  return make;\n}\n",
    );
}

#[test]
fn an_imported_object_binding_is_not_the_object_builtin() {
    assert_merge_sink(&format!(
        "import {{ Object }} from './fakeobj';\n{SHALLOW_COPY}"
    ));
}

#[test]
fn a_local_object_binding_is_not_the_object_builtin() {
    assert_merge_sink(
        "function merge(obj: object, subset: object, Object: any) {\n  const copy = Object.assign({}, obj);\n  for (const k in subset) { copy[k] = subset[k]; }\n  return copy;\n}\n",
    );
}

#[test]
fn an_imported_object_binding_records_no_inert_export() {
    assert!(
        inert_exports(&format!(
            "import {{ Object }} from './fakeobj';\nexport {SHALLOW_COPY}"
        ))
        .is_empty()
    );
}
