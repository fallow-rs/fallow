//! `fallow architecture`: cycles, boundaries and rule-pack policy rules.
//!
//! The command runs the dead-code pipeline with the architecture issue types
//! selected. These tests pin that the findings, the finding ids, the exit
//! code and the baselines stay the same as the `dead-code` structure flags,
//! and that the JSON root has its own `kind` and `schema_version`.

use std::path::Path;

use crate::common::{
    CommandOutput, canonical_report, parse_json, run_fallow_in_root, run_fallow_raw,
    strip_volatile_fields,
};

/// `ARCHITECTURE_SCHEMA_VERSION` of the `architecture` envelope.
const ARCHITECTURE_SCHEMA_VERSION: u64 = 2;

const ARCHITECTURE_ARRAYS: [&str; 7] = [
    "circular_dependencies",
    "re_export_cycles",
    "package_cycles",
    "boundary_violations",
    "boundary_coverage_violations",
    "boundary_call_violations",
    "policy_violations",
];

const HINT: &str = "fallow architecture";

fn write(root: &Path, rel: &str, body: &str) {
    let path = root.join(rel);
    std::fs::create_dir_all(path.parent().expect("parent")).expect("create dir");
    std::fs::write(path, body).expect("write file");
}

/// A project with one import cycle, one boundary violation, one rule-pack
/// violation and one unused export.
fn architecture_project(config_rules: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    write(
        root,
        "package.json",
        r#"{ "name": "arch-fixture", "main": "src/ui/App.ts", "dependencies": { "moment": "^2.30.0" } }"#,
    );
    write(
        root,
        ".fallowrc.json",
        &format!(
            r#"{{
  "entry": ["src/ui/App.ts"],
  "rules": {{ {config_rules} }},
  "rulePacks": ["packs/team.jsonc"],
  "boundaries": {{
    "zones": [
      {{ "name": "ui", "patterns": ["src/ui/**"] }},
      {{ "name": "db", "patterns": ["src/db/**"] }},
      {{ "name": "shared", "patterns": ["src/shared/**"] }}
    ],
    "rules": [
      {{ "from": "ui", "allow": ["shared"] }},
      {{ "from": "db", "allow": ["shared"] }}
    ]
  }}
}}"#
        ),
    );
    write(
        root,
        "packs/team.jsonc",
        r#"{
  "version": 1,
  "name": "team",
  "rules": [
    { "id": "no-moment", "kind": "banned-import", "specifiers": ["moment"], "message": "Use date-fns." }
  ]
}"#,
    );
    write(
        root,
        "src/ui/App.ts",
        "import { helper } from '../shared/utils';\n\
         import { query } from '../db/query';\n\
         import moment from 'moment';\n\
         import { first } from './cycle-a';\n\
         export const app = () => helper() + query() + first() + String(moment());\n",
    );
    write(
        root,
        "src/ui/cycle-a.ts",
        "import { second } from './cycle-b';\nexport const first = () => second();\n",
    );
    write(
        root,
        "src/ui/cycle-b.ts",
        "import { first } from './cycle-a';\nexport const second = () => (first ? 'b' : 'c');\n",
    );
    write(
        root,
        "src/db/query.ts",
        "import { helper } from '../shared/utils';\nexport const query = () => 'SELECT ' + helper();\n",
    );
    write(
        root,
        "src/shared/utils.ts",
        "export const helper = () => 'help';\nexport const neverUsed = 1;\n",
    );
    dir
}

fn run(command: &str, root: &Path, args: &[&str]) -> CommandOutput {
    let mut full = vec!["--no-cache"];
    full.extend_from_slice(args);
    run_fallow_in_root(command, root, &full)
}

fn array_len(json: &serde_json::Value, key: &str) -> usize {
    json[key].as_array().map_or(0, Vec::len)
}

/// The report body without the fields that name the command: the root `kind`
/// and `schema_version`, `_meta` and `next_steps`. Equal bodies mean equal
/// findings, finding ids, actions, gate outcomes and baselines.
fn envelope_body(output: &CommandOutput) -> String {
    let mut value = parse_json(output);
    strip_volatile_fields(&mut value);
    if let Some(root) = value.as_object_mut() {
        for key in ["kind", "schema_version", "_meta", "next_steps"] {
            root.remove(key);
        }
    }
    serde_json::to_string(&value).expect("re-serialize report body")
}

#[test]
fn architecture_json_equals_dead_code_structure_filters() {
    let dir = architecture_project("");
    let architecture = run("architecture", dir.path(), &["--format", "json", "--quiet"]);
    let legacy = run(
        "dead-code",
        dir.path(),
        &[
            "--circular-deps",
            "--re-export-cycles",
            "--package-cycles",
            "--boundary-violations",
            "--policy-violations",
            "--format",
            "json",
            "--quiet",
        ],
    );
    assert_eq!(architecture.code, legacy.code, "{}", architecture.stderr);
    let architecture_json = parse_json(&architecture);
    let legacy_json = parse_json(&legacy);
    assert_eq!(architecture_json["kind"], "architecture");
    assert_eq!(
        architecture_json["schema_version"],
        ARCHITECTURE_SCHEMA_VERSION
    );
    assert_eq!(legacy_json["kind"], "dead-code");
    assert_eq!(envelope_body(&architecture), envelope_body(&legacy));
}

