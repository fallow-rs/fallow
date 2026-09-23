//! Config facts a framework plugin could not use reach
//! `workspace_diagnostics[]`.
//!
//! The Module Federation reader printed its advisory through `tracing` only, so
//! a consumer running `--quiet --format json` never saw that an `exposes` or
//! `remotes` declaration had not been read, and the Nuxt auto-import gate said
//! nothing at all when a config kept its convention entry patterns (issue
//! #2736).
//!
//! These tests drive real runs into each condition on a temp project rather
//! than asserting a constant, because the fact is produced in the plugin stage
//! and has to survive a config re-load, a warm cache and combined mode to reach
//! a consumer at all.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests and benches use unwrap and expect to keep fixture setup concise"
)]

#[path = "common/mod.rs"]
mod common;

use common::{parse_json, run_fallow_raw, run_fallow_raw_with_env};
use serde_json::Value;
use std::path::Path;
use tempfile::TempDir;

const UNREADABLE: &str = "plugin-config-unreadable";
const NOT_MODELED: &str = "plugin-effect-not-modeled";

/// A Module Federation project whose `module-federation.config.ts` holds the
/// declarations the caller asks for.
fn federation_project(config: &str) -> TempDir {
    let dir = TempDir::new().expect("temp project");
    let root = dir.path();
    std::fs::create_dir_all(root.join("src")).expect("src dir");
    std::fs::create_dir_all(root.join("node_modules/@module-federation/enhanced"))
        .expect("installed enabler");
    std::fs::write(
        root.join("node_modules/@module-federation/enhanced/package.json"),
        r#"{"name":"@module-federation/enhanced","version":"0.9.0"}"#,
    )
    .expect("enabler manifest");
    std::fs::write(
        root.join("package.json"),
        r#"{"name":"mf-fx","version":"1.0.0","private":true,"main":"src/index.ts","dependencies":{"@module-federation/enhanced":"^0.9.0"}}"#,
    )
    .expect("package.json");
    std::fs::write(
        root.join("src/index.ts"),
        "export const main = (value: number): number => value + 1;\n",
    )
    .expect("entry");
    std::fs::write(root.join("module-federation.config.ts"), config).expect("federation config");
    dir
}

/// The project root as the run sees it. Canonicalized, because a macOS temp
/// directory is reached through a symlink (`/var` to `/private/var`) and the
/// stored diagnostic path is canonical, so an uncanonicalized root makes the
/// project-relative comparison pass locally and fail on Linux.
fn root_arg(dir: &TempDir) -> String {
    dunce::canonicalize(dir.path())
        .expect("canonical temp root")
        .to_str()
        .expect("temp path is UTF-8")
        .to_owned()
}

fn diagnostics(envelope: &Value) -> &[Value] {
    envelope["workspace_diagnostics"]
        .as_array()
        .map_or(&[] as &[Value], Vec::as_slice)
}

/// Every entry of one kind, in envelope order.
fn of_kind<'a>(envelope: &'a Value, kind: &str) -> Vec<&'a Value> {
    diagnostics(envelope)
        .iter()
        .filter(|entry| entry["kind"] == kind)
        .collect()
}

fn dead_code_json(root: &str, extra: &[&str]) -> common::CommandOutput {
    let mut args = vec!["dead-code", "--root", root, "--format", "json"];
    args.extend_from_slice(extra);
    run_fallow_raw(&args)
}

/// The same run with warnings enabled. The shared harness sets `RUST_LOG=""`,
/// which switches tracing off entirely, so a test that asserts what a human sees
/// on stderr has to ask for the CLI's default level explicitly.
fn dead_code_json_with_warnings(root: &str, extra: &[&str]) -> common::CommandOutput {
    let mut args = vec!["dead-code", "--root", root, "--format", "json"];
    args.extend_from_slice(extra);
    run_fallow_raw_with_env(&args, &[("RUST_LOG", "warn")])
}

