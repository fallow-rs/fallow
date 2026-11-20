//! Tests that the issue registry rows agree with what the commands do.

use clap::CommandFactory;
use fallow_types::suppress::DEAD_CODE_FILTER_FLAGS;
use rustc_hash::FxHashSet;

use super::build_cli_schema;
use crate::Cli;

fn schema() -> serde_json::Value {
    build_cli_schema(&Cli::command())
}

/// Collect every `--long` flag of a subcommand from live clap state.
fn subcommand_flags(name: &str) -> FxHashSet<String> {
    let cmd = Cli::command();
    let sub = cmd
        .find_subcommand(name)
        .unwrap_or_else(|| panic!("no subcommand named {name}"));
    sub.get_arguments()
        .filter_map(|a| a.get_long().map(|l| format!("--{l}")))
        .collect()
}

fn issue_row(id: &str) -> serde_json::Value {
    let schema = build_cli_schema(&Cli::command());
    schema["issue_types"]
        .as_array()
        .expect("issue_types array")
        .iter()
        .find(|row| row["id"] == id)
        .unwrap_or_else(|| panic!("no issue_types row for {id}"))
        .clone()
}

/// The catalog detector reads a `#` comment above a `pnpm-workspace.yaml`
/// entry, so the row is suppressible with that YAML comment.
#[test]
fn unused_catalog_entry_row_gives_the_yaml_suppression_comment() {
    let row = issue_row("unused-catalog-entry");
    assert_eq!(row["suppressible"], true, "{row}");
    assert_eq!(
        row["suppress_comment"], "# fallow-ignore-next-line unused-catalog-entry",
        "{row}"
    );
}

/// `fallow dead-code` still reports the import-cycle, boundary and policy
/// findings until the next major version, so their rows say so next to
/// `command: architecture`.
#[test]
fn architecture_rows_name_the_deprecated_dead_code_form() {
    for id in [
        "circular-dependency",
        "re-export-cycle",
        "package-cycle",
        "boundary-violation",
        "boundary-coverage",
        "boundary-call-violation",
        "policy-violation",
    ] {
        let row = issue_row(id);
        let note = row["note"].as_str().unwrap_or_default();
        assert!(
            note.contains("`fallow dead-code`"),
            "{id} note must name the deprecated `fallow dead-code` form: {row}"
        );
    }
}

/// Filter flags in the manifest must exist on the live clap command,
/// and the shared dead-code filter-flag list must be fully represented.
#[test]
fn filter_flags_exist_on_live_clap_commands() {
    let schema = schema();
    let rows = schema["issue_types"].as_array().unwrap();
    let dead_code_flags = subcommand_flags("dead-code");
    let health_flags = subcommand_flags("health");
    let architecture_flags = subcommand_flags("architecture");

    let mut seen_dead_code_filters: FxHashSet<&str> = FxHashSet::default();
    for row in rows {
        let Some(flag) = row["filter_flag"].as_str() else {
            continue;
        };
        let dead_code_flag = row["deprecated_filter_flag"].as_str().unwrap_or(flag);
        match row["command"].as_str().unwrap() {
            "architecture" => {
                assert!(
                    architecture_flags.contains(flag),
                    "row {} filter_flag {flag} does not exist on the architecture subcommand",
                    row["id"]
                );
                assert!(
                    dead_code_flags.contains(dead_code_flag),
                    "row {} deprecated_filter_flag {dead_code_flag} does not exist on the dead-code subcommand",
                    row["id"]
                );
                seen_dead_code_filters.insert(
                    DEAD_CODE_FILTER_FLAGS
                        .iter()
                        .find(|f| **f == dead_code_flag)
                        .unwrap(),
                );
            }
            "dead-code" => {
                assert!(
                    DEAD_CODE_FILTER_FLAGS.contains(&flag),
                    "row {} filter_flag {flag} is not in the shared DEAD_CODE_FILTER_FLAGS list",
                    row["id"]
                );
                assert!(
                    dead_code_flags.contains(flag),
                    "row {} filter_flag {flag} does not exist on the dead-code subcommand",
                    row["id"]
                );
                seen_dead_code_filters
                    .insert(DEAD_CODE_FILTER_FLAGS.iter().find(|f| **f == flag).unwrap());
            }
            "health" => {
                assert!(
                    health_flags.contains(flag),
                    "row {} filter_flag {flag} does not exist on the health subcommand",
                    row["id"]
                );
            }
            other => panic!("unexpected filter_flag on command {other}"),
        }
    }
    for flag in DEAD_CODE_FILTER_FLAGS.iter() {
        assert!(
            seen_dead_code_filters.contains(flag),
            "shared filter flag {flag} is not represented by any issue_types row"
        );
    }
}
