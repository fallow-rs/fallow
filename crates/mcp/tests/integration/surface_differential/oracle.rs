//! The differential checks over the generated projects.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Debug;
use std::path::Path;

use serde_json::{Value, json};

use super::facts::{self, Finding};
use super::project::{Checkout, Project, materialize};
use super::surfaces::{self, Mcp, api};
use super::templates;

/// The project seeds of pull request CI.
const PR_SEEDS: [u64; 3] = [1, 2, 3];

/// The file that the editor session opens.
const OPENED: &str = "packages/app/src/main.ts";

/// The trace fields that do not give a dependency credit.
const DEPENDENCY_VERDICT_FIELDS: [&str; 7] = [
    "package_name",
    "is_used",
    "import_count",
    "unused_in",
    "imported_by",
    "type_only_imported_by",
    "used_in_scripts",
];

/// The disagreements of one project, one line each.
#[derive(Default)]
struct Failures(Vec<String>);

impl Failures {
    fn same<T: PartialEq + Debug>(&mut self, what: &str, reference: &T, other: &T) {
        if reference != other {
            self.0.push(format!(
                "{what}\n  reference: {reference:?}\n  other:     {other:?}"
            ));
        }
    }

    fn check(&mut self, holds: bool, what: impl FnOnce() -> String) {
        if !holds {
            self.0.push(what());
        }
    }

    fn assert_none(self, seed: u64, check: &str) {
        assert!(
            self.0.is_empty(),
            "{check}: the surfaces disagree on project seed {seed} ({} differences).\n\
             Run the same project in the sweep with FALLOW_SURFACE_SEED_START={seed} FALLOW_SURFACE_SEEDS=1.\n\n{}",
            self.0.len(),
            self.0.join("\n\n")
        );
    }
}

struct Run {
    project: Project,
    checkout: Checkout,
    mcp: Mcp,
}

impl Run {
    fn new(seed: u64) -> Self {
        let project = templates::generate(seed);
        let checkout = materialize(&project);
        Self {
            project,
            checkout,
            mcp: Mcp::start(&std::env::temp_dir(), &[]),
        }
    }

    fn root(&self) -> &Path {
        &self.checkout.root
    }

    fn report(&self) -> BTreeSet<Finding> {
        facts::dead_code(&surfaces::cli(
            self.root(),
            &["dead-code", "--no-cache"],
            &[],
        ))
    }
}

/// The CLI, MCP, `fallow_api`, the LSP and the Node bindings report the same
/// dead-code findings, with the same paths, positions and ids.
fn dead_code_agrees(seed: u64) {
    let mut run = Run::new(seed);
    let root = run.root().to_path_buf();
    let mut failures = Failures::default();
    let reference = run.report();
    failures.check(!reference.is_empty(), || {
        "the CLI reports no finding".into()
    });

    let mcp = facts::dead_code(&run.mcp.call("analyze", &root, &json!({ "no_cache": true })));
    failures.same("MCP `analyze` findings", &reference, &mcp);
    let scratch = tempfile::tempdir().expect("scratch dir");
    let baseline = empty_baseline(scratch.path(), "dead-code");
    let mcp_cli_route = facts::dead_code(&run.mcp.call(
        "analyze",
        &root,
        &json!({ "no_cache": true, "baseline": baseline }),
    ));
    failures.same(
        "MCP `analyze` with `baseline` (CLI route) findings",
        &reference,
        &mcp_cli_route,
    );
    failures.same(
        "fallow_api dead-code findings (the Node binding path)",
        &reference,
        &facts::dead_code(&api::dead_code(&root)),
    );
    if let Some(addon) = surfaces::node_addon() {
        let node = surfaces::node(
            &addon,
            "detectDeadCode",
            &json!({ "root": root, "noCache": true }),
            &std::env::temp_dir(),
            &[],
        );
        failures.same(
            "Node `detectDeadCode` findings",
            &reference,
            &facts::dead_code(&node),
        );
    }

    let editor = surfaces::lsp(&root, OPENED, &root, &[]);
    failures.same(
        "LSP diagnostics (finding id, path, line, column)",
        &facts::expected_diagnostics(&reference),
        &editor,
    );
    failures.assert_none(seed, "dead-code facts");
}

/// An empty baseline of `command`, saved on an empty project. A run with it
/// keeps every finding, and the MCP server takes the CLI route for it.
fn empty_baseline(dir: &Path, command: &str) -> String {
    let empty = dir.join("empty");
    std::fs::create_dir_all(&empty).expect("empty project");
    std::fs::write(empty.join("package.json"), "{\"name\":\"empty\"}\n").expect("manifest");
    let baseline = dir.join(format!("{command}-baseline.json"));
    let baseline_arg = baseline.to_str().expect("UTF-8 path").to_owned();
    surfaces::cli(
        &empty,
        &[command, "--no-cache", "--save-baseline", &baseline_arg],
        &[],
    );
    assert!(baseline.is_file(), "the CLI saves the {command} baseline");
    baseline_arg
}