/// The headline case: a computed `exposes` is recorded with the plugin, the key,
/// the reason and the config file, and it says the run was degraded.
#[test]
fn an_unreadable_exposes_reaches_the_envelope() {
    let project = federation_project(
        "import { makeExposes } from './build/exposes';\n\
         export default { name: 'host', exposes: makeExposes() };\n",
    );
    let envelope = parse_json(&dead_code_json(&root_arg(&project), &["--quiet"]));
    let entries = of_kind(&envelope, UNREADABLE);
    assert_eq!(entries.len(), 1, "{}", envelope["workspace_diagnostics"]);
    let entry = entries[0];
    assert_eq!(entry["plugin"], "module-federation");
    assert_eq!(entry["key"], "exposes");
    assert_eq!(entry["reason"], "not-object-literal");
    assert_eq!(
        entry["path"], "module-federation.config.ts",
        "the config file is named project-relative: {entry}"
    );
    assert_eq!(
        entry["degrades_analysis"], true,
        "a declaration that did not reach the analysis degrades it: {entry}"
    );
    let message = entry["message"].as_str().expect("a remedy sentence");
    assert!(
        message.contains("dynamicallyLoaded") && !message.contains('\n'),
        "the sentence names the option that covers the gap and stays on one line: {message}"
    );
}

/// Both keys unreadable in ONE config file is two entries and two stderr lines.
/// They share a kind and a path, so a dedupe keyed on either drops the second,
/// and a reader told only about `exposes` would fix half the problem.
#[test]
fn two_unreadable_keys_in_one_config_are_two_entries_and_two_lines() {
    let project = federation_project(
        "import { makeExposes } from './build/exposes';\n\
         export default {\n\
         \x20 name: 'host',\n\
         \x20 exposes: makeExposes(),\n\
         \x20 remotes: { ...runtimeRemotes },\n\
         };\n",
    );
    let output = dead_code_json_with_warnings(&root_arg(&project), &["--quiet"]);
    let envelope = parse_json(&output);
    let keys: Vec<&str> = of_kind(&envelope, UNREADABLE)
        .iter()
        .map(|entry| entry["key"].as_str().expect("key member"))
        .collect();
    assert_eq!(
        keys,
        vec!["exposes", "remotes"],
        "{}",
        envelope["workspace_diagnostics"]
    );
    let printed = output
        .stderr
        .lines()
        .filter(|line| line.contains("Plugin 'module-federation'"))
        .count();
    assert_eq!(
        printed, 2,
        "both keys warn on stderr, and --quiet does not remove them; stderr was:\n{}",
        output.stderr
    );
}

/// The advisory names the plugin whose config file the user must edit, not the
/// reader it shares with four bundlers.
#[test]
fn inline_bundler_options_name_the_bundler_plugin() {
    let dir = TempDir::new().expect("temp project");
    let root = dir.path();
    std::fs::create_dir_all(root.join("src")).expect("src dir");
    std::fs::create_dir_all(root.join("node_modules/webpack")).expect("installed webpack");
    std::fs::write(
        root.join("node_modules/webpack/package.json"),
        r#"{"name":"webpack","version":"5.99.0"}"#,
    )
    .expect("webpack manifest");
    std::fs::write(
        root.join("package.json"),
        r#"{"name":"wp-fx","version":"1.0.0","private":true,"main":"src/index.ts","devDependencies":{"webpack":"^5.99.0"}}"#,
    )
    .expect("package.json");
    std::fs::write(
        root.join("src/index.ts"),
        "export const main = (value: number): number => value + 1;\n",
    )
    .expect("entry");
    std::fs::write(
        root.join("webpack.config.js"),
        "module.exports = {\n\
         \x20 plugins: [new ModuleFederationPlugin({ remotes: buildRemotes() })],\n\
         };\n",
    )
    .expect("webpack config");

    let canonical = dunce::canonicalize(root).expect("canonical temp root");
    let envelope = parse_json(&dead_code_json(
        canonical.to_str().expect("temp path is UTF-8"),
        &["--quiet"],
    ));
    let entries = of_kind(&envelope, UNREADABLE);
    assert_eq!(entries.len(), 1, "{}", envelope["workspace_diagnostics"]);
    assert_eq!(entries[0]["plugin"], "webpack");
    assert_eq!(entries[0]["key"], "remotes");
    assert_eq!(entries[0]["path"], "webpack.config.js");
}

