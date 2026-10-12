//! Search visibility: can a whole-word text search for an export name find
//! each consumer that the module graph resolves?

use std::fs;
use std::path::Path;

use crate::common::{commit_all, git, parse_json, run_fallow_in_root, run_fallow_raw};

/// A project where each consumer reaches `formatDate` (and the default
/// export `Widget`) in a different way.
fn write_search_probe(root: &Path) {
    fs::create_dir_all(root.join("src")).expect("create source directory");
    let files: &[(&str, &str)] = &[
        (
            "package.json",
            r#"{"name":"search-probe","type":"module","main":"src/index.ts"}"#,
        ),
        (
            "src/index.ts",
            "import './direct';\nimport './alias';\nimport './via-barrel';\nimport './ns';\nimport './default-consumer';\nimport './named-default';\n",
        ),
        (
            "src/lib.ts",
            "export const formatDate = (d: Date): string => d.toISOString();\n\nexport default function Widget(): string {\n  return 'w';\n}\n",
        ),
        (
            "src/direct.ts",
            "import { formatDate } from './lib';\nconsole.log(formatDate(new Date()));\n",
        ),
        (
            "src/alias.ts",
            "import { formatDate as fmt } from './lib';\nconsole.log(fmt(new Date()));\n",
        ),
        (
            "src/barrel.ts",
            "export { formatDate as fd } from './lib';\n",
        ),
        (
            "src/via-barrel.ts",
            "// uses the barrel name only\n\n\nimport { fd } from './barrel';\nconsole.log(fd(new Date()));\n",
        ),
        (
            "src/ns.ts",
            "import * as lib from './lib';\nconsole.log(Object.keys(lib));\n",
        ),
        (
            "src/default-consumer.ts",
            "import Thing from './lib';\nconsole.log(Thing());\n",
        ),
        (
            "src/named-default.ts",
            "import Widget from './lib';\nconsole.log(Widget());\n",
        ),
    ];
    for (path, content) in files {
        fs::write(root.join(path), content).expect("write probe file");
    }
}

fn reference<'a>(references: &'a serde_json::Value, from_file: &str) -> &'a serde_json::Value {
    references
        .as_array()
        .expect("references array")
        .iter()
        .find(|reference| reference["from_file"] == from_file)
        .unwrap_or_else(|| panic!("no reference from {from_file}: {references}"))
}

fn trace(root: &Path, target: &str) -> serde_json::Value {
    parse_json(&run_fallow_in_root(
        "dead-code",
        root,
        &[
            "--trace",
            target,
            "--format",
            "json",
            "--quiet",
            "--no-cache",
        ],
    ))
}

#[test]
fn trace_export_reports_search_visibility_per_reference() {
    let dir = tempfile::tempdir().expect("temporary project");
    let root = dir.path();
    write_search_probe(root);

    let trace = trace(root, "src/lib.ts:formatDate");
    let references = &trace["direct_references"];

    let direct = reference(references, "src/direct.ts");
    assert_eq!(direct["search_visibility"], "found", "{direct}");
    assert_eq!(direct["line"], 1, "{direct}");
    assert!(direct.get("reason").is_none(), "{direct}");
    assert!(direct.get("search_as").is_none(), "{direct}");

    let alias = reference(references, "src/alias.ts");
    assert_eq!(alias["search_visibility"], "renamed", "{alias}");
    assert_eq!(alias["reason"], "import-alias", "{alias}");
    assert_eq!(alias["search_as"], "fmt", "{alias}");

    let via_barrel = reference(references, "src/via-barrel.ts");
    assert_eq!(via_barrel["search_visibility"], "missed", "{via_barrel}");
    assert_eq!(via_barrel["reason"], "re-export-alias", "{via_barrel}");
    assert_eq!(via_barrel["search_as"], "fd", "{via_barrel}");
    assert_eq!(via_barrel["line"], 4, "{via_barrel}");

    let namespace = reference(references, "src/ns.ts");
    assert_eq!(namespace["search_visibility"], "missed", "{namespace}");
    assert_eq!(namespace["reason"], "namespace-use", "{namespace}");
    assert_eq!(namespace["search_as"], "lib", "{namespace}");

    let human = run_fallow_in_root(
        "dead-code",
        root,
        &["--trace", "src/lib.ts:formatDate", "--quiet", "--no-cache"],
    );
    let text = format!("{}{}", human.stdout, human.stderr);
    assert!(
        text.contains("src/alias.ts (named import) [used under another name, search fmt]")
            && text.contains("src/ns.ts (namespace import) [name search misses it, search lib]")
            && text.contains("src/direct.ts (named import)\n"),
        "human trace shows the search hint: {text}"
    );
}

