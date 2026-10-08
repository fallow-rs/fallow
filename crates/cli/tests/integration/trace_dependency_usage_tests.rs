//! `fallow trace --dependency <PKG>` reports how the code uses each imported
//! name of a package. `fallow dead-code --trace-dependency <PKG>` keeps its
//! old output on the same fixture.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "integration tests keep fixture setup concise"
)]

use crate::common::{fixture_path, parse_json, redact_paths, run_fallow, run_fallow_in_root};
use serde_json::{Value, json};

const FIXTURE: &str = "trace-dependency-usage";
const PACKAGE: &str = "react-redux";

/// Drop the run metadata, which changes on each run.
fn without_meta(mut value: Value) -> Value {
    if let Some(object) = value.as_object_mut() {
        object.remove("_meta");
    }
    value
}

fn dead_code_trace(extra: &[&str]) -> Value {
    let mut args = vec![
        "--trace-dependency",
        PACKAGE,
        "--format",
        "json",
        "--quiet",
        "--no-cache",
    ];
    args.extend_from_slice(extra);
    without_meta(parse_json(&run_fallow("dead-code", FIXTURE, &args)))
}

fn usage_trace(extra: &[&str]) -> Value {
    let mut args = vec![
        "--dependency",
        PACKAGE,
        "--format",
        "json",
        "--quiet",
        "--no-cache",
    ];
    args.extend_from_slice(extra);
    without_meta(parse_json(&run_fallow("trace", FIXTURE, &args)))
}

/// Each site as `file:line:col specifier local kind via member`.
fn site_rows(trace: &Value) -> Vec<String> {
    let text = |site: &Value, key: &str| site[key].as_str().unwrap_or("").to_owned();
    trace["usage"]["sites"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|site| {
            format!(
                "{}:{}:{} {} {} {} {} {}",
                text(site, "file"),
                site["line"],
                site["col"],
                text(site, "specifier"),
                text(site, "local_name"),
                text(site, "kind"),
                text(site, "via"),
                text(site, "member"),
            )
            .trim_end()
            .to_owned()
        })
        .collect()
}

/// The 24 sites of the fixture, in order.
const ALL_SITES: [&str; 24] = [
    "src/components/Alias.tsx:4:17 useStore useStore value_alias",
    "src/components/Alias.tsx:5:27 Provider Provider jsx_element",
    "src/components/Badge.tsx:3:31 useSelector useAppSelector call src/store/hooks.ts:useAppSelector",
    "src/components/Counter.tsx:5:16 useSelector useAppSelector call src/store/hooks.ts:useAppSelector",
    "src/components/Counter.tsx:6:16 useSelector useAppSelector call src/store/hooks.ts:useAppSelector",
    "src/components/Counter.tsx:7:14 useSelector useSel call",
    "src/components/Counter.tsx:8:19 useDispatch useAppDispatch call src/store/hooks.ts:useAppDispatch",
    "src/components/Counter.tsx:13:15 connect connect call",
    "src/components/Namespace.tsx:5:3 Provider RR jsx_element",
    "src/components/Namespace.tsx:6:18 useSelector RR call",
    "src/components/Namespace.tsx:9:22 useDispatch RR non_call_reference",
    "src/legacy.js:1:20   require",
    "src/legacy.js:2:26   dynamic_import",
    "src/main.tsx:11:84 useSelector usePick non_call_reference src/store/local.ts:usePick",
    "src/store/barrel.ts:1:9 useSelector useAppSelector re_export src/store/hooks.ts:useAppSelector",
    "src/store/hooks.ts:4:30 useSelector useSelector wrapper_definition  withTypes",
    "src/store/hooks.ts:5:49 useDispatch useDispatch wrapper_definition",
    "src/store/hooks.ts:6:35 useSelector useAppSelector call src/store/hooks.ts:useAppSelector",
    "src/store/local.ts:3:20 useSelector pick call  withTypes",
    "src/store/local.ts:4:18 useSelector pick wrapper_definition",
    "src/store/local.ts:7:27 useSelector pick non_call_reference",
    "src/store/nested.ts:3:24 useSelector useAppSelector nested_wrapper src/store/hooks.ts:useAppSelector",
    "src/store/reexports.ts:1:9 useStore useStore re_export",
    "src/store/reexports.ts:2:0   star_re_export",
];