/// The clone groups and families, in report order, agree on every surface
/// and on every scope that covers each file: no scope, `--changed-since`
/// the base commit, an empty baseline, and `audit` against the base commit.
fn duplication_agrees(seed: u64) {
    let mut run = Run::new(seed);
    let root = run.root().to_path_buf();
    let base = run.checkout.base.clone();
    let scratch = tempfile::tempdir().expect("scratch dir");
    let baseline = empty_baseline(scratch.path(), "dupes");
    let mut failures = Failures::default();

    let reference = facts::duplication(&surfaces::cli(&root, &["dupes", "--no-cache"], &[]));
    failures.check(reference.families.len() >= 2, || {
        format!("the CLI reports fewer than two clone families: {reference:?}")
    });

    let mut candidates: Vec<(&str, Value)> = vec![
        (
            "CLI `dupes --changed-since <base>`",
            surfaces::cli(
                &root,
                &["dupes", "--no-cache", "--changed-since", &base],
                &[],
            ),
        ),
        (
            "CLI `dupes --baseline <empty>`",
            surfaces::cli(
                &root,
                &["dupes", "--no-cache", "--baseline", &baseline],
                &[],
            ),
        ),
        (
            "CLI `audit --base <base>` duplication",
            surfaces::cli(&root, &["audit", "--no-cache", "--base", &base], &[])["duplication"]
                .clone(),
        ),
        (
            "MCP `find_dupes`",
            run.mcp
                .call("find_dupes", &root, &json!({ "no_cache": true })),
        ),
        (
            "MCP `find_dupes` with `changed_since`",
            run.mcp.call(
                "find_dupes",
                &root,
                &json!({ "no_cache": true, "changed_since": base }),
            ),
        ),
        (
            "MCP `find_dupes` with `baseline` (CLI route)",
            run.mcp.call(
                "find_dupes",
                &root,
                &json!({ "no_cache": true, "baseline": baseline }),
            ),
        ),
        (
            "MCP `audit` duplication",
            run.mcp
                .call("audit", &root, &json!({ "no_cache": true, "base": base }))["duplication"]
                .clone(),
        ),
        (
            "fallow_api duplication (the Node binding path)",
            api::duplication(&root, None),
        ),
        (
            "fallow_api duplication with `changed_since`",
            api::duplication(&root, Some(&base)),
        ),
    ];
    if let Some(addon) = surfaces::node_addon() {
        candidates.push((
            "Node `detectDuplication`",
            surfaces::node(
                &addon,
                "detectDuplication",
                &json!({ "root": root, "noCache": true }),
                &std::env::temp_dir(),
                &[],
            ),
        ));
        candidates.push((
            "Node `detectDuplication` with `changedSince`",
            surfaces::node(
                &addon,
                "detectDuplication",
                &json!({ "root": root, "noCache": true, "changedSince": base }),
                &std::env::temp_dir(),
                &[],
            ),
        ));
    }
    for (surface, report) in &candidates {
        failures.same(
            &format!("{surface} clone groups and families"),
            &reference,
            &facts::duplication(report),
        );
    }
    failures.assert_none(seed, "duplication facts");
}

/// `trace` agrees with the report, and the trace answers agree on the CLI,
/// MCP and `fallow_api`.
fn traces_agree(seed: u64) {
    let mut run = Run::new(seed);
    let root = run.root().to_path_buf();
    let report = run.report();
    let mut failures = Failures::default();
    // The template declares both tools only where a sibling configures them,
    // so the report must flag them, or the trace check proves nothing.
    for (package, manifest) in [
        ("jscpd", "packages/app/package.json"),
        ("madge", "package.json"),
    ] {
        failures.check(
            facts::flagged_manifests(&report, package).contains(manifest),
            || format!("the report does not flag `{package}` in {manifest}"),
        );
    }

    for package in run.project.dependency_names() {
        let cli = surfaces::cli(
            &root,
            &["dead-code", "--no-cache", "--trace-dependency", &package],
            &[],
        );
        let mcp = run.mcp.call(
            "trace_dependency",
            &root,
            &json!({ "package_name": package }),
        );
        let api = api::trace_dependency(&root, &package);
        let what = format!("`{package}` dependency trace");
        failures.same(
            &format!("{what} on MCP"),
            &facts::trace(&cli),
            &facts::trace(&mcp),
        );
        failures.same(
            &format!("{what} on fallow_api"),
            &facts::trace(&cli),
            &facts::trace(&api),
        );
        check_dependency_trace(
            &mut failures,
            &package,
            &cli,
            &facts::flagged_manifests(&report, &package),
            &run.project.manifests_of(&package).into_iter().collect(),
        );
    }

    let mut unused: Vec<(String, String)> = report
        .iter()
        .filter(|finding| matches!(finding.category.as_str(), "unused_exports" | "unused_types"))
        .map(|finding| (finding.path.clone(), finding.subject.clone()))
        .collect();
    unused.sort();
    failures.check(!unused.is_empty(), || {
        "the report has no unused export".into()
    });
    for (file, name) in &unused {
        let answer = trace_export_everywhere(&mut failures, &mut run.mcp, &root, file, name);
        failures.check(
            answer["is_used"] == false
                && empty(&answer["direct_references"])
                && empty(&answer["re_export_chains"]),
            || format!("the report calls `{file}:{name}` unused, but its trace is {answer}"),
        );
    }
    for (file, name) in &run.project.used_exports {
        let answer = trace_export_everywhere(&mut failures, &mut run.mcp, &root, file, name);
        failures.check(
            answer["is_used"] == true && !empty(&answer["direct_references"]),
            || format!("`{file}:{name}` has an importer, but its trace is {answer}"),
        );
        failures.check(
            !unused
                .iter()
                .any(|(path, export)| path == file && export == name),
            || format!("the report calls the imported `{file}:{name}` unused"),
        );
    }
    failures.assert_none(seed, "trace facts");
}

