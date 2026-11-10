//! `stable_finding_id` hashes the exact source text of the sink expression.
//!
//! Each test writes a fixture, then checks three things: sinks with different
//! text get different base ids, an unrelated line above the code keeps every
//! id, and a reformat of the sinks keeps every id.

#![expect(
    clippy::expect_used,
    reason = "tests use expect to keep fixture setup concise"
)]

use std::path::Path;

use crate::common::{parse_json, run_fallow_in_root};

const PACKAGE_JSON: &str =
    r#"{ "name": "stable-id", "private": true, "type": "module", "main": "src/index.js" }"#;

/// A comment line that goes above the first line of each fixture.
const UNRELATED_LINE: &str = "// an unrelated comment\n";

fn write_project(root: &Path, file: &str, source: &str) {
    std::fs::create_dir_all(root.join("src")).expect("create src");
    std::fs::write(root.join("package.json"), PACKAGE_JSON).expect("write package.json");
    std::fs::write(root.join(file), source).expect("write source");
}

/// The config that turns on the include-required `hardcoded-secret` category.
const SECRET_CONFIG: &str =
    r#"{ "security": { "categories": { "include": ["hardcoded-secret"] } } }"#;

fn findings_for(file: &str, source: &str) -> Vec<serde_json::Value> {
    findings_with_config(file, source, None)
}

fn findings_with_config(file: &str, source: &str, config: Option<&str>) -> Vec<serde_json::Value> {
    let project = tempfile::tempdir().expect("project dir");
    write_project(project.path(), file, source);
    if let Some(config) = config {
        std::fs::write(project.path().join(".fallowrc.json"), config).expect("write config");
    }
    let output = run_fallow_in_root(
        "security",
        project.path(),
        &["--no-cache", "--format", "json", "--quiet"],
    );
    assert_eq!(output.code, 0, "stderr: {}", output.stderr);
    parse_json(&output)["security_findings"]
        .as_array()
        .expect("findings array")
        .clone()
}

/// The stable ids of the findings in `source`, in source order.
fn stable_ids(file: &str, source: &str) -> Vec<String> {
    stable_ids_with_config(file, source, None)
}

fn stable_ids_with_config(file: &str, source: &str, config: Option<&str>) -> Vec<String> {
    findings_with_config(file, source, config)
        .iter()
        .map(|finding| {
            finding["stable_finding_id"]
                .as_str()
                .unwrap_or_else(|| panic!("no stable_finding_id in {finding}"))
                .to_owned()
        })
        .collect()
}

/// Assert that `source` has `expected` findings with distinct base ids, and
/// that an unrelated line insert and the `reformatted` source keep each id.
fn assert_distinct_and_stable(file: &str, source: &str, reformatted: &str, expected: usize) {
    let ids = stable_ids(file, source);
    assert_eq!(ids.len(), expected, "ids: {ids:?}");
    for id in &ids {
        assert!(
            !id.contains('~'),
            "sinks with different text share a base id: {ids:?}"
        );
    }
    let moved = stable_ids(file, &format!("{UNRELATED_LINE}{source}"));
    assert_eq!(moved, ids, "an unrelated line insert changed an id");
    let reformatted = stable_ids(file, reformatted);
    assert_eq!(reformatted, ids, "a reformat changed an id");
}

/// The text after the first closed bracket group of the assigned value is
/// part of the sink.
#[test]
fn stable_id_covers_the_value_after_a_call() {
    let source = "export function a(el, user) {\n  el.innerHTML = esc(a) + 'static'\n}\n\
        export function b(el, user) {\n  el.innerHTML = esc(a) + user\n}\n\
        export function c(el, user) {\n  el.innerHTML = esc(a) || user\n}\n\
        export function d(el, user) {\n  el.innerHTML = esc(a) ? user : 'x'\n}\n";
    let reformatted = "export function a(el, user) {\n  el.innerHTML =\n    esc( a ) +\n    'static';\n}\n\
        export function b(el, user) {\n  el.innerHTML = esc(\n    a,\n  ) + user;\n}\n\
        export function c(el, user) {\n  el.innerHTML = esc(a)\n    || user;\n}\n\
        export function d(el, user) {\n  el.innerHTML = esc(a)\n    ? user\n    : 'x';\n}\n";
    assert_distinct_and_stable("src/index.js", source, reformatted, 4);
}