#[test]
fn dead_code_trace_dependency_output_is_unchanged() {
    let trace = dead_code_trace(&[]);
    assert!(trace.get("usage").is_none(), "{trace:#}");
    insta::assert_snapshot!(
        "trace_dependency_usage_dead_code_json",
        serde_json::to_string_pretty(&trace).unwrap()
    );
}

#[test]
fn counts_each_imported_name() {
    let trace = usage_trace(&[]);
    let usage = &trace["usage"];
    assert_eq!(usage["schema_version"], "1");
    assert_eq!(usage["confidence"], "syntactic");
    assert!(usage.get("sites").is_none(), "{usage:#}");
    assert!(usage.get("closure").is_none(), "{usage:#}");
    let zero = json!({
        "value_alias": 0, "non_call_reference": 0, "jsx_element": 0,
        "re_export": 0, "nested_wrapper": 0, "binding_without_site": 0
    });
    let unresolved = |pairs: &[(&str, usize)]| {
        let mut value = zero.clone();
        for (key, count) in pairs {
            value[*key] = json!(count);
        }
        value
    };
    let entry = |name: &str,
                 files: usize,
                 type_only: usize,
                 calls: usize,
                 reasons: &[(&str, usize)],
                 wrappers: Value| {
        json!({
            "name": name,
            "file_count": files,
            "type_only_file_count": type_only,
            "call_site_count": calls,
            "unresolved": unresolved(reasons),
            "wrappers": wrappers,
        })
    };
    assert_eq!(
        usage["specifiers"],
        json!([
            entry("Provider", 2, 0, 0, &[("jsx_element", 2)], json!([])),
            entry("TypedUseSelectorHook", 1, 1, 0, &[], json!([])),
            entry("connect", 1, 0, 1, &[], json!([])),
            entry(
                "useDispatch",
                2,
                0,
                0,
                &[("non_call_reference", 1)],
                json!([{
                    "file": "src/store/hooks.ts", "export": "useAppDispatch", "shape": "alias",
                    "line": 5, "consumer_file_count": 1, "call_site_count": 1
                }])
            ),
            entry(
                "useSelector",
                4,
                0,
                4,
                &[
                    ("non_call_reference", 2),
                    ("re_export", 1),
                    ("nested_wrapper", 1)
                ],
                json!([
                    {
                        "file": "src/store/hooks.ts", "export": "useAppSelector", "shape": "call",
                        "line": 4, "consumer_file_count": 3, "call_site_count": 4
                    },
                    {
                        "file": "src/store/local.ts", "export": "usePick", "shape": "alias",
                        "line": 4, "consumer_file_count": 0, "call_site_count": 0
                    }
                ])
            ),
            entry(
                "useStore",
                2,
                0,
                0,
                &[("value_alias", 1), ("re_export", 1)],
                json!([])
            ),
        ]),
        "{usage:#}"
    );
    assert_eq!(
        usage["unresolved"],
        json!({
            "dynamic_import": 1, "require": 1, "side_effect_import": 0,
            "star_re_export": 1, "unattributed_file": 0
        })
    );
}

#[test]
fn lists_every_site_in_order() {
    let trace = usage_trace(&["--sites"]);
    let page = &trace["usage"]["sites"];
    assert_eq!(page["total"], 24, "{page:#}");
    assert_eq!(page["limit"], 50);
    assert!(page.get("next_cursor").is_none(), "{page:#}");
    assert_eq!(site_rows(&trace), ALL_SITES);
}

#[test]
fn a_cursor_pages_through_the_sites() {
    let mut cursor: Option<String> = None;
    let mut rows = Vec::new();
    let mut sizes = Vec::new();
    loop {
        let mut extra = vec!["--limit", "5"];
        if let Some(token) = cursor.as_deref() {
            extra.extend(["--cursor", token]);
        }
        let trace = usage_trace(&extra);
        let page = &trace["usage"]["sites"];
        assert_eq!(page["total"], 24);
        let page_rows = site_rows(&trace);
        sizes.push(page_rows.len());
        rows.extend(page_rows);
        match page["next_cursor"].as_str() {
            Some(next) => cursor = Some(next.to_owned()),
            None => break,
        }
    }
    assert_eq!(sizes, vec![5, 5, 5, 5, 4]);
    assert_eq!(rows, ALL_SITES);
}