#[test]
fn trace_default_export_searches_for_the_declaration_name() {
    let dir = tempfile::tempdir().expect("temporary project");
    let root = dir.path();
    write_search_probe(root);

    let trace = trace(root, "src/lib.ts:default");
    let references = &trace["direct_references"];

    let renamed = reference(references, "src/default-consumer.ts");
    assert_eq!(renamed["search_visibility"], "missed", "{renamed}");
    assert_eq!(renamed["reason"], "default-import", "{renamed}");
    assert_eq!(renamed["search_as"], "Thing", "{renamed}");

    let same_name = reference(references, "src/named-default.ts");
    assert_eq!(same_name["search_visibility"], "found", "{same_name}");
}

#[test]
fn class_member_trace_checks_owner_references_by_owner_name() {
    let dir = tempfile::tempdir().expect("temporary project");
    let root = dir.path();
    fs::create_dir_all(root.join("src")).expect("create source directory");
    let files: &[(&str, &str)] = &[
        (
            "package.json",
            r#"{"name":"member-probe","type":"module","main":"src/index.ts"}"#,
        ),
        (
            "src/index.ts",
            "import './direct';\nimport './alias';\nimport './via-barrel';\n",
        ),
        (
            "src/service.ts",
            "export class Service {\n  run(): number {\n    return 1;\n  }\n}\n",
        ),
        (
            "src/direct.ts",
            "import { Service } from './service';\nconsole.log(new Service().run());\n",
        ),
        (
            "src/alias.ts",
            "import { Service as Svc } from './service';\nconsole.log(new Svc().run());\n",
        ),
        (
            "src/barrel.ts",
            "export { Service as Worker } from './service';\n",
        ),
        (
            "src/via-barrel.ts",
            "import { Worker } from './barrel';\nconsole.log(new Worker().run());\n",
        ),
    ];
    for (path, content) in files {
        fs::write(root.join(path), content).expect("write probe file");
    }

    let trace = trace(root, "src/service.ts:run");
    assert_eq!(trace["owner_export"], "Service", "{trace}");
    let references = &trace["owner_direct_references"];

    let direct = reference(references, "src/direct.ts");
    assert_eq!(direct["search_visibility"], "found", "{direct}");
    assert_eq!(direct["line"], 1, "{direct}");

    let alias = reference(references, "src/alias.ts");
    assert_eq!(alias["search_visibility"], "renamed", "{alias}");
    assert_eq!(alias["reason"], "import-alias", "{alias}");
    assert_eq!(alias["search_as"], "Svc", "{alias}");

    let via_barrel = reference(references, "src/via-barrel.ts");
    assert_eq!(via_barrel["search_visibility"], "missed", "{via_barrel}");
    assert_eq!(via_barrel["reason"], "re-export-alias", "{via_barrel}");
    assert_eq!(via_barrel["search_as"], "Worker", "{via_barrel}");
}

#[test]
fn impact_closure_gaps_report_search_visibility() {
    let dir = tempfile::tempdir().expect("temporary project");
    let root = dir.path();
    write_search_probe(root);

    let output = parse_json(&run_fallow_in_root(
        "dead-code",
        root,
        &[
            "--impact-closure",
            "src/lib.ts",
            "--format",
            "json",
            "--quiet",
            "--no-cache",
        ],
    ));
    let gaps = output["coordination_gap"]
        .as_array()
        .unwrap_or_else(|| panic!("coordination_gap array: {output}"));
    let gap = |consumer: &str| {
        gaps.iter()
            .find(|gap| gap["consumer_file"] == consumer)
            .unwrap_or_else(|| panic!("no gap for {consumer}: {output}"))
    };

    assert_eq!(gap("src/direct.ts")["search_visibility"], "found");
    assert_eq!(gap("src/alias.ts")["search_visibility"], "renamed");
    assert_eq!(gap("src/alias.ts")["search_as"], "fmt");
    let default_gap = gap("src/default-consumer.ts");
    assert_eq!(default_gap["search_visibility"], "missed", "{default_gap}");
    assert_eq!(default_gap["reason"], "default-import", "{default_gap}");
    assert_eq!(default_gap["search_as"], "Thing", "{default_gap}");
}