/// A line that starts with `+`, `-`, `(`, `[`, a backtick, `/`, `in` or
/// `instanceof` continues the expression of the line above.
#[test]
fn stable_id_covers_a_continuation_line() {
    let continuations = [
        "",
        "+ user",
        "- user",
        "(user)",
        "[user]",
        "`x`",
        "/ user",
        "in user",
        "instanceof user",
    ];
    let function = |index: usize, body: String| {
        format!("export function f{index}(el, safe, user) {{\n  el.innerHTML = {body}\n}}\n")
    };
    let source: String = continuations
        .iter()
        .enumerate()
        .map(|(index, tail)| {
            let tail_line = if tail.is_empty() {
                String::new()
            } else {
                format!("\n    {tail}")
            };
            function(index, format!("safe{tail_line}"))
        })
        .collect();
    let reformatted: String = continuations
        .iter()
        .enumerate()
        .map(|(index, tail)| function(index, format!("safe {tail};")))
        .collect();
    assert_distinct_and_stable("src/index.js", &source, &reformatted, continuations.len());
}

/// A regex literal is not a comment.
#[test]
fn stable_id_reads_a_regex_literal_as_a_literal() {
    let source = "export function a(el, s, user) {\n  el.innerHTML = s.replace(/\\/\\//g, user)\n}\n\
        export function b(el, s, other) {\n  el.innerHTML = s.replace(/\\/\\//g, other)\n}\n\
        export function c(el, s, user) {\n  el.innerHTML = s.replace(/\\/*/g, user)\n}\n\
        export function d(el, s, other) {\n  el.innerHTML = s.replace(/\\/*/g, other)\n}\n";
    let reformatted = source
        .replace("g, user)", "g,\n    user,\n  );")
        .replace("g, other)", "g,\n    other,\n  );");
    assert_distinct_and_stable("src/index.js", source, &reformatted, 4);
}

/// The hash covers the full sink, also when it is longer than a few
/// kilobytes.
#[test]
fn stable_id_covers_a_long_sink() {
    let markup = "a".repeat(5000);
    let source = format!(
        "export function a(el, user) {{\n  el.innerHTML = '{markup}' + user\n}}\n\
         export function b(el, other) {{\n  el.innerHTML = '{markup}' + other\n}}\n"
    );
    let reformatted = source.replace("' + ", "' +\n    ");
    assert_distinct_and_stable("src/index.js", &source, &reformatted, 2);
}

/// A leading byte order mark does not change the id of a sink on line 1.
#[test]
fn stable_id_ignores_a_byte_order_mark() {
    let source =
        "import { exec } from 'node:child_process'; export const run = (cmd) => exec(cmd)\n";
    let plain = stable_ids("src/index.js", source);
    assert_eq!(plain.len(), 1);
    let with_bom = stable_ids("src/index.js", &format!("\u{feff}{source}"));
    assert_eq!(with_bom, plain);
    let reformatted =
        "import { exec } from 'node:child_process'\nexport const run = (cmd) =>\n  exec( cmd )\n";
    assert_eq!(stable_ids("src/index.js", reformatted), plain);
    assert_eq!(
        stable_ids("src/index.js", &format!("{UNRELATED_LINE}{source}")),
        plain
    );
}

/// A CRLF checkout gives the same id as an LF checkout, also for a line end
/// inside a string literal.
#[test]
fn stable_id_ignores_crlf_inside_a_string_continuation() {
    let source = "import { exec } from 'node:child_process'\n\nexport function run(name) {\n  exec('git log \\\n' + name)\n}\n";
    let lf = stable_ids("src/index.js", source);
    assert_eq!(lf.len(), 1);
    let crlf = stable_ids("src/index.js", &source.replace('\n', "\r\n"));
    assert_eq!(crlf, lf);
    let reformatted = source.replace("' + name)", "' +\n    name,\n  );");
    assert_eq!(stable_ids("src/index.js", &reformatted), lf);
    assert_eq!(
        stable_ids("src/index.js", &format!("{UNRELATED_LINE}{source}")),
        lf
    );
}