#[test]
fn a_cursor_of_another_query_exits_2() {
    let first = usage_trace(&["--limit", "5"]);
    let cursor = first["usage"]["sites"]["next_cursor"].as_str().unwrap();
    let output = run_fallow(
        "trace",
        FIXTURE,
        &[
            "--dependency",
            PACKAGE,
            "--specifier",
            "useSelector",
            "--cursor",
            cursor,
            "--format",
            "json",
            "--quiet",
            "--no-cache",
        ],
    );
    assert_eq!(output.code, 2, "{}", output.stdout);
    assert!(output.stdout.contains("cursor"), "{}", output.stdout);
}

#[test]
fn a_specifier_selects_its_sites_and_keeps_file_level_counts() {
    let trace = usage_trace(&["--specifier", "useSelector"]);
    let usage = &trace["usage"];
    let names: Vec<_> = usage["specifiers"]
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| entry["name"].clone())
        .collect();
    assert_eq!(names, vec![json!("useSelector")]);
    let expected: Vec<&str> = [3, 4, 5, 6, 10, 14, 15, 16, 18, 19, 20, 21, 22]
        .iter()
        .map(|row| ALL_SITES[row - 1])
        .collect();
    assert_eq!(site_rows(&trace), expected);
    assert_eq!(usage["sites"]["total"], 13);
    assert_eq!(usage["unresolved"]["require"], 1);
    assert_eq!(usage["unresolved"]["dynamic_import"], 1);
    assert_eq!(usage["unresolved"]["star_re_export"], 1);
}

#[test]
fn a_specifier_that_no_file_imports_has_zero_counts() {
    let trace = usage_trace(&["--specifier", "shallowEqual"]);
    let usage = &trace["usage"];
    assert_eq!(usage["specifiers"][0]["name"], "shallowEqual");
    assert_eq!(usage["specifiers"][0]["file_count"], 0);
    assert_eq!(usage["sites"]["total"], 0);
    assert_eq!(usage["sites"]["items"], json!([]));
}

#[test]
fn callers_add_the_files_that_import_the_users() {
    let trace = usage_trace(&["--callers", "--depth", "1"]);
    assert_eq!(
        trace["usage"]["closure"],
        json!({
            "depth": 1,
            "file_count": 4,
            "files": [
                {"file": "src/components/Badge.tsx", "depth": 1},
                {"file": "src/main.tsx", "depth": 1},
                {"file": "src/store/barrel.ts", "depth": 1},
                {"file": "src/store/nested.ts", "depth": 1}
            ],
            "truncated": true
        })
    );
    // The users of `useSelector` include the barrel. Its importer is the only
    // file that the walk adds.
    let selected = usage_trace(&["--specifier", "useSelector", "--callers", "--depth", "1"]);
    assert_eq!(
        selected["usage"]["closure"],
        json!({
            "depth": 1,
            "file_count": 1,
            "files": [{"file": "src/components/Total.tsx", "depth": 1}],
            "truncated": false
        })
    );
}