#[test]
fn audit_brief_lists_missed_consumers_of_touched_declarations() {
    let dir = tempfile::tempdir().expect("temporary project");
    let root = dir.path();
    write_search_probe(root);
    git(root, &["init", "-b", "main"]);
    commit_all(root, "base");

    fs::write(
        root.join("src/lib.ts"),
        "export const formatDate = (d: Date): string => d.toUTCString();\n\nexport default function Widget(): string {\n  return 'w';\n}\n",
    )
    .expect("change lib.ts");
    commit_all(root, "head");

    let output = run_fallow_raw(&[
        "audit",
        "--root",
        root.to_str().expect("utf-8 root"),
        "--base",
        "main~1",
        "--brief",
        "--format",
        "json",
        "--quiet",
    ]);
    assert_eq!(output.code, 0, "stderr: {}", output.stderr);
    let brief = parse_json(&output);
    let search_gap = &brief["impact_closure"]["search_gap"];
    let exports = search_gap["exports"]
        .as_array()
        .unwrap_or_else(|| panic!("search_gap.exports: {}", brief["impact_closure"]));
    assert_eq!(
        exports.len(),
        1,
        "only the touched declaration: {search_gap}"
    );
    let export = &exports[0];
    assert_eq!(export["file"], "src/lib.ts");
    assert_eq!(export["export_name"], "formatDate");
    assert_eq!(export["line"], 1);
    assert_eq!(export["total"], 3, "{export}");
    assert_eq!(search_gap["total"], 3, "{search_gap}");
    let consumers = export["consumers"].as_array().expect("consumers array");
    let files: Vec<&str> = consumers
        .iter()
        .map(|consumer| consumer["consumer_file"].as_str().unwrap_or_default())
        .collect();
    assert_eq!(
        files,
        ["src/ns.ts", "src/via-barrel.ts", "src/alias.ts"],
        "missed consumers come first, then renamed ones: {search_gap}"
    );
    assert!(
        consumers
            .iter()
            .all(|consumer| consumer["search_visibility"] != "found"),
        "{search_gap}"
    );
}

#[test]
fn audit_brief_omits_search_gap_when_every_consumer_is_found() {
    let dir = tempfile::tempdir().expect("temporary project");
    let root = dir.path();
    fs::create_dir_all(root.join("src")).expect("create source directory");
    fs::write(
        root.join("package.json"),
        r#"{"name":"search-clean","type":"module","main":"src/index.ts"}"#,
    )
    .expect("write package.json");
    fs::write(
        root.join("src/index.ts"),
        "import { value } from './lib';\nconsole.log(value);\n",
    )
    .expect("write index.ts");
    fs::write(root.join("src/lib.ts"), "export const value = 1;\n").expect("write lib.ts");
    git(root, &["init", "-b", "main"]);
    commit_all(root, "base");
    fs::write(root.join("src/lib.ts"), "export const value = 2;\n").expect("change lib.ts");
    commit_all(root, "head");

    let output = run_fallow_raw(&[
        "audit",
        "--root",
        root.to_str().expect("utf-8 root"),
        "--base",
        "main~1",
        "--brief",
        "--format",
        "json",
        "--quiet",
    ]);
    assert_eq!(output.code, 0, "stderr: {}", output.stderr);
    let brief = parse_json(&output);
    assert!(
        brief["impact_closure"].is_object(),
        "the brief still has an impact closure: {brief}"
    );
    assert!(
        brief["impact_closure"].get("search_gap").is_none(),
        "{}",
        brief["impact_closure"]
    );
}
