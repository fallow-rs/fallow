#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests use unwrap and expect to keep fixture setup concise"
)]

use crate::common::{copy_fixture, parse_json, run_fallow, run_fallow_in_root};

const FIXTURE: &str = "package-cycle-workspace";

fn write_rules(root: &std::path::Path, severity: &str) {
    std::fs::write(
        root.join(".fallowrc.json"),
        format!(r#"{{ "rules": {{ "package-cycle": "{severity}" }} }}"#),
    )
    .expect("write config");
}

#[test]
fn reproduction_reports_package_cycle_as_json() {
    let output = run_fallow(
        "dead-code",
        FIXTURE,
        &["--format", "json", "--quiet", "--no-cache"],
    );
    assert_eq!(output.code, 0, "warn does not fail: {}", output.stderr);
    let json = parse_json(&output);
    assert_eq!(json["summary"]["package_cycles"], 1);
    assert_eq!(json["circular_dependencies"].as_array().unwrap().len(), 0);
    let cycles = json["package_cycles"].as_array().unwrap();
    assert_eq!(cycles.len(), 1);
    let cycle = &cycles[0];
    assert_eq!(
        cycle["packages"],
        serde_json::json!(["@repro/a", "@repro/b"])
    );
    assert_eq!(cycle["length"], 2);
    assert_eq!(cycle["edges"][0]["path"], "packages/a/src/x.ts");
    assert_eq!(cycle["edges"][0]["target_path"], "packages/b/src/y.ts");
    assert_eq!(cycle["edges"][1]["path"], "packages/b/src/z.ts");
    assert_eq!(cycle["edges"][1]["type_only"], false);
    assert_eq!(cycle["effective_severity"], "warn");
}

#[test]
fn error_severity_fails_the_run() {
    let dir = copy_fixture(FIXTURE);
    write_rules(dir.path(), "error");
    let output = run_fallow_in_root(
        "dead-code",
        dir.path(),
        &["--format", "json", "--quiet", "--no-cache"],
    );
    assert_eq!(output.code, 1, "error severity fails: {}", output.stderr);
    let json = parse_json(&output);
    assert_eq!(json["package_cycles"][0]["effective_severity"], "error");
}

#[test]
fn off_severity_hides_the_finding() {
    let dir = copy_fixture(FIXTURE);
    write_rules(dir.path(), "off");
    let output = run_fallow_in_root(
        "dead-code",
        dir.path(),
        &["--format", "json", "--quiet", "--no-cache"],
    );
    assert_eq!(output.code, 0, "{}", output.stderr);
    let json = parse_json(&output);
    assert_eq!(json["package_cycles"].as_array().map_or(0, Vec::len), 0);
}

#[test]
fn package_cycles_flag_scopes_the_report() {
    let output = run_fallow(
        "dead-code",
        FIXTURE,
        &[
            "--package-cycles",
            "--format",
            "json",
            "--quiet",
            "--no-cache",
        ],
    );
    assert_eq!(output.code, 0, "{}", output.stderr);
    let json = parse_json(&output);
    assert_eq!(json["package_cycles"].as_array().unwrap().len(), 1);
    assert_eq!(json["total_issues"], 1);
}

#[test]
fn human_output_lists_packages_and_example_imports() {
    let output = run_fallow("dead-code", FIXTURE, &["--no-cache"]);
    assert_eq!(output.code, 0, "{}", output.stderr);
    assert!(
        output.stdout.contains("Package cycles (1)"),
        "{}",
        output.stdout
    );
    assert!(output.stdout.contains("@repro/a"), "{}", output.stdout);
    assert!(
        output.stdout.contains("packages/b/src/z.ts:1"),
        "{}",
        output.stdout
    );
}

fn write_override(root: &std::path::Path, files: &[&str]) {
    let files = files
        .iter()
        .map(|file| format!("\"{file}\""))
        .collect::<Vec<_>>()
        .join(", ");
    std::fs::write(
        root.join(".fallowrc.json"),
        format!(
            r#"{{ "overrides": [{{ "files": [{files}], "rules": {{ "package-cycle": "off" }} }}] }}"#
        ),
    )
    .expect("write config");
}

/// A per-file `off` on every importing file removes every hop, so the cycle
/// goes away.
#[test]
fn per_file_off_on_every_importing_file_hides_the_cycle() {
    let dir = copy_fixture(FIXTURE);
    write_override(dir.path(), &["packages/**"]);
    let output = run_fallow_in_root(
        "dead-code",
        dir.path(),
        &["--format", "json", "--quiet", "--no-cache"],
    );
    assert_eq!(output.code, 0, "{}", output.stderr);
    let json = parse_json(&output);
    assert_eq!(json["package_cycles"].as_array().map_or(0, Vec::len), 0);
    assert_eq!(json["total_issues"], 0);
}

/// The hop from `@repro/a` to `@repro/b` has one import. A per-file `off`
/// on that file removes the hop, so the cycle goes away, the same as an
/// inline suppression on that import.
#[test]
fn per_file_off_on_the_only_import_of_a_hop_hides_the_cycle() {
    let dir = copy_fixture(FIXTURE);
    write_override(dir.path(), &["packages/a/**"]);
    let output = run_fallow_in_root(
        "dead-code",
        dir.path(),
        &["--format", "json", "--quiet", "--no-cache"],
    );
    assert_eq!(output.code, 0, "{}", output.stderr);
    let json = parse_json(&output);
    assert_eq!(json["package_cycles"].as_array().map_or(0, Vec::len), 0);
}

/// Add a second import on each hop of the reproduction cycle. The new files
/// sort before the original example imports, so they become the example
/// imports while nothing removes them.
fn add_second_import_per_hop(root: &std::path::Path, comment: &str) {
    std::fs::write(
        root.join("packages/a/src/m.ts"),
        format!("{comment}import {{ y }} from \"@repro/b/y\";\nexport const m = () => y();\n"),
    )
    .expect("write a/src/m.ts");
    std::fs::write(
        root.join("packages/b/src/v.ts"),
        format!("{comment}import {{ w }} from \"@repro/a/w\";\nexport const v = () => w();\n"),
    )
    .expect("write b/src/v.ts");
}

fn example_paths(json: &serde_json::Value) -> Vec<String> {
    json["package_cycles"][0]["edges"]
        .as_array()
        .unwrap()
        .iter()
        .map(|edge| edge["path"].as_str().unwrap().to_owned())
        .collect()
}

/// A per-file `off` removes the imports of that file from the package graph.
/// The cycle stays while another import keeps each hop, and the example
/// import moves to that other import.
#[test]
fn per_file_off_keeps_the_cycle_while_another_import_keeps_each_hop() {
    let dir = copy_fixture(FIXTURE);
    add_second_import_per_hop(dir.path(), "");
    write_override(dir.path(), &["packages/a/src/m.ts", "packages/b/src/v.ts"]);
    let output = run_fallow_in_root(
        "dead-code",
        dir.path(),
        &[
            "--format",
            "json",
            "--quiet",
            "--no-cache",
            "--package-cycles",
        ],
    );
    assert_eq!(output.code, 0, "{}", output.stderr);
    let json = parse_json(&output);
    assert_eq!(json["package_cycles"].as_array().map_or(0, Vec::len), 1);
    assert_eq!(
        example_paths(&json),
        ["packages/a/src/x.ts", "packages/b/src/z.ts"]
    );
}

/// An inline suppression removes one import. The cycle stays while another
/// import keeps each hop.
#[test]
fn inline_suppression_keeps_the_cycle_while_another_import_keeps_each_hop() {
    let dir = copy_fixture(FIXTURE);
    add_second_import_per_hop(dir.path(), "// fallow-ignore-next-line package-cycle\n");
    let output = run_fallow_in_root(
        "dead-code",
        dir.path(),
        &[
            "--format",
            "json",
            "--quiet",
            "--no-cache",
            "--package-cycles",
        ],
    );
    assert_eq!(output.code, 0, "{}", output.stderr);
    let json = parse_json(&output);
    assert_eq!(json["package_cycles"].as_array().map_or(0, Vec::len), 1);
    assert_eq!(
        example_paths(&json),
        ["packages/a/src/x.ts", "packages/b/src/z.ts"]
    );
}

/// `--group-by` puts each package cycle in the group of its first example
/// import file, the same file that scope and severity use.
#[test]
fn group_by_directory_puts_the_cycle_in_a_group_as_json() {
    let output = run_fallow(
        "dead-code",
        FIXTURE,
        &[
            "--package-cycles",
            "--group-by",
            "directory",
            "--format",
            "json",
            "--quiet",
            "--no-cache",
        ],
    );
    assert_eq!(output.code, 0, "{}", output.stderr);
    let json = parse_json(&output);
    assert_eq!(json["total_issues"], 1);
    let groups = json["groups"].as_array().unwrap();
    assert_eq!(groups.len(), 1, "{}", output.stdout);
    assert_eq!(groups[0]["key"], "packages");
    assert_eq!(groups[0]["total_issues"], 1);
    let cycles = groups[0]["package_cycles"].as_array().unwrap();
    assert_eq!(cycles.len(), 1);
    assert_eq!(
        cycles[0]["packages"],
        serde_json::json!(["@repro/a", "@repro/b"])
    );
}

#[test]
fn group_by_directory_lists_the_cycle_in_human_output() {
    let output = run_fallow(
        "dead-code",
        FIXTURE,
        &["--package-cycles", "--group-by", "directory", "--no-cache"],
    );
    assert_eq!(output.code, 0, "{}", output.stderr);
    assert!(
        output.stdout.contains("Package cycles (1)"),
        "{}",
        output.stdout
    );
    assert!(
        output.stdout.contains("packages/b/src/z.ts:1"),
        "{}",
        output.stdout
    );
}

/// Re-export cycles use the same grouping loop: the first file of the cycle
/// picks the group.
#[test]
fn group_by_directory_puts_the_re_export_cycle_in_a_group_as_json() {
    let output = run_fallow(
        "dead-code",
        "re-export-cycle-2-node",
        &[
            "--re-export-cycles",
            "--group-by",
            "directory",
            "--format",
            "json",
            "--quiet",
            "--no-cache",
        ],
    );
    let json = parse_json(&output);
    assert_eq!(json["total_issues"], 1, "{}", output.stdout);
    let groups = json["groups"].as_array().unwrap();
    assert_eq!(groups.len(), 1, "{}", output.stdout);
    assert_eq!(groups[0]["key"], "src");
    assert_eq!(groups[0]["re_export_cycles"].as_array().unwrap().len(), 1);
}

const SUPPRESS: &str = "// fallow-ignore-next-line package-cycle\n";

/// Write `packages/a/src/x.ts` with two imports from one target module, run
/// the full dead-code report and return the JSON.
fn run_with_two_imports_in_one_file(first: &str, second: &str) -> serde_json::Value {
    let dir = copy_fixture(FIXTURE);
    std::fs::write(
        dir.path().join("packages/b/src/y.ts"),
        "export const y = () => \"y\";\nexport type Y = string;\n",
    )
    .expect("write b/src/y.ts");
    std::fs::write(
        dir.path().join("packages/a/src/x.ts"),
        format!("{first}\nexport const x = () => y();\n{second}\n"),
    )
    .expect("write a/src/x.ts");
    let output = run_fallow_in_root(
        "dead-code",
        dir.path(),
        &["--format", "json", "--quiet", "--no-cache"],
    );
    assert_eq!(output.code, 0, "{}", output.stderr);
    parse_json(&output)
}

/// Lines of the stale suppressions in `packages/a/src/x.ts`.
fn stale_lines(json: &serde_json::Value) -> Vec<u64> {
    json["stale_suppressions"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|stale| stale["path"] == "packages/a/src/x.ts")
        .map(|stale| stale["line"].as_u64().unwrap())
        .collect()
}

fn cycle_count(json: &serde_json::Value) -> usize {
    json["package_cycles"].as_array().map_or(0, Vec::len)
}

/// Two imports in one file from one target module are two imports, not one.
/// A suppression on the re-export removes only the re-export, so the cycle
/// stays through the plain import and the suppression is not stale.
#[test]
fn suppressed_re_export_keeps_the_cycle_through_the_other_import_in_the_same_file() {
    let json = run_with_two_imports_in_one_file(
        "import { y } from \"@repro/b/y\";",
        &format!("{SUPPRESS}export {{ y as y2 }} from \"@repro/b/y\";"),
    );
    assert_eq!(cycle_count(&json), 1, "{json}");
    assert_eq!(json["package_cycles"][0]["edges"][0]["line"], 1, "{json}");
    assert!(stale_lines(&json).is_empty(), "{json}");
}

/// A suppression on the first import does not remove the re-export below it
/// from the same module, so the cycle stays through the re-export.
#[test]
fn suppressed_import_keeps_the_cycle_through_the_re_export_in_the_same_file() {
    let json = run_with_two_imports_in_one_file(
        &format!("{SUPPRESS}import {{ y }} from \"@repro/b/y\";"),
        "export { y as y2 } from \"@repro/b/y\";",
    );
    assert_eq!(cycle_count(&json), 1, "{json}");
    assert_eq!(json["package_cycles"][0]["edges"][0]["line"], 4, "{json}");
    assert!(stale_lines(&json).is_empty(), "{json}");
}

/// A suppressed type-only import and a runtime import from one module. The
/// cycle stays through the runtime import, and the suppression is not stale.
#[test]
fn suppressed_type_import_keeps_the_cycle_through_the_runtime_import_in_the_same_file() {
    let json = run_with_two_imports_in_one_file(
        &format!(
            "{SUPPRESS}import type {{ Y }} from \"@repro/b/y\";\nimport {{ y }} from \"@repro/b/y\";"
        ),
        "export type Z = Y;",
    );
    assert_eq!(cycle_count(&json), 1, "{json}");
    let edge = &json["package_cycles"][0]["edges"][0];
    assert_eq!(edge["line"], 3, "{json}");
    assert_eq!(edge["type_only"], false, "{json}");
    assert!(stale_lines(&json).is_empty(), "{json}");
}

/// A suppressed runtime import leaves only the type-only import, so the hop
/// is type-only.
#[test]
fn suppressed_runtime_import_leaves_a_type_only_hop() {
    let json = run_with_two_imports_in_one_file(
        "import type { Y } from \"@repro/b/y\";",
        &format!("{SUPPRESS}import {{ y }} from \"@repro/b/y\";\nexport type Z = Y;"),
    );
    assert_eq!(cycle_count(&json), 1, "{json}");
    let edge = &json["package_cycles"][0]["edges"][0];
    assert_eq!(edge["line"], 1, "{json}");
    assert_eq!(edge["type_only"], true, "{json}");
    assert!(stale_lines(&json).is_empty(), "{json}");
}

/// A suppression on every import of the hop removes the hop, so the cycle
/// goes away and no suppression is stale.
#[test]
fn suppressing_every_import_in_the_same_file_hides_the_cycle() {
    let json = run_with_two_imports_in_one_file(
        &format!("{SUPPRESS}import {{ y }} from \"@repro/b/y\";"),
        &format!("{SUPPRESS}export {{ y as y2 }} from \"@repro/b/y\";"),
    );
    assert_eq!(cycle_count(&json), 0, "{json}");
    assert!(stale_lines(&json).is_empty(), "{json}");
}

/// A suppression above a multi-line re-export statement removes the
/// re-export. It is the only import of the hop, so the cycle goes away.
#[test]
fn suppressed_multi_line_re_export_hides_the_cycle() {
    let json = run_with_two_imports_in_one_file(
        &format!("{SUPPRESS}export {{\n  y as y2,\n}} from \"@repro/b/y\";"),
        "",
    );
    assert_eq!(cycle_count(&json), 0, "{json}");
    assert!(stale_lines(&json).is_empty(), "{json}");
}

/// A re-export is the example import of its hop, on its own line. A
/// type-only re-export makes a type-only hop.
#[test]
fn a_type_only_re_export_is_a_type_only_hop_on_its_own_line() {
    let json = run_with_two_imports_in_one_file(
        "const y = () => \"local\";",
        "export type { Y } from \"@repro/b/y\";",
    );
    assert_eq!(cycle_count(&json), 1, "{json}");
    let edge = &json["package_cycles"][0]["edges"][0];
    assert_eq!(edge["line"], 3, "{json}");
    assert_eq!(edge["type_only"], true, "{json}");
}