#[test]
fn architecture_grouped_json_has_architecture_grouped_kind() {
    let dir = architecture_project("");
    let args = ["--group-by", "directory", "--format", "json", "--quiet"];
    let architecture = run("architecture", dir.path(), &args);
    let legacy = run(
        "dead-code",
        dir.path(),
        &[
            "--circular-deps",
            "--re-export-cycles",
            "--package-cycles",
            "--boundary-violations",
            "--policy-violations",
            "--group-by",
            "directory",
            "--format",
            "json",
            "--quiet",
        ],
    );
    assert_eq!(architecture.code, legacy.code, "{}", architecture.stderr);
    let json = parse_json(&architecture);
    assert_eq!(
        json["kind"], "architecture-grouped",
        "{}",
        architecture.stdout
    );
    assert_eq!(json["schema_version"], ARCHITECTURE_SCHEMA_VERSION);
    assert_eq!(json["grouped_by"], "directory");
    assert_eq!(parse_json(&legacy)["kind"], "dead-code-grouped");
    assert_eq!(envelope_body(&architecture), envelope_body(&legacy));
}

#[test]
fn dead_code_json_keeps_dead_code_kind_and_version() {
    let dir = architecture_project("");
    let flat = parse_json(&run(
        "dead-code",
        dir.path(),
        &["--format", "json", "--quiet"],
    ));
    assert_eq!(flat["kind"], "dead-code");
    assert_eq!(flat["schema_version"], 11);
    let grouped = parse_json(&run(
        "dead-code",
        dir.path(),
        &["--group-by", "directory", "--format", "json", "--quiet"],
    ));
    assert_eq!(grouped["kind"], "dead-code-grouped");
    assert_eq!(grouped["schema_version"], 11);
}

#[test]
fn bare_only_architecture_json_keeps_combined_shape() {
    let dir = architecture_project("");
    let output = run_bare(
        dir.path(),
        &["--only", "architecture", "--format", "json", "--quiet"],
    );
    let json = parse_json(&output);
    assert_eq!(json["kind"], "combined", "{}", output.stdout);
    assert!(json.get("architecture").is_none());
    assert!(
        json["check"].get("kind").is_none(),
        "the embedded check section carries no kind"
    );
    assert_eq!(json["check"]["schema_version"], 11);
}

#[test]
fn architecture_save_baseline_keeps_the_dead_code_baseline_format() {
    let dir = architecture_project("");
    let baseline = dir.path().join("architecture-baseline.json");
    let baseline_arg = baseline.to_str().expect("utf-8 path");
    let save = run(
        "architecture",
        dir.path(),
        &[
            "--save-baseline",
            baseline_arg,
            "--format",
            "json",
            "--quiet",
        ],
    );
    let saved: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(&baseline)
            .unwrap_or_else(|err| panic!("baseline saved ({err}): {}", save.stderr)),
    )
    .expect("baseline is JSON");
    assert_eq!(saved["kind"], "dead-code");
    assert_eq!(parse_json(&save)["kind"], "architecture");
}

#[test]
fn architecture_recheck_baseline_step_names_architecture() {
    let dir = architecture_project("");
    let baseline = dir.path().join("baseline.json");
    let baseline_arg = baseline.to_str().expect("utf-8 path");
    let save = run(
        "architecture",
        dir.path(),
        &[
            "--save-baseline",
            baseline_arg,
            "--format",
            "json",
            "--quiet",
        ],
    );
    assert!(baseline.exists(), "baseline saved: {}", save.stderr);

    // The positional path narrows the run, so the baseline matches less than
    // it carries and the envelope offers the unscoped recheck.
    let output = run(
        "architecture",
        dir.path(),
        &[
            "src/db/query.ts",
            "--baseline",
            baseline_arg,
            "--format",
            "json",
            "--quiet",
        ],
    );
    let json = parse_json(&output);
    let recheck = json["next_steps"]
        .as_array()
        .and_then(|steps| steps.iter().find(|step| step["id"] == "recheck-baseline"))
        .unwrap_or_else(|| panic!("expected a recheck-baseline entry, got {json}"));
    let command = recheck["command"].as_str().expect("command is a string");
    assert!(
        command.starts_with("fallow architecture --baseline "),
        "the step reruns the command that ran: {command}"
    );
}