fn trace_export_everywhere(
    failures: &mut Failures,
    mcp: &mut Mcp,
    root: &Path,
    file: &str,
    name: &str,
) -> Value {
    let target = format!("{file}:{name}");
    let cli = surfaces::cli(root, &["dead-code", "--no-cache", "--trace", &target], &[]);
    let from_mcp = mcp.call(
        "trace_export",
        root,
        &json!({ "file": file, "export_name": name }),
    );
    failures.same(
        &format!("`{target}` trace on MCP"),
        &facts::trace(&cli),
        &facts::trace(&from_mcp),
    );
    failures.same(
        &format!("`{target}` trace on fallow_api"),
        &facts::trace(&cli),
        &facts::trace(&api::trace_export(root, file, name)),
    );
    cli
}

/// A dependency that the report flags in each manifest that declares it has
/// no importer and no credit. A dependency that the report does not flag is
/// used. `unused_in` names the flagged manifests.
fn check_dependency_trace(
    failures: &mut Failures,
    package: &str,
    trace: &Value,
    flagged: &BTreeSet<String>,
    declared: &BTreeSet<String>,
) {
    if flagged == declared {
        failures.check(
            trace["is_used"] == false
                && trace["used_in_scripts"] != true
                && empty(&trace["imported_by"])
                && empty(&trace["type_only_imported_by"]),
            || format!("the report flags `{package}` in {flagged:?}, but its trace is {trace}"),
        );
        let credits: BTreeMap<&String, &Value> = trace
            .as_object()
            .expect("trace object")
            .iter()
            .filter(|(key, value)| {
                !DEPENDENCY_VERDICT_FIELDS.contains(&key.as_str())
                    && !key.starts_with('_')
                    && key.as_str() != "kind"
                    && !empty(value)
            })
            .collect();
        failures.check(credits.is_empty(), || {
            format!("the report flags `{package}` in {flagged:?}, but the trace gives credits {credits:?}")
        });
    }
    if flagged.is_empty() {
        failures.check(trace["is_used"] == true, || {
            format!("the report does not flag `{package}`, but its trace is {trace}")
        });
    }
    if let Some(unused_in) = trace.get("unused_in").and_then(Value::as_array) {
        let listed: BTreeSet<String> = unused_in
            .iter()
            .filter_map(|path| path.as_str().map(str::to_owned))
            .collect();
        failures.same(
            &format!("`{package}` trace `unused_in` against the report"),
            flagged,
            &listed,
        );
    }
}

/// `null`, `false`, `0`, an empty string, array or object.
fn empty(value: &Value) -> bool {
    match value {
        Value::Null => true,
        Value::Bool(flag) => !flag,
        Value::Number(number) => number.as_f64() == Some(0.0),
        Value::String(text) => text.is_empty(),
        Value::Array(items) => items.is_empty(),
        Value::Object(fields) => fields.is_empty(),
    }
}

#[test]
fn dead_code_facts_agree_on_every_surface() {
    for seed in PR_SEEDS {
        dead_code_agrees(seed);
    }
}

#[test]
fn clone_families_agree_on_every_surface_and_scope() {
    for seed in PR_SEEDS {
        duplication_agrees(seed);
    }
}

#[test]
fn traces_agree_with_the_report_on_every_surface() {
    for seed in PR_SEEDS {
        traces_agree(seed);
    }
}

/// The daily `Oracle Seeds` job: `FALLOW_SURFACE_SEEDS` project seeds from
/// `FALLOW_SURFACE_SEED_START`.
#[test]
#[ignore = "seed sweep for the daily Oracle Seeds workflow"]
fn seed_sweep_agrees_on_every_surface() {
    let number = |name: &str, default: u64| {
        std::env::var(name)
            .ok()
            .map_or(default, |value| value.parse().expect("a whole number"))
    };
    let start = number("FALLOW_SURFACE_SEED_START", 1);
    let count = number("FALLOW_SURFACE_SEEDS", 20);
    for seed in start..start + count {
        dead_code_agrees(seed);
        duplication_agrees(seed);
        traces_agree(seed);
    }
}