#[test]
fn a_call_wrapper_counts_method_calls_on_its_result_as_references() {
    // Redux Toolkit: `store.dispatch()` calls a method of the store, not
    // `configureStore`.
    let trace = without_meta(parse_json(&run_fallow(
        "trace",
        FIXTURE,
        &[
            "--dependency",
            "@reduxjs/toolkit",
            "--sites",
            "--format",
            "json",
            "--quiet",
            "--no-cache",
        ],
    )));
    let calls: Vec<_> = trace["usage"]["specifiers"]
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| {
            (
                entry["name"].clone(),
                entry["call_site_count"].clone(),
                entry["unresolved"]["non_call_reference"].clone(),
                entry["wrappers"][0]["call_site_count"].clone(),
            )
        })
        .collect();
    assert_eq!(
        calls,
        vec![
            (json!("configureStore"), json!(1), json!(2), json!(0)),
            (json!("createSlice"), json!(1), json!(2), json!(0)),
        ]
    );
    assert_eq!(
        site_rows(&trace),
        vec![
            "src/rtk/app.ts:3:0 configureStore store non_call_reference src/rtk/store.ts:store dispatch",
            "src/rtk/app.ts:3:15 createSlice slice non_call_reference src/rtk/store.ts:slice actions.inc",
            "src/rtk/app.ts:4:21 configureStore store non_call_reference src/rtk/store.ts:store getState",
            "src/rtk/store.ts:3:21 createSlice createSlice wrapper_definition",
            "src/rtk/store.ts:4:21 configureStore configureStore wrapper_definition",
            "src/rtk/store.ts:4:47 createSlice slice non_call_reference src/rtk/store.ts:slice reducer",
        ]
    );
}

#[test]
fn a_depth_out_of_range_exits_2() {
    for depth in ["0", "11"] {
        let output = run_fallow(
            "trace",
            FIXTURE,
            &[
                "--dependency",
                PACKAGE,
                "--callers",
                "--depth",
                depth,
                "--format",
                "json",
                "--quiet",
            ],
        );
        assert_eq!(output.code, 2, "{depth}: {}", output.stdout);
    }
}

#[test]
fn other_formats_exit_2() {
    let output = run_fallow(
        "trace",
        FIXTURE,
        &["--dependency", PACKAGE, "--format", "sarif", "--quiet"],
    );
    assert_eq!(output.code, 2, "{}", output.stdout);
}

/// The fields that `fallow dead-code --trace-dependency` emits.
fn base_fields(mut trace: Value) -> Value {
    trace.as_object_mut().unwrap().remove("usage");
    trace
}

#[test]
fn base_fields_match_the_dead_code_trace() {
    assert_eq!(base_fields(usage_trace(&[])), dead_code_trace(&[]));
    assert_eq!(
        base_fields(usage_trace(&["--production"])),
        dead_code_trace(&["--production"])
    );
}

#[test]
fn base_fields_match_the_dead_code_trace_in_a_workspace() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let write = |path: &str, content: &str| {
        let path = root.join(path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, content).unwrap();
    };
    write(
        "package.json",
        r#"{"name":"mono","private":true,"workspaces":["packages/*"]}"#,
    );
    write(
        "packages/app/package.json",
        r#"{"name":"app","main":"src/index.ts","dependencies":{"react-redux":"^9.0.0"}}"#,
    );
    write(
        "packages/app/src/index.ts",
        "import { useSelector } from 'react-redux';\nexport const read = () => useSelector(1);\n",
    );
    write(
        "packages/lib/package.json",
        r#"{"name":"lib","main":"src/index.ts","dependencies":{"react-redux":"^9.0.0"}}"#,
    );
    write("packages/lib/src/index.ts", "export const value = 1;\n");
    let args = |command: &str| -> Value {
        let target = if command == "trace" {
            "--dependency"
        } else {
            "--trace-dependency"
        };
        without_meta(parse_json(&run_fallow_in_root(
            command,
            root,
            &[
                target,
                PACKAGE,
                "--workspace",
                "app",
                "--format",
                "json",
                "--quiet",
                "--no-cache",
            ],
        )))
    };
    let trace = args("trace");
    assert_eq!(trace["usage"]["specifiers"][0]["call_site_count"], 1);
    assert_eq!(base_fields(trace), args("dead-code"));
}

#[test]
fn human_output_lists_the_usage() {
    let root = fixture_path(FIXTURE);
    let output = run_fallow(
        "trace",
        FIXTURE,
        &[
            "--dependency",
            PACKAGE,
            "--limit",
            "5",
            "--callers",
            "--depth",
            "1",
            "--format",
            "human",
            "--quiet",
            "--no-cache",
        ],
    );
    assert_eq!(output.code, 0, "{}", output.stderr);
    insta::assert_snapshot!(
        "trace_dependency_usage_human",
        redact_paths(&output.stdout, &root)
    );
}
