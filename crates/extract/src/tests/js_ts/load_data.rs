//! Tests for the SvelteKit `load()` producer harvest (`load_return_keys` /
//! `has_unharvestable_load`) and the FP-1 whole-`data` use signal
//! (`has_load_data_whole_use`).

use crate::tests::{parse_at_path, parse_ts};

fn key_names(info: &crate::ModuleInfo) -> Vec<String> {
    info.load_return_keys
        .iter()
        .map(|k| k.name.clone())
        .collect()
}

#[test]
fn harvests_object_literal_keys_from_arrow_load() {
    let info = parse_at_path(
        "src/routes/+page.ts",
        "export const load = async () => { return { used: 1, dead: 2 }; };",
    );
    assert_eq!(key_names(&info), vec!["used", "dead"]);
    assert!(!info.has_unharvestable_load);
}

#[test]
fn harvests_keys_from_async_function_load() {
    let info = parse_at_path(
        "src/routes/+page.server.ts",
        "export async function load() { return { a: 1, b: 2 }; }",
    );
    assert_eq!(key_names(&info), vec!["a", "b"]);
    assert!(!info.has_unharvestable_load);
}

#[test]
fn harvests_through_satisfies_pageload() {
    let info = parse_at_path(
        "src/routes/+page.ts",
        "export const load = (async () => ({ x: 1 })) satisfies PageLoad;",
    );
    assert_eq!(key_names(&info), vec!["x"]);
    assert!(!info.has_unharvestable_load);
}

#[test]
fn abstains_on_spread_return() {
    let info = parse_at_path(
        "src/routes/+page.ts",
        "export const load = async () => { return { ...base, extra: 1 }; };",
    );
    assert!(info.load_return_keys.is_empty());
    assert!(info.has_unharvestable_load);
}

#[test]
fn abstains_on_non_object_return() {
    let info = parse_at_path(
        "src/routes/+page.ts",
        "export const load = async () => { return makeData(); };",
    );
    assert!(info.load_return_keys.is_empty());
    assert!(info.has_unharvestable_load);
}

#[test]
fn abstains_on_multi_return_body() {
    let info = parse_at_path(
        "src/routes/+page.ts",
        "export async function load(x) { if (x) { return { a: 1 }; } return { b: 2 }; }",
    );
    assert!(info.has_unharvestable_load);
}

#[test]
fn abstains_on_computed_key() {
    let info = parse_at_path(
        "src/routes/+page.ts",
        "export const load = async () => { return { [k]: 1 }; };",
    );
    assert!(info.has_unharvestable_load);
}

#[test]
fn abstains_on_reexported_load() {
    let info = parse_at_path("src/routes/+page.ts", "export { load } from './shared';");
    assert!(info.load_return_keys.is_empty());
    assert!(info.has_unharvestable_load);
}

#[test]
fn non_page_file_harvests_nothing() {
    // A plain module exporting a `load` is not a SvelteKit page producer; the
    // basename gate in parse.rs clears the harvest.
    let info = parse_at_path(
        "src/lib/helpers.ts",
        "export const load = async () => { return { a: 1 }; };",
    );
    assert!(info.load_return_keys.is_empty());
    assert!(!info.has_unharvestable_load);
}

// FP-1: the four whole-`data` use forms must set `has_load_data_whole_use`.

#[test]
fn whole_data_use_script_const_assignment() {
    let info = parse_ts("const d = data;");
    assert!(
        info.has_load_data_whole_use,
        "const X = data is a whole use"
    );
}

#[test]
fn whole_data_use_function_call_arg() {
    let info = parse_ts("someFn(data);");
    assert!(info.has_load_data_whole_use, "fn(data) is a whole use");
}

#[test]
fn whole_data_use_spread_call_arg() {
    let info = parse_ts("someFn(...data);");
    assert!(info.has_load_data_whole_use, "fn(...data) is a whole use");
}

#[test]
fn whole_data_use_destructure_assignment() {
    // `({ guests } = data)` inside an effect/reactive block is an ASSIGNMENT,
    // not a declaration, so Primitive A does not credit the keys; the whole-use
    // signal must fire so the detector abstains. (syntaxfm guests FP.)
    let info = parse_ts("let guests; ({ guests } = data);");
    assert!(
        info.has_load_data_whole_use,
        "({{ x }} = data) destructure-assignment is a whole-data use"
    );
}

#[test]
fn member_access_on_data_is_not_a_whole_use() {
    // `data.x` is a credited member access, NOT a whole-object use.
    let info = parse_ts("const x = data.title;");
    assert!(
        !info.has_load_data_whole_use,
        "data.x member access must not set the whole-use flag"
    );
}

#[test]
fn non_data_binding_does_not_trip_whole_use() {
    let info = parse_ts("const d = other; fn(other);");
    assert!(
        !info.has_load_data_whole_use,
        "only the `data` binding is name-gated for the whole-use signal"
    );
}
