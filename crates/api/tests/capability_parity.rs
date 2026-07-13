//! api-side drift guard for the cross-surface capability parity table
//! (`fallow_types::mcp_manifest::CAPABILITY_PARITY`).
//!
//! Source-scans this crate's public `run_*` re-exports and asserts they match
//! the table's `api_runner` column in both directions: every runner named in
//! the table exists, and every public runner appears in the table. A new
//! `run_*` entry point that is not recorded in the parity table fails here.

use std::collections::BTreeSet;

/// Extract the public `run_*` identifiers re-exported from `lib.rs`.
///
/// Scans the non-`#[cfg(test)]` portion, skipping comment lines, for tokens
/// that begin with `run_` at an identifier boundary. This mirrors the
/// include_str source-scan the schemars-alias guards use, and matches the 17
/// `pub use runtime::{...}` / `pub use list_runtime::{...}` runner re-exports
/// without hard-coding their names.
fn public_run_fns(source: &str) -> BTreeSet<String> {
    let head = source.split("#[cfg(test)]").next().unwrap_or(source);
    let mut found = BTreeSet::new();
    for line in head.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("//") || trimmed.starts_with('*') || trimmed.starts_with("/*") {
            continue;
        }
        let bytes = line.as_bytes();
        let mut cursor = 0;
        while let Some(pos) = line[cursor..].find("run_") {
            let start = cursor + pos;
            let boundary = start == 0
                || !matches!(bytes[start - 1], b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'_');
            if boundary {
                let mut end = start + "run_".len();
                while end < bytes.len() && matches!(bytes[end], b'a'..=b'z' | b'_') {
                    end += 1;
                }
                found.insert(line[start..end].to_string());
            }
            cursor = start + "run_".len();
        }
    }
    found
}

#[test]
fn api_run_fns_match_capability_parity_table() {
    let scanned = public_run_fns(include_str!("../src/lib.rs"));
    assert!(
        scanned.len() >= 7,
        "source scan found too few run_* re-exports ({}); the scan pattern likely broke",
        scanned.len()
    );

    let table: BTreeSet<String> = fallow_types::mcp_manifest::CAPABILITY_PARITY
        .iter()
        .filter_map(|row| row.api_runner)
        .map(str::to_string)
        .collect();

    for runner in &table {
        assert!(
            scanned.contains(runner),
            "capability-parity api_runner {runner:?} is not a public run_* re-export of fallow-api"
        );
    }
    for runner in &scanned {
        assert!(
            table.contains(runner),
            "public run_* {runner:?} is missing from the CAPABILITY_PARITY table; add a row for it"
        );
    }
}