#[test]
fn architecture_reports_only_architecture_findings() {
    let dir = architecture_project("");
    let output = run("architecture", dir.path(), &["--format", "json", "--quiet"]);
    let json = parse_json(&output);
    assert_eq!(json["kind"], "architecture");
    assert_eq!(json["schema_version"], ARCHITECTURE_SCHEMA_VERSION);
    assert_eq!(array_len(&json, "circular_dependencies"), 1);
    assert_eq!(array_len(&json, "boundary_violations"), 1);
    assert_eq!(array_len(&json, "policy_violations"), 1);
    assert_eq!(array_len(&json, "unused_exports"), 0);

    let dead_code = run("dead-code", dir.path(), &["--format", "json", "--quiet"]);
    let dead_code = parse_json(&dead_code);
    assert_eq!(
        array_len(&dead_code, "unused_exports"),
        1,
        "dead-code still reports the unused export"
    );
    for key in ARCHITECTURE_ARRAYS {
        assert_eq!(
            array_len(&dead_code, key),
            array_len(&json, key),
            "dead-code keeps reporting {key} by default"
        );
        let ids = |value: &serde_json::Value| -> Vec<String> {
            value[key]
                .as_array()
                .map(|items| {
                    items
                        .iter()
                        .map(|item| item["finding_id"].to_string())
                        .collect()
                })
                .unwrap_or_default()
        };
        assert_eq!(ids(&dead_code), ids(&json), "finding ids of {key} match");
    }
}

#[test]
fn architecture_filter_flags_select_one_kind() {
    let dir = architecture_project("");
    let cycles = parse_json(&run(
        "architecture",
        dir.path(),
        &["--cycles", "--format", "json", "--quiet"],
    ));
    assert_eq!(array_len(&cycles, "circular_dependencies"), 1);
    assert_eq!(array_len(&cycles, "boundary_violations"), 0);
    assert_eq!(array_len(&cycles, "policy_violations"), 0);

    let boundaries = parse_json(&run(
        "architecture",
        dir.path(),
        &["--boundaries", "--format", "json", "--quiet"],
    ));
    assert_eq!(array_len(&boundaries, "circular_dependencies"), 0);
    assert_eq!(array_len(&boundaries, "boundary_violations"), 1);
    assert_eq!(array_len(&boundaries, "policy_violations"), 0);

    let policy = parse_json(&run(
        "architecture",
        dir.path(),
        &["--policy", "--format", "json", "--quiet"],
    ));
    assert_eq!(array_len(&policy, "circular_dependencies"), 0);
    assert_eq!(array_len(&policy, "boundary_violations"), 0);
    assert_eq!(array_len(&policy, "policy_violations"), 1);

    let legacy_cycles = run(
        "dead-code",
        dir.path(),
        &[
            "--circular-deps",
            "--re-export-cycles",
            "--package-cycles",
            "--format",
            "json",
            "--quiet",
        ],
    );
    let architecture_cycles = run(
        "architecture",
        dir.path(),
        &["--cycles", "--format", "json", "--quiet"],
    );
    assert_eq!(
        envelope_body(&architecture_cycles),
        envelope_body(&legacy_cycles)
    );
}

#[test]
fn architecture_honors_file_scope() {
    let dir = architecture_project("");
    let output = run(
        "architecture",
        dir.path(),
        &["--file", "src/db/query.ts", "--format", "json", "--quiet"],
    );
    let json = parse_json(&output);
    assert_eq!(json["kind"], "architecture", "{}", output.stderr);
    assert_eq!(array_len(&json, "boundary_violations"), 0);
    assert_eq!(array_len(&json, "policy_violations"), 0);
}

#[test]
fn architecture_accepts_a_dead_code_baseline() {
    let dir = architecture_project("");
    let baseline = dir.path().join("dead-code-baseline.json");
    let baseline_arg = baseline.to_str().expect("utf-8 path");
    let save = run(
        "dead-code",
        dir.path(),
        &[
            "--save-baseline",
            baseline_arg,
            "--format",
            "json",
            "--quiet",
        ],
    );
    assert!(baseline.exists(), "baseline saved: {}", save.stderr);

    let output = run(
        "architecture",
        dir.path(),
        &["--baseline", baseline_arg, "--format", "json", "--quiet"],
    );
    let json = parse_json(&output);
    assert_eq!(json["kind"], "architecture", "{}", output.stderr);
    for key in ARCHITECTURE_ARRAYS {
        assert_eq!(array_len(&json, key), 0, "{key} is in the baseline");
    }
}

#[test]
fn architecture_exit_code_matches_dead_code_severity() {
    let dir = architecture_project(
        r#""circular-dependency": "error", "boundary-violation": "warn", "unused-export": "warn""#,
    );
    let architecture = run("architecture", dir.path(), &["--format", "json", "--quiet"]);
    let dead_code = run("dead-code", dir.path(), &["--format", "json", "--quiet"]);
    assert_eq!(architecture.code, 1, "{}", architecture.stderr);
    assert_eq!(dead_code.code, 1, "{}", dead_code.stderr);

    let all_warn = architecture_project(
        r#""circular-dependency": "warn", "boundary-violation": "warn", "unused-export": "warn""#,
    );
    let architecture = run(
        "architecture",
        all_warn.path(),
        &["--format", "json", "--quiet"],
    );
    let dead_code = run(
        "dead-code",
        all_warn.path(),
        &["--format", "json", "--quiet"],
    );
    assert_eq!(architecture.code, 0, "{}", architecture.stderr);
    assert_eq!(dead_code.code, 0, "{}", dead_code.stderr);
}