/// A sink in Vue template markup has no exact expression span, so it gets
/// no stable id. A sink in the script block of the same file gets one, and
/// markup below the template sink does not change that id.
#[test]
fn stable_id_is_absent_for_a_template_sink_and_exact_for_a_script_sink() {
    let template = "<template>\n  <div v-html=\"html\"></div>\n</template>\n";
    let script = "<script setup>\nimport { exec } from 'node:child_process'\nconst props = defineProps(['html', 'cmd'])\nexec(props.cmd)\n</script>\n";
    let below = "<template>\n  <div v-html=\"html\"></div>\n  <p>more markup</p>\n</template>\n";
    let ids_for = |source: &str| -> (Vec<serde_json::Value>, Vec<serde_json::Value>) {
        findings_for("src/App.vue", source)
            .into_iter()
            .partition(|finding| finding["category"] == "dangerous-html")
    };
    let (html, exec) = ids_for(&format!("{template}{script}"));
    assert_eq!(html.len(), 1, "template findings: {html:?}");
    assert!(
        html[0].get("stable_finding_id").is_none(),
        "a template sink got a stable id: {}",
        html[0]
    );
    assert_eq!(exec.len(), 1, "script findings: {exec:?}");
    let exec_id = exec[0]["stable_finding_id"].clone();
    assert!(exec_id.is_string(), "the script sink has no stable id");

    let (html, exec) = ids_for(&format!("{below}{script}"));
    assert!(html[0].get("stable_finding_id").is_none());
    assert_eq!(exec[0]["stable_finding_id"], exec_id);
    let (_, exec) = ids_for(&format!(
        "{template}{}",
        script.replace("exec(props.cmd)", "exec(\n  props.cmd,\n)")
    ));
    assert_eq!(exec[0]["stable_finding_id"], exec_id);
}

/// A hard-coded secret hashes the binding name and the detector signal, never
/// the value. A new value of the same provider keeps the id. A known provider
/// key in place of a high-entropy value gets a new id.
#[test]
fn stable_id_of_a_secret_follows_the_provider_family() {
    let secret = |value: &str| {
        let source = format!("export const API_KEY = '{value}'\n");
        let ids = stable_ids_with_config("src/index.js", &source, Some(SECRET_CONFIG));
        assert_eq!(ids.len(), 1, "no secret finding for {value}");
        let moved = stable_ids_with_config(
            "src/index.js",
            &format!("{UNRELATED_LINE}{source}"),
            Some(SECRET_CONFIG),
        );
        assert_eq!(moved, ids);
        ids[0].clone()
    };
    // Built at run time, so the source holds no literal that looks like a live key.
    let stripe = secret(&format!("sk_live_{}", "a".repeat(24)));
    assert_eq!(stripe, secret(&format!("sk_live_{}", "b".repeat(24))));
    assert_ne!(stripe, secret("Zq8vT3nLwP5xR2kM9sB7dF4hJ6gY1cA0"));
}

/// An inline suppression hides a sink, but the sink keeps its ordinal. The
/// next identical sink keeps its `~1` id, so a verdict of the hidden sink
/// never applies to it.
#[test]
fn stable_id_ordinal_counts_a_suppressed_sink() {
    let sink = "export function f(el, html) {\n  el.innerHTML = html\n}\n";
    let source = format!("{sink}{sink}");
    let ids = stable_ids("src/index.js", &source);
    assert_eq!(ids.len(), 2, "ids: {ids:?}");
    assert_eq!(ids[1], format!("{}~1", ids[0]));

    let suppressed = source.replacen(
        "  el.innerHTML",
        "  // fallow-ignore-next-line security-sink\n  el.innerHTML",
        1,
    );
    assert_eq!(
        stable_ids("src/index.js", &suppressed),
        vec![ids[1].clone()]
    );
}

/// The stable id of a client-server leak hashes the names of the env
/// secrets that the server module reads.
#[test]
fn stable_id_of_a_client_server_leak_follows_the_secret_names() {
    let leak_id = |server: &str| {
        let project = tempfile::tempdir().expect("project dir");
        write_project(
            project.path(),
            "src/client.tsx",
            "\"use client\";\nimport { v } from './server';\nexport const C = () => v;\n",
        );
        std::fs::write(project.path().join("src/server.ts"), server).expect("write server");
        let output = run_fallow_in_root(
            "security",
            project.path(),
            &["--no-cache", "--format", "json", "--quiet"],
        );
        assert_eq!(output.code, 0, "stderr: {}", output.stderr);
        let json = parse_json(&output);
        let findings = json["security_findings"].as_array().expect("findings");
        assert_eq!(findings.len(), 1, "findings: {findings:?}");
        findings[0]["stable_finding_id"]
            .as_str()
            .expect("stable id")
            .to_owned()
    };
    let label = leak_id("export const v = process.env.BUILD_LABEL;\n");
    assert_eq!(
        label,
        leak_id("// a comment\nexport const v = process.env.BUILD_LABEL;\n")
    );
    assert_ne!(
        label,
        leak_id("export const v = process.env.DATABASE_PASSWORD;\n")
    );
}
