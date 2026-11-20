//! Tests that the issue registry rows agree with what the commands do.

use clap::CommandFactory;

use super::build_cli_schema;
use crate::Cli;

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

/// `fallow architecture` reports the import-cycle, boundary and policy
/// findings, so their rows say so next to `command: dead-code`.
#[test]
fn architecture_rows_name_the_architecture_command() {
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
            note.contains("`fallow architecture`"),
            "{id} note must name `fallow architecture`: {row}"
        );
    }
}