#[test]
fn architecture_human_output_has_one_architecture_category() {
    let dir = architecture_project("");
    let output = run("architecture", dir.path(), &[]);
    assert!(
        output.stdout.contains("Architecture"),
        "architecture category header: {}",
        output.stdout
    );
    assert!(output.stdout.contains("Circular dependencies"));
    assert!(output.stdout.contains("Boundary violations"));
    assert!(output.stdout.contains("Policy violations"));
    assert!(!output.stdout.contains("Unused Code"));
    assert!(!output.stdout.contains("Structure"));
    assert!(
        !output.stderr.contains(HINT),
        "no hint on the architecture command: {}",
        output.stderr
    );
}

#[test]
fn dead_code_human_prints_architecture_hint_once() {
    let dir = architecture_project("");
    let output = run("dead-code", dir.path(), &[]);
    assert_eq!(
        output.stderr.matches(HINT).count(),
        1,
        "one hint on stderr: {}",
        output.stderr
    );
    assert!(!output.stdout.contains(HINT), "stdout stays unchanged");
    assert!(
        output.stdout.contains("Structure"),
        "dead-code keeps its Structure heading"
    );
}

#[test]
fn dead_code_quiet_and_json_print_no_hint() {
    let dir = architecture_project("");
    let quiet = run("dead-code", dir.path(), &["--quiet"]);
    assert!(!quiet.stderr.contains(HINT), "{}", quiet.stderr);
    let json = run("dead-code", dir.path(), &["--format", "json"]);
    assert!(!json.stderr.contains(HINT), "{}", json.stderr);
    assert!(!json.stdout.contains(HINT));
}

#[test]
fn dead_code_without_architecture_findings_prints_no_hint() {
    let dir = architecture_project("");
    let output = run("dead-code", dir.path(), &["--unused-exports"]);
    assert!(
        output.stdout.contains("Unused exports"),
        "{}",
        output.stdout
    );
    assert!(!output.stderr.contains(HINT), "{}", output.stderr);
}

#[test]
fn bare_fallow_human_shows_architecture_section() {
    let dir = architecture_project("");
    let output = run_fallow_raw(&[
        "--root",
        dir.path().to_str().expect("utf-8 path"),
        "--no-cache",
    ]);
    assert!(
        !output.stderr.contains("── Architecture"),
        "no top-level architecture section heading on stderr: {}",
        output.stderr
    );
    let dead_code = output.stderr.find("── Dead Code ──").expect("dead code");
    let duplication = output.stderr.find("── Duplication ──").expect("dupes");
    let status = output
        .stderr
        .find("1 policy violation (")
        .unwrap_or_else(|| panic!("dead-code status line: {}", output.stderr));
    assert!(
        dead_code < status && status < duplication,
        "the status line stays in the Dead Code section: {}",
        output.stderr
    );
    for title in ["Circular dependencies", "Policy violations"] {
        assert_eq!(
            last_category_before(&output.stdout, title),
            Some("Architecture"),
            "{title} sits in the Architecture category: {}",
            output.stdout
        );
    }
    let unused = output.stdout.find("── Unused Code ").expect("unused code");
    let architecture = output.stdout.find("── Architecture ").expect("arch");
    assert!(unused < architecture, "{}", output.stdout);
    assert!(
        !output.stdout.contains("── Structure ──"),
        "the cycles leave the dead-code Structure category: {}",
        output.stdout
    );
    assert!(!output.stderr.contains("Tip: run `fallow architecture`"));
}

/// The label of the last `── Label ──` category heading in `text` before the
/// first occurrence of `needle`.
fn last_category_before<'a>(text: &'a str, needle: &str) -> Option<&'a str> {
    let end = text.find(needle)?;
    text[..end]
        .lines()
        .rev()
        .find_map(|line| line.trim().strip_prefix("── ")?.split(" ─").next())
}

#[test]
fn bare_fallow_quiet_uses_the_same_architecture_category() {
    let dir = architecture_project("");
    let plain = run_bare(dir.path(), &[]);
    let quiet = run_bare(dir.path(), &["--quiet"]);
    for output in [&plain, &quiet] {
        assert_eq!(
            last_category_before(&output.stdout, "Circular dependencies"),
            Some("Architecture"),
            "{}",
            output.stdout
        );
        assert!(
            !output.stdout.contains("── Structure "),
            "{}",
            output.stdout
        );
        assert!(!output.stdout.contains("── Policy "), "{}", output.stdout);
    }
}