/// A Module Federation project fallow reads in full carries no advisory, so a
/// consumer warning on the kind warns about something.
#[test]
fn a_readable_federation_config_carries_no_advisory() {
    let project = federation_project(
        "export default {\n\
         \x20 name: 'host',\n\
         \x20 exposes: { './Main': './src/index.ts' },\n\
         \x20 remotes: { checkout: 'checkout@https://example.test/remoteEntry.js' },\n\
         };\n",
    );
    let output = dead_code_json_with_warnings(&root_arg(&project), &[]);
    let envelope = parse_json(&output);
    assert!(
        of_kind(&envelope, UNREADABLE).is_empty() && of_kind(&envelope, NOT_MODELED).is_empty(),
        "{}",
        envelope["workspace_diagnostics"]
    );
    assert!(
        !output.stderr.contains("Plugin 'module-federation'"),
        "stderr was:\n{}",
        output.stderr
    );
}

/// A shape the reader learns to read stops reporting: the array form of
/// `exposes` carries no entry and no stderr line, while the array form of
/// `remotes` in the same file still carries both (issue #2698).
#[test]
fn the_array_form_of_exposes_no_longer_reports() {
    let project = federation_project(
        "export default {\n\
         \x20 name: 'host',\n\
         \x20 exposes: ['./src/index.ts'],\n\
         \x20 remotes: ['checkout@https://example.test/remoteEntry.js'],\n\
         };\n",
    );
    let output = dead_code_json_with_warnings(&root_arg(&project), &[]);
    let envelope = parse_json(&output);
    let reported: Vec<(&str, &str)> = of_kind(&envelope, UNREADABLE)
        .iter()
        .map(|entry| {
            (
                entry["key"].as_str().expect("a key"),
                entry["reason"].as_str().expect("a reason"),
            )
        })
        .collect();
    assert_eq!(
        reported,
        vec![("remotes", "array-form")],
        "only the unread key reports: {}",
        envelope["workspace_diagnostics"]
    );
    assert_eq!(
        output
            .stderr
            .lines()
            .filter(|line| line.contains("Plugin 'module-federation'"))
            .count(),
        1,
        "stderr was:\n{}",
        output.stderr
    );
}

/// The entry is produced in the plugin stage, which is not cached, so a warm
/// run carries it exactly like the cold one that wrote the cache. A consumer
/// reading only the second run of a CI job would otherwise see nothing.
#[test]
fn the_advisory_survives_a_warm_cache_and_the_quiet_flag() {
    let project = federation_project(
        "export default { name: 'host', remotes: ['checkout@https://example.test/re.js'] };\n",
    );
    let root = root_arg(&project);
    let cold = parse_json(&dead_code_json(&root, &[]));
    let cold_entries = of_kind(&cold, UNREADABLE);
    assert_eq!(cold_entries.len(), 1, "{}", cold["workspace_diagnostics"]);
    assert_eq!(cold_entries[0]["reason"], "array-form");

    let warm = parse_json(&dead_code_json(&root, &["--quiet"]));
    let warm_entries = of_kind(&warm, UNREADABLE);
    assert_eq!(
        warm_entries.len(),
        1,
        "the second run carries it too: {}",
        warm["workspace_diagnostics"]
    );
    assert_eq!(warm_entries[0], cold_entries[0]);
}

/// The combined run re-loads config per analysis, and its root-level array is
/// the one every CI consumer reads, so the entry has to survive both the
/// re-stash and the analyze pass's own clear.
#[test]
fn the_combined_root_carries_the_advisory() {
    let project = federation_project(
        "export default { name: 'host', exposes: { './Main': computeTarget() } };\n",
    );
    let envelope = parse_json(&run_fallow_raw(&[
        "--root",
        &root_arg(&project),
        "--format",
        "json",
        "--quiet",
    ]));
    let entries = of_kind(&envelope, UNREADABLE);
    assert_eq!(
        entries.len(),
        1,
        "the combined root carries one copy: {}",
        envelope["workspace_diagnostics"]
    );
    assert_eq!(entries[0]["reason"], "unreadable-entries");
    assert_eq!(entries[0]["key"], "exposes");
}

