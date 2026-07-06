//! Tests for the SvelteKit `load()` producer harvest (`load_return_keys` /
//! `has_unharvestable_load`) and the FP-1 whole-`data` use signal
//! (`has_load_data_whole_use`).

use crate::tests::{parse_at_path, parse_ts};
use crate::visitor::ROUTE_LOADER_DATA_OBJECT;

fn key_names(info: &crate::ModuleInfo) -> Vec<String> {
    info.load_return_keys
        .iter()
        .map(|k| k.name.clone())
        .collect()
}

fn has_route_data_access(info: &crate::ModuleInfo, member: &str) -> bool {
    info.member_accesses
        .iter()
        .any(|access| access.object == ROUTE_LOADER_DATA_OBJECT && access.member == member)
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

#[test]
fn sveltekit_page_ignores_route_loader_exports() {
    let info = parse_at_path(
        "src/routes/+page.ts",
        "export const loader = async () => ({ routeOnly: 1 }); export const load = async () => ({ pageOnly: 1 });",
    );
    assert_eq!(key_names(&info), vec!["pageOnly"]);
    assert!(!info.has_unharvestable_load);
}

#[test]
fn harvests_object_literal_keys_from_react_router_loader() {
    let info = parse_at_path(
        "app/routes/home.tsx",
        "export async function loader() { return { used: 1, dead: 2 }; }",
    );
    assert_eq!(key_names(&info), vec!["used", "dead"]);
    assert!(!info.has_unharvestable_load);
}

#[test]
fn conventional_route_ignores_sveltekit_load_exports() {
    let info = parse_at_path(
        "app/routes/home.tsx",
        "export const load = async () => ({ pageOnly: 1 }); export const loader = async () => ({ routeOnly: 1 });",
    );
    assert_eq!(key_names(&info), vec!["routeOnly"]);
    assert!(!info.has_unharvestable_load);
}

#[test]
fn harvests_object_literal_keys_from_client_loader() {
    let info = parse_at_path(
        "app/routes/home.tsx",
        "export const clientLoader = async () => ({ local: 1, stale: 2 });",
    );
    assert_eq!(key_names(&info), vec!["local", "stale"]);
    assert!(!info.has_unharvestable_load);
}

#[test]
fn harvests_remix_json_return_keys() {
    let info = parse_at_path(
        "app/routes/home.tsx",
        "export const loader = async () => json({ used: 1, dead: 2 });",
    );
    assert_eq!(key_names(&info), vec!["used", "dead"]);
    assert!(!info.has_unharvestable_load);
}

#[test]
fn route_loader_data_destructure_records_key_reads() {
    let info = parse_at_path(
        "app/routes/home.tsx",
        "import { useLoaderData } from 'react-router'; const { used } = useLoaderData() as Data;",
    );
    assert!(has_route_data_access(&info, "used"));
}

#[test]
fn route_loader_data_alias_member_records_key_reads() {
    let info = parse_at_path(
        "app/routes/home.tsx",
        "import { useLoaderData as useData } from '@remix-run/react'; const data = useData(); console.log(data.used);",
    );
    assert!(has_route_data_access(&info, "used"));
}

#[test]
fn route_loader_data_namespace_import_records_key_reads() {
    let info = parse_at_path(
        "app/routes/home.tsx",
        "import * as Router from 'react-router-dom'; const { used } = Router.useLoaderData();",
    );
    assert!(has_route_data_access(&info, "used"));
}

#[test]
fn route_loader_data_rest_destructure_marks_whole_use() {
    let info = parse_at_path(
        "app/routes/home.tsx",
        "import { useLoaderData } from 'react-router'; const { used, ...rest } = useLoaderData();",
    );
    assert!(
        info.whole_object_uses
            .iter()
            .any(|name| name == ROUTE_LOADER_DATA_OBJECT)
    );
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