#[test]
fn dead_code_quiet_keeps_the_structure_and_policy_categories() {
    let dir = architecture_project("");
    let output = run("dead-code", dir.path(), &["--quiet"]);
    assert!(output.stdout.contains("── Structure "), "{}", output.stdout);
    assert!(output.stdout.contains("── Policy "), "{}", output.stdout);
    assert!(
        !output.stdout.contains("── Architecture "),
        "{}",
        output.stdout
    );
}

#[test]
fn architecture_status_line_names_policy_and_boundary_violations() {
    let dir = architecture_project("");
    let policy = run("architecture", dir.path(), &["--policy"]);
    assert!(
        policy.stderr.contains("1 policy violation ("),
        "policy status line: {}",
        policy.stderr
    );
    let boundaries = run("architecture", dir.path(), &["--boundaries"]);
    assert!(
        boundaries.stderr.contains("1 boundary violation ("),
        "boundary status line: {}",
        boundaries.stderr
    );
}

#[test]
fn bare_only_architecture_failure_line_names_architecture() {
    let dir = architecture_project(r#""circular-dependency": "error""#);
    let output = run_bare(dir.path(), &["--only", "architecture"]);
    assert_eq!(output.code, 1, "{}", output.stderr);
    assert!(
        output.stderr.contains("Failed: architecture (3 issues)"),
        "{}",
        output.stderr
    );
    assert!(
        !output.stderr.contains("Failed: dead-code"),
        "{}",
        output.stderr
    );
}

#[test]
fn bare_summary_without_dead_code_rows_starts_at_the_architecture_label() {
    let dir = architecture_project(r#""unused-export": "off", "unused-file": "off""#);
    let output = run_bare(dir.path(), &["--only", "dead-code", "--summary"]);
    assert!(
        output.stdout.starts_with("  Architecture"),
        "no leading blank line: {:?}",
        output.stdout
    );
}

#[test]
fn architecture_json_points_to_the_architecture_docs() {
    let dir = architecture_project("");
    let output = run(
        "architecture",
        dir.path(),
        &["--format", "json", "--quiet", "--explain"],
    );
    let json = parse_json(&output);
    assert_eq!(
        json["_meta"]["docs"], "https://fallow.tools/docs/cli/architecture/",
        "{}",
        output.stdout
    );
    let rules: Vec<&str> = json["_meta"]["rules"]
        .as_object()
        .expect("_meta.rules is an object")
        .keys()
        .map(String::as_str)
        .collect();
    // `--explain` lists only the rules of the reported findings, and the
    // fixture reports a boundary violation, a cycle and a policy violation.
    assert_eq!(
        rules,
        [
            "boundary-violation",
            "circular-dependency",
            "policy-violation",
        ],
        "_meta.rules lists only the architecture rules with findings"
    );
    let dead_code = run(
        "dead-code",
        dir.path(),
        &["--format", "json", "--quiet", "--explain"],
    );
    let dead_code = parse_json(&dead_code);
    assert_eq!(
        dead_code["_meta"]["docs"],
        "https://fallow.tools/docs/cli/dead-code/"
    );
}

#[test]
fn architecture_help_uses_one_description() {
    const DESCRIPTION: &str = "Check import cycles, boundaries and policy rules after editing";
    let root = run_fallow_raw(&["--help"]);
    let row = root
        .stdout
        .lines()
        .find(|line| line.trim_start().starts_with("architecture "))
        .expect("architecture command row");
    assert!(row.contains(DESCRIPTION), "{row}");
    let short = run_fallow_raw(&["architecture", "-h"]);
    assert!(short.stdout.starts_with(DESCRIPTION), "{}", short.stdout);
}

#[test]
fn bare_fallow_json_keeps_architecture_findings_in_dead_code() {
    let dir = architecture_project("");
    let output = run_fallow_raw(&[
        "--root",
        dir.path().to_str().expect("utf-8 path"),
        "--no-cache",
        "--format",
        "json",
        "--quiet",
    ]);
    let json = parse_json(&output);
    assert_eq!(array_len(&json["check"], "circular_dependencies"), 1);
    assert!(json.get("architecture").is_none());
}

#[test]
fn help_marks_dead_code_structure_flags_as_deprecated_aliases() {
    let output = run_fallow_raw(&["dead-code", "--help"]);
    for flag in [
        "--circular-deps",
        "--re-export-cycles",
        "--package-cycles",
        "--boundary-violations",
        "--policy-violations",
    ] {
        let line = output
            .stdout
            .lines()
            .skip_while(|line| !line.trim_start().starts_with(flag))
            .take(3)
            .collect::<Vec<_>>()
            .join(" ");
        assert!(
            line.contains("Deprecated alias") && line.contains("fallow architecture"),
            "{flag} help: {line}"
        );
    }
}

#[test]
fn root_help_puts_guard_and_architecture_together() {
    let output = run_fallow_raw(&["--help"]);
    let lines: Vec<&str> = output.stdout.lines().collect();
    let guard_row = lines
        .iter()
        .position(|line| line.contains("fallow guard <files>"))
        .expect("guard row");
    assert!(
        lines[guard_row + 1].contains("fallow architecture"),
        "architecture row follows guard: {}",
        lines[guard_row + 1]
    );
    let guard_cmd = lines
        .iter()
        .position(|line| line.trim_start().starts_with("guard "))
        .expect("guard command row");
    assert!(
        lines[guard_cmd + 1]
            .trim_start()
            .starts_with("architecture "),
        "architecture command follows guard: {}",
        lines[guard_cmd + 1]
    );
}

fn run_bare(root: &Path, args: &[&str]) -> CommandOutput {
    let mut full = vec!["--root", root.to_str().expect("utf-8 path"), "--no-cache"];
    full.extend_from_slice(args);
    run_fallow_raw(&full)
}

#[test]
fn bare_only_architecture_reports_the_architecture_findings() {
    let dir = architecture_project("");
    let bare = run_bare(
        dir.path(),
        &["--only", "architecture", "--format", "json", "--quiet"],
    );
    let architecture = run("architecture", dir.path(), &["--format", "json", "--quiet"]);
    let bare_json = parse_json(&bare);
    let architecture_json = parse_json(&architecture);
    for key in ARCHITECTURE_ARRAYS {
        assert_eq!(
            bare_json["check"][key], architecture_json[key],
            "{key} must match `fallow architecture`"
        );
    }
    assert_eq!(array_len(&bare_json["check"], "circular_dependencies"), 1);
    assert_eq!(array_len(&bare_json["check"], "unused_exports"), 0);
    assert!(bare_json.get("dupes").is_none(), "no duplication section");
    assert!(bare_json.get("health").is_none(), "no health section");
}

#[test]
fn bare_only_dead_code_and_architecture_is_the_full_dead_code_section() {
    let dir = architecture_project("");
    let both = run_bare(
        dir.path(),
        &[
            "--only",
            "dead-code,architecture",
            "--format",
            "json",
            "--quiet",
        ],
    );
    let dead_code = run_bare(
        dir.path(),
        &["--only", "dead-code", "--format", "json", "--quiet"],
    );
    assert_eq!(canonical_report(&both), canonical_report(&dead_code));
}

#[test]
fn bare_skip_architecture_drops_only_the_architecture_findings() {
    let dir = architecture_project("");
    let full = run_bare(dir.path(), &["--format", "json", "--quiet"]);
    let skipped = run_bare(
        dir.path(),
        &["--skip", "architecture", "--format", "json", "--quiet"],
    );
    let full_json = parse_json(&full);
    let skipped_json = parse_json(&skipped);
    assert!(array_len(&full_json["check"], "circular_dependencies") > 0);
    for key in ARCHITECTURE_ARRAYS {
        assert_eq!(
            array_len(&skipped_json["check"], key),
            0,
            "{key} is dropped"
        );
    }
    assert_eq!(
        skipped_json["check"]["unused_exports"], full_json["check"]["unused_exports"],
        "the other dead-code findings stay"
    );
    assert!(
        skipped_json.get("dupes").is_some(),
        "duplication still runs"
    );
    assert_eq!(
        skipped_json["health"]["health_score"], full_json["health"]["health_score"],
        "dropping report lines does not change the health score"
    );
}

#[test]
fn bare_only_architecture_human_has_no_dead_code_heading() {
    let dir = architecture_project("");
    let output = run_bare(dir.path(), &["--only", "architecture"]);
    assert!(
        output.stdout.starts_with("── Architecture "),
        "{}",
        output.stdout
    );
    assert!(
        !output.stderr.contains("── Dead Code ──"),
        "the run reports no dead-code finding types: {}",
        output.stderr
    );
    assert!(
        !output.stderr.contains("── Architecture"),
        "one architecture heading: {}",
        output.stderr
    );
    assert!(output.stdout.contains("Circular dependencies"));
    assert!(!output.stdout.contains("Unused exports"));
}

#[test]
fn bare_only_architecture_quiet_keeps_the_architecture_category() {
    let dir = architecture_project("");
    let output = run_bare(dir.path(), &["--only", "architecture", "--quiet"]);
    assert!(
        output.stdout.contains("── Architecture "),
        "{}",
        output.stdout
    );
    assert!(
        !output.stdout.contains("── Structure "),
        "{}",
        output.stdout
    );
}

#[test]
fn bare_group_by_renders_an_architecture_category_per_group() {
    let dir = architecture_project("");
    let output = run_bare(
        dir.path(),
        &["--only", "dead-code", "--group-by", "directory"],
    );
    assert!(
        output.stdout.contains("── Architecture "),
        "architecture category in the groups: {}",
        output.stdout
    );
    assert!(
        !output.stdout.contains("── Structure "),
        "the cycles leave the Structure category: {}",
        output.stdout
    );
    assert!(!output.stdout.contains("── Policy "), "{}", output.stdout);
    assert!(output.stdout.contains("Circular dependencies"));
    assert!(output.stdout.contains("Policy violations"));
}

#[test]
fn architecture_group_by_uses_one_architecture_heading() {
    let dir = architecture_project("");
    let output = run("architecture", dir.path(), &["--group-by", "directory"]);
    assert!(
        output.stdout.contains("── Architecture "),
        "{}",
        output.stdout
    );
    assert!(
        !output.stdout.contains("── Structure "),
        "{}",
        output.stdout
    );
    assert!(!output.stdout.contains("── Policy "), "{}", output.stdout);
    assert!(output.stdout.contains("Circular dependencies"));
    assert!(output.stdout.contains("Policy violations"));
}

#[test]
fn bare_summary_groups_the_architecture_rows() {
    let dir = architecture_project("");
    let output = run_bare(dir.path(), &["--only", "dead-code", "--summary"]);
    let heading = output
        .stdout
        .find("Architecture")
        .unwrap_or_else(|| panic!("architecture row group: {}", output.stdout));
    let unused = output.stdout.find("Unused exports").expect("unused row");
    let cycles = output
        .stdout
        .find("Circular dependencies")
        .expect("cycle row");
    let policy = output.stdout.find("Policy violations").expect("policy row");
    let total = output.stdout.find("Total").expect("total row");
    assert!(unused < heading, "{}", output.stdout);
    assert!(heading < cycles && cycles < total, "{}", output.stdout);
    assert!(heading < policy && policy < total, "{}", output.stdout);
}

#[test]
fn dead_code_summary_keeps_one_row_list() {
    let dir = architecture_project("");
    let output = run("dead-code", dir.path(), &["--summary"]);
    assert!(output.stdout.contains("Circular dependencies"));
    assert!(
        !output.stdout.contains("Architecture"),
        "dead-code keeps its row list: {}",
        output.stdout
    );
}

#[test]
fn shared_state_inventory_selection_and_format_refusal() {
    let dir = architecture_project("");
    write(
        dir.path(),
        "src/shared/state.ts",
        "export const state = {};\n",
    );
    write(
        dir.path(),
        "src/writer.ts",
        "import {state} from './shared/state'; state.value = 1; window.custom = 2;\n",
    );
    write(
        dir.path(),
        "src/reader.ts",
        "export const read = () => window.custom;\n",
    );
    let output = run(
        "architecture",
        dir.path(),
        &[
            "--shared-state",
            "--format",
            "json",
            "--quiet",
            "--fail-on-issues",
        ],
    );
    assert_eq!(output.code, 0, "{}", output.stderr);
    let json = parse_json(&output);
    assert_eq!(json["shared_state"]["version"], 1);
    assert!(
        !json["shared_state"]["imported_writes"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert!(json["circular_dependencies"].as_array().unwrap().is_empty());
    let cycles = run(
        "architecture",
        dir.path(),
        &["--shared-state", "--cycles", "--format", "json", "--quiet"],
    );
    assert!(
        !parse_json(&cycles)["circular_dependencies"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    for format in ["sarif", "compact", "codeclimate"] {
        let unsupported = run(
            "architecture",
            dir.path(),
            &["--shared-state", "--format", format, "--quiet"],
        );
        assert_eq!(unsupported.code, 2, "{format}: {}", unsupported.stderr);
    }
}

#[test]
fn shared_state_saved_output_and_scope_preserve_supported_evidence() {
    let dir = architecture_project("");
    write(
        dir.path(),
        "src/shared/state.ts",
        "export const state = {};\n",
    );
    write(
        dir.path(),
        "src/writer.ts",
        "import {state} from './shared/state'; state['a.b'] = 1; window.custom = 2;\n",
    );
    write(
        dir.path(),
        "src/reader.ts",
        "export const read = () => window.custom;\n",
    );
    let output = run(
        "architecture",
        dir.path(),
        &["--shared-state", "--format", "json", "--quiet"],
    );
    assert_eq!(output.code, 0, "{}", output.stderr);
    let json = parse_json(&output);
    assert_eq!(
        json["shared_state"]["imported_writes"][0]["property"],
        serde_json::json!(["a.b"])
    );
    assert_eq!(
        json["shared_state"]["imported_writes"][0]["writer"]["path"],
        "src/writer.ts"
    );
    let scoped = run(
        "architecture",
        dir.path(),
        &[
            "--shared-state",
            "--file",
            "src/reader.ts",
            "--format",
            "json",
            "--quiet",
        ],
    );
    let scoped_json = parse_json(&scoped);
    assert!(
        scoped_json["shared_state"]["imported_writes"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        scoped_json["shared_state"]["globals"],
        json["shared_state"]["globals"]
    );
    write(dir.path(), "saved.json", &output.stdout);
    let markdown = run(
        "report",
        dir.path(),
        &[
            "--from",
            dir.path().join("saved.json").to_str().unwrap(),
            "--format",
            "markdown",
        ],
    );
    assert_eq!(markdown.code, 0, "{}", markdown.stderr);
    assert!(markdown.stdout.contains("Shared-state inventory"));
    assert!(markdown.stdout.contains("src/writer.ts"));
    let unsupported = run(
        "report",
        dir.path(),
        &[
            "--from",
            dir.path().join("saved.json").to_str().unwrap(),
            "--format",
            "sarif",
        ],
    );
    assert_eq!(unsupported.code, 2);
    let mut invalid = json;
    invalid["shared_state"]["version"] = serde_json::json!(2);
    write(
        dir.path(),
        "saved.json",
        &serde_json::to_string(&invalid).unwrap(),
    );
    let refused = run(
        "report",
        dir.path(),
        &[
            "--from",
            dir.path().join("saved.json").to_str().unwrap(),
            "--format",
            "markdown",
        ],
    );
    assert_eq!(refused.code, 2);
}

#[test]
fn shared_state_keeps_parse_gates_and_rejects_observation_filters() {
    let dir = architecture_project("");
    write(dir.path(), "src/broken.ts", "export const broken = ;\n");
    let ungated = run(
        "architecture",
        dir.path(),
        &["--shared-state", "--format", "json", "--quiet"],
    );
    assert_eq!(ungated.code, 0, "{}", ungated.stderr);
    assert_eq!(parse_json(&ungated)["shared_state"]["complete"], false);
    let gated = run(
        "architecture",
        dir.path(),
        &[
            "--shared-state",
            "--fail-on-parse-error",
            "--format",
            "json",
            "--quiet",
        ],
    );
    assert_eq!(gated.code, 1, "{}", gated.stderr);
    for selector in [
        ["--group-by", "directory"],
        ["--finding-id", "dc1:unused-file:0000000000000000"],
    ] {
        let refused = run(
            "architecture",
            dir.path(),
            &[
                "--shared-state",
                selector[0],
                selector[1],
                "--format",
                "json",
                "--quiet",
            ],
        );
        assert_eq!(refused.code, 2);
    }
}

#[test]
fn shared_state_saved_grouped_inventory_is_rejected_even_when_empty() {
    let dir = architecture_project("");
    let grouped = parse_json(&run(
        "architecture",
        dir.path(),
        &["--group-by", "directory", "--format", "json", "--quiet"],
    ));
    let saved = dir.path().join("grouped.json");
    let render = |value: &serde_json::Value| {
        std::fs::write(&saved, serde_json::to_vec(value).unwrap()).unwrap();
        run(
            "report",
            dir.path(),
            &["--from", saved.to_str().unwrap(), "--format", "markdown"],
        )
    };
    let ordinary = render(&grouped);
    assert_eq!(ordinary.code, 0, "{}", ordinary.stderr);
    assert!(ordinary.stdout.contains("Circular"), "{}", ordinary.stdout);
    let empty = serde_json::json!({"version":1,"complete":true,"truncated":false,"imported_writes":[],"globals":[]});
    let mut root = grouped.clone();
    root["shared_state"] = empty.clone();
    let rejected = render(&root);
    assert_eq!(
        rejected.code, 2,
        "present empty inventory must not be dropped: {}",
        rejected.stdout
    );
    let mut nested = grouped;
    nested["groups"][0]["shared_state"] = empty;
    assert_eq!(render(&nested).code, 2);
}

#[test]
fn shared_state_rejects_sarif_sidecar_without_overwriting_artifact() {
    for populated in [false, true] {
        let dir = architecture_project("");
        if populated {
            write(dir.path(), "src/writer.ts", "window.custom = 1;");
            write(
                dir.path(),
                "src/reader.ts",
                "export const read = () => window.custom;",
            );
        }
        let supported = run(
            "architecture",
            dir.path(),
            &["--shared-state", "--format", "json", "--quiet"],
        );
        assert_eq!(supported.code, 0, "{}", supported.stderr);
        assert_eq!(
            parse_json(&supported)["shared_state"]["globals"]
                .as_array()
                .unwrap()
                .len(),
            usize::from(populated)
        );
        let artifact = dir.path().join("inventory.sarif");
        std::fs::write(&artifact, "existing sentinel").unwrap();
        let rejected = run(
            "architecture",
            dir.path(),
            &[
                "--shared-state",
                "--format",
                "json",
                "--sarif-file",
                artifact.to_str().unwrap(),
                "--quiet",
            ],
        );
        assert_eq!(rejected.code, 2, "{}", rejected.stdout);
        assert_eq!(
            std::fs::read_to_string(&artifact).unwrap(),
            "existing sentinel"
        );
    }
}