/// A Nuxt project whose `nuxt.config` customizes one surface: the run keeps that
/// surface's convention entry patterns, so `autoImports` did not reach it, and
/// only the entry says so.
fn nuxt_project(config: &str) -> TempDir {
    let dir = TempDir::new().expect("temp project");
    let root = dir.path();
    std::fs::create_dir_all(root.join("app/components")).expect("components dir");
    std::fs::create_dir_all(root.join("node_modules/nuxt")).expect("installed nuxt");
    std::fs::write(
        root.join("node_modules/nuxt/package.json"),
        r#"{"name":"nuxt","version":"3.17.0"}"#,
    )
    .expect("nuxt manifest");
    std::fs::write(
        root.join("package.json"),
        r#"{"name":"nuxt-fx","version":"1.0.0","private":true,"dependencies":{"nuxt":"^3.17.0"}}"#,
    )
    .expect("package.json");
    std::fs::write(
        root.join("app/app.vue"),
        "<template><div>app</div></template>\n",
    )
    .expect("app entry");
    std::fs::write(
        root.join("app/components/Widget.vue"),
        "<template><span>widget</span></template>\n",
    )
    .expect("component");
    std::fs::write(root.join("nuxt.config.ts"), config).expect("nuxt config");
    std::fs::write(root.join(".fallowrc.json"), r#"{"autoImports":true}"#).expect("fallow config");
    dir
}

/// A surface that kept its patterns is reported as not modeled, quietly: the
/// config loaded and nothing the run could measure was lost, so it must not warn
/// on every run forever.
#[test]
fn a_retained_nuxt_surface_is_reported_without_a_stderr_line() {
    let project = nuxt_project("export default { components: { dirs: ['~/ui'] } };\n");
    let output = dead_code_json_with_warnings(&root_arg(&project), &[]);
    let envelope = parse_json(&output);
    let entries = of_kind(&envelope, NOT_MODELED);
    assert_eq!(entries.len(), 1, "{}", envelope["workspace_diagnostics"]);
    let entry = entries[0];
    assert_eq!(entry["plugin"], "nuxt");
    assert_eq!(entry["key"], "components");
    assert_eq!(entry["reason"], "key-effect-not-modeled");
    assert_eq!(entry["path"], "nuxt.config.ts");
    assert!(
        entry.get("degrades_analysis").is_none(),
        "nothing measurable was lost, so the run is not degraded: {entry}"
    );
    assert!(
        !output.stderr.contains("Plugin 'nuxt'"),
        "the quiet kind warns on no channel; stderr was:\n{}",
        output.stderr
    );
}

/// A top-level property this reader cannot resolve stands both surfaces down,
/// which is the silent case: no `components:` or `imports:` key is present at
/// all, so the entry has to name the property rather than a surface the user
/// never wrote.
#[test]
fn an_unreadable_top_level_property_reports_both_surfaces() {
    let project = nuxt_project("export default { ...baseConfig, devtools: {} };\n");
    let envelope = parse_json(&dead_code_json(&root_arg(&project), &["--quiet"]));
    let reasons: Vec<(&str, &str)> = of_kind(&envelope, NOT_MODELED)
        .iter()
        .map(|entry| {
            (
                entry["key"].as_str().expect("key member"),
                entry["reason"].as_str().expect("reason member"),
            )
        })
        .collect();
    assert_eq!(
        reasons,
        vec![
            ("components", "config-property-unreadable"),
            ("imports", "config-property-unreadable"),
        ],
        "{}",
        envelope["workspace_diagnostics"]
    );
}

/// A `nuxt.config` fallow models fully drops the patterns it was asked to drop
/// and reports nothing, so the advisory fires only where a finding was actually
/// suppressed.
#[test]
fn a_modeled_nuxt_config_carries_no_advisory() {
    let project = nuxt_project("export default { devtools: { enabled: true } };\n");
    let envelope = parse_json(&dead_code_json(&root_arg(&project), &["--quiet"]));
    assert!(
        of_kind(&envelope, NOT_MODELED).is_empty(),
        "{}",
        envelope["workspace_diagnostics"]
    );
}

/// A `hooks` object or an environment override that does not touch a surface
/// keeps the static proof of the top-level keys: `components: false` still drops
/// the component patterns, and a nested `routeRules` path is not a surface key.
/// The unreferenced component reports, and no advisory names a key fallow models.
#[test]
fn an_unrelated_hook_or_override_keeps_the_surface_modeled() {
    for config in [
        "export default { components: false, hooks: { 'pages:extend'() {} } };\n",
        "export default { components: false, $production: { routeRules: {} } };\n",
        "export default { routeRules: { '/docs/components': { prerender: true } }, hooks: { 'pages:extend'() {} } };\n",
    ] {
        let project = nuxt_project(config);
        let envelope = parse_json(&dead_code_json(&root_arg(&project), &["--quiet"]));
        assert!(
            of_kind(&envelope, NOT_MODELED).is_empty(),
            "{config}: {}",
            envelope["workspace_diagnostics"]
        );
        let unused: Vec<&str> = envelope["unused_files"]
            .as_array()
            .map_or(&[] as &[Value], Vec::as_slice)
            .iter()
            .filter_map(|entry| entry["path"].as_str())
            .collect();
        assert!(
            unused.contains(&"app/components/Widget.vue"),
            "{config}: the unreferenced component must report, got {unused:?}"
        );
    }
}

/// A hook that changes the component scan keeps the component patterns, in the
/// nested hook form and inside an environment override. Here the hook drops the
/// path prefix, so `<Btn/>` names `base/Btn.vue` and only the retained entry
/// pattern keeps it alive; the advisory says why.
#[test]
fn a_component_hook_keeps_the_patterns_in_every_shape() {
    let hook = "(dirs) { dirs.length = 0; dirs.push({ path: '~/components', pathPrefix: false }) }";
    for config in [
        format!("export default {{ hooks: {{ components: {{ dirs{hook} }} }} }};\n"),
        format!("export default {{ $production: {{ hooks: {{ 'components:dirs'{hook} }} }} }};\n"),
    ] {
        let project = nuxt_project(&config);
        let root = project.path();
        std::fs::create_dir_all(root.join("app/components/base")).expect("base dir");
        std::fs::create_dir_all(root.join("app/pages")).expect("pages dir");
        std::fs::write(
            root.join("app/components/base/Btn.vue"),
            "<template><button>btn</button></template>\n",
        )
        .expect("component");
        std::fs::write(
            root.join("app/pages/index.vue"),
            "<template><Btn /></template>\n",
        )
        .expect("page");
        let envelope = parse_json(&dead_code_json(&root_arg(&project), &["--quiet"]));
        let unused: Vec<&str> = envelope["unused_files"]
            .as_array()
            .map_or(&[] as &[Value], Vec::as_slice)
            .iter()
            .filter_map(|entry| entry["path"].as_str())
            .collect();
        assert!(
            !unused.contains(&"app/components/base/Btn.vue"),
            "{config}: the hook-registered component must not report, got {unused:?}"
        );
        let keys: Vec<&str> = of_kind(&envelope, NOT_MODELED)
            .iter()
            .filter_map(|entry| entry["key"].as_str())
            .collect();
        assert_eq!(keys, vec!["components"], "{config}");
    }
}

/// `fallow check` is the other command a CI job runs, and it reaches the array
/// through the same generic route, so no per-command wiring is needed.
#[test]
fn the_check_envelope_carries_the_advisory_too() {
    let project = federation_project("export default { name: 'host', exposes: buildExposes() };\n");
    let envelope = parse_json(&run_fallow_raw(&[
        "check",
        "--root",
        &root_arg(&project),
        "--format",
        "json",
        "--quiet",
    ]));
    assert_eq!(
        of_kind(&envelope, UNREADABLE).len(),
        1,
        "{}",
        envelope["workspace_diagnostics"]
    );
}

/// The advisory is advisory: it withholds nothing and fails nothing, so a run
/// that records it still reports its findings and never exits 2.
#[test]
fn the_advisory_never_fails_the_run() {
    let project = federation_project("export default { name: 'host', exposes: buildExposes() };\n");
    let root = root_arg(&project);
    let output = dead_code_json(&root, &["--quiet"]);
    assert_ne!(
        output.code, 2,
        "a recorded advisory is not an execution error; stderr was:\n{}",
        output.stderr
    );
    let envelope = parse_json(&output);
    assert_eq!(of_kind(&envelope, UNREADABLE).len(), 1);
    assert!(Path::new(&root).is_dir(), "the fixture outlives the run");
}
