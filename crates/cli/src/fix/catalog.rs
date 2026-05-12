//! Auto-fix for `unused-catalog-entries` findings.
//!
//! Removes unused pnpm catalog entries from `pnpm-workspace.yaml`. The
//! strategy is line-aware deletion rather than full YAML parse-and-reprint:
//! there is no comment-preserving YAML writer in the workspace, and a
//! full reprint via `serde_yaml_ng` would obliterate comments, anchors,
//! and stylistic choices. Each entry's start line is taken from the
//! finding's `line` field; the end line is computed by scanning forward
//! for lines whose indentation is strictly greater than the entry line's
//! own indent (this covers object-form entries such as
//! `react:\n    specifier: ^18.2.0\n    publishConfig: {}`).
//!
//! Entries whose `hardcoded_consumers` is non-empty are skipped: removing
//! the catalog entry while a workspace package still references it via
//! the `catalog:` protocol would break the user's next `pnpm install`.
//! The skip is reported in the fix output (and in human stderr) so the
//! user knows to migrate the consumer first.
//!
//! Multi-document YAML files (`---` document separators) are rejected
//! with a skip record because the single-pass line scanner cannot
//! reliably attribute lines to documents.

use std::path::Path;

use fallow_config::OutputFormat;
use fallow_core::results::UnusedCatalogEntry;

use super::io::{atomic_write, read_source};

/// Apply unused-catalog-entry fixes to `pnpm-workspace.yaml`.
///
/// Returns `(had_write_error, applied_count, skipped_count)` so the
/// orchestrator can build the top-level fix-output summary. The returned
/// `skipped_count` only counts entries that were intentionally not
/// removed (hardcoded consumer, multi-doc YAML, line out of range); it
/// does NOT count entries that produced a write error.
pub(super) fn apply_catalog_entry_fixes(
    root: &Path,
    entries: &[UnusedCatalogEntry],
    output: OutputFormat,
    dry_run: bool,
    fixes: &mut Vec<serde_json::Value>,
) -> CatalogFixSummary {
    let mut summary = CatalogFixSummary::default();

    if entries.is_empty() {
        return summary;
    }

    // All entries share the same file (`pnpm-workspace.yaml`), but we group
    // defensively so we read+write the file once even if a future detector
    // adds entries from multiple files.
    let mut by_path: rustc_hash::FxHashMap<&Path, Vec<&UnusedCatalogEntry>> =
        rustc_hash::FxHashMap::default();
    for entry in entries {
        by_path.entry(entry.path.as_path()).or_default().push(entry);
    }

    for (relative_path, file_entries) in by_path {
        let absolute = root.join(relative_path);
        let Some((content, line_ending)) = read_source(root, &absolute) else {
            // Skip silently when the workspace file is unreadable or escapes
            // the root: matches the existing pattern in enum_members/deps.
            continue;
        };

        // Multi-document YAML defense (panel P1.6). The line scanner cannot
        // reliably attribute lines to documents when `---` separators are
        // present; refuse to edit and surface the skip.
        if is_multi_document_yaml(&content) {
            for entry in &file_entries {
                summary.skipped += 1;
                fixes.push(skip_record(
                    entry,
                    "multi_document_yaml",
                    "Skipped: pnpm-workspace.yaml contains a `---` document separator; fallow fix does not support multi-document YAML",
                    output,
                    relative_path,
                ));
            }
            continue;
        }

        let lines: Vec<&str> = content.split(line_ending).collect();

        // Compute the line range for each entry and split into "remove" vs
        // "skip" buckets.
        let mut to_remove: Vec<(std::ops::Range<usize>, &UnusedCatalogEntry)> = Vec::new();
        for entry in &file_entries {
            if !entry.hardcoded_consumers.is_empty() {
                summary.skipped += 1;
                let consumer_summary = format_consumer_summary(&entry.hardcoded_consumers);
                let description = format!(
                    "Skipped: {consumer_summary} still pin `{}` with a hardcoded version. Switch the consumer(s) to \"{}\": \"catalog:{}\" first, then rerun fallow fix.",
                    entry.entry_name,
                    entry.entry_name,
                    if entry.catalog_name == "default" {
                        String::new()
                    } else {
                        entry.catalog_name.clone()
                    },
                );
                fixes.push(skip_record(
                    entry,
                    "hardcoded_consumers",
                    &description,
                    output,
                    relative_path,
                ));
                continue;
            }

            let line_idx = entry.line.saturating_sub(1) as usize;
            if line_idx >= lines.len() {
                summary.skipped += 1;
                fixes.push(skip_record(
                    entry,
                    "line_out_of_range",
                    "Skipped: the reported line is past the end of pnpm-workspace.yaml; the file may have been edited since fallow check ran",
                    output,
                    relative_path,
                ));
                continue;
            }

            let range = compute_deletion_range(&lines, line_idx);
            to_remove.push((range, entry));
        }

        if to_remove.is_empty() {
            continue;
        }

        // Sort descending by start so removals don't shift later indices.
        // Use end as a tiebreaker (longer-range first) so an overlapping
        // pair is handled deterministically.
        to_remove.sort_by(|a, b| {
            b.0.start
                .cmp(&a.0.start)
                .then_with(|| b.0.end.cmp(&a.0.end))
        });

        // Dedup overlapping ranges. With at most one entry per source line
        // (the detector emits one finding per line), overlap should only
        // occur on object-form entries where two findings somehow share a
        // span. Keep the first (longer) range in each overlapping pair.
        let mut deduped: Vec<(std::ops::Range<usize>, &UnusedCatalogEntry)> = Vec::new();
        for (range, entry) in to_remove {
            if let Some((last_range, _)) = deduped.last()
                && last_range.start < range.end
                && range.start < last_range.end
            {
                continue;
            }
            deduped.push((range, entry));
        }

        if dry_run {
            for (range, entry) in &deduped {
                if !matches!(output, OutputFormat::Json) {
                    eprintln!(
                        "Would remove catalog entry from {}:{} `{}` (catalog: {})",
                        relative_path.display(),
                        range.start + 1,
                        entry.entry_name,
                        entry.catalog_name,
                    );
                }
                fixes.push(remove_record(entry, range, false, relative_path));
            }
            summary.applied += deduped.len();
            continue;
        }

        // Track the parent header line for each deletion so we can detect
        // when an entire catalog group becomes empty (e.g. removing the
        // last entry from `catalogs.react17` leaves `react17:` with a
        // null value, which pnpm rejects with "Cannot convert undefined
        // or null to object" at install time).
        let parent_header_indices: Vec<usize> = deduped
            .iter()
            .filter_map(|(_, entry)| find_parent_header_line(&lines, entry))
            .collect();

        // Apply: drain ranges from a fresh Vec<String>, rewrite emptied
        // parent headers to `key: {}`, validate by reparse, then atomic-write.
        let mut new_lines: Vec<String> = lines.iter().map(ToString::to_string).collect();
        for (range, _) in &deduped {
            new_lines.drain(range.clone());
        }
        rewrite_empty_catalog_parents(&mut new_lines, &parent_header_indices, &deduped);

        let mut new_content = new_lines.join(line_ending);
        if content.ends_with(line_ending) && !new_content.ends_with(line_ending) {
            new_content.push_str(line_ending);
        }

        // Reparse-validate (panel P1.7). If the post-edit content fails to
        // parse, abort the write rather than risk corrupting the file. We
        // do not attempt a structural diff: any successful parse is a good
        // enough signal here, because the failure modes the validator
        // catches are syntactic (indent disasters, key-value disasters).
        if serde_yaml_ng::from_str::<serde_yaml_ng::Value>(&new_content).is_err() {
            summary.write_error = true;
            eprintln!(
                "Error: refusing to write {}: post-edit content failed YAML reparse. The file was not modified.",
                relative_path.display(),
            );
            continue;
        }

        if let Err(e) = atomic_write(&absolute, new_content.as_bytes()) {
            summary.write_error = true;
            eprintln!("Error: failed to write {}: {e}", relative_path.display());
            continue;
        }

        for (range, entry) in &deduped {
            fixes.push(remove_record(entry, range, true, relative_path));
        }
        summary.applied += deduped.len();
    }

    summary
}

/// Output of `apply_catalog_entry_fixes` consumed by the orchestrator.
#[derive(Debug, Default)]
pub(super) struct CatalogFixSummary {
    pub applied: usize,
    pub skipped: usize,
    pub write_error: bool,
}

/// Compute the deletion range `[start, end)` (line indices) for a catalog
/// entry whose key sits on `start_idx`. Object-form entries
/// (`react:\n    specifier: ^18.2.0`) consume every subsequent line with
/// strictly greater indent. Blank lines and lines at the entry's own
/// indent (or shallower) stop the scan: blank lines are conservatively
/// treated as inter-entry whitespace that should be preserved.
fn compute_deletion_range(lines: &[&str], start_idx: usize) -> std::ops::Range<usize> {
    let entry_indent = leading_spaces(lines[start_idx]);
    let mut end_idx = start_idx + 1;
    while end_idx < lines.len() {
        let line = lines[end_idx];
        if line.trim().is_empty() {
            break;
        }
        if leading_spaces(line) <= entry_indent {
            break;
        }
        end_idx += 1;
    }
    start_idx..end_idx
}

fn leading_spaces(line: &str) -> usize {
    line.bytes().take_while(|&b| b == b' ').count()
}

/// Detect a `---` YAML document separator on its own line. We don't try to
/// distinguish "leading directive divider" from "real document split"; any
/// `---` on its own line disqualifies the file from in-place line edits.
fn is_multi_document_yaml(content: &str) -> bool {
    content
        .lines()
        .any(|line| line.trim_end() == "---" || line.trim_end().starts_with("--- "))
}

/// Locate the line index of a catalog entry's parent header in the
/// PRE-deletion `lines` Vec. Returns:
/// - `Some(idx)` of the line containing `catalog:` for default-catalog entries
/// - `Some(idx)` of the line containing `<name>:` (indented under `catalogs:`)
///   for named-catalog entries
/// - `None` if no matching parent is found (the file shape diverges from
///   what the detector reported; the caller skips the rewrite step)
fn find_parent_header_line(lines: &[&str], entry: &UnusedCatalogEntry) -> Option<usize> {
    let entry_line_idx = entry.line.saturating_sub(1) as usize;
    if entry_line_idx >= lines.len() {
        return None;
    }
    let entry_indent = leading_spaces(lines[entry_line_idx]);

    // Walk backwards from the entry line to find the first line at
    // strictly lower indent. For default-catalog entries the parent
    // must start with `catalog:`; for named-catalog entries the parent
    // is the `<name>:` line at an intermediate indent under `catalogs:`.
    for idx in (0..entry_line_idx).rev() {
        let line = lines[idx];
        let stripped = line.trim_end();
        let content = stripped.trim_start();
        if content.is_empty() || content.starts_with('#') {
            continue;
        }
        let indent = leading_spaces(stripped);
        if indent >= entry_indent {
            continue;
        }
        if entry.catalog_name == "default" {
            return content.starts_with("catalog:").then_some(idx);
        }
        // Strip leading quotes for quoted-key catalog names.
        let key = content
            .trim_start_matches(['"', '\''])
            .split([':', '"', '\''])
            .next()
            .unwrap_or("");
        return (key == entry.catalog_name).then_some(idx);
    }
    None
}

/// Rewrite parent catalog headers whose only children were just deleted.
///
/// pnpm rejects null-valued catalogs (`catalogs:\n  react17:\n` parses
/// as `{'catalogs': {'react17': None}}`) with
/// `Cannot convert undefined or null to object` at install time. When
/// we empty a catalog group via `apply_catalog_entry_fixes`, rewrite
/// the header from `react17:` to `react17: {}` so the file stays
/// installable. Verified against pnpm 10.33.4.
///
/// `parent_indices` are line indices into the PRE-deletion `lines` Vec.
/// `deleted_ranges` are the ranges that were drained from that Vec.
/// Both are translated into POST-deletion `new_lines` coordinates by
/// subtracting the number of deleted lines preceding each anchor.
fn rewrite_empty_catalog_parents(
    new_lines: &mut [String],
    parent_indices: &[usize],
    deleted_ranges: &[(std::ops::Range<usize>, &UnusedCatalogEntry)],
) {
    // Dedup parents and map their pre-deletion indices into post-deletion
    // indices. A parent header line itself is NEVER inside a deletion
    // range (deletions cover entry lines plus their multi-line children,
    // which all sit BELOW the parent), so the mapping is simply
    // `new_idx = pre_idx - (lines deleted strictly before pre_idx)`.
    let mut unique_parents: Vec<usize> = parent_indices.to_vec();
    unique_parents.sort_unstable();
    unique_parents.dedup();

    for parent_pre_idx in unique_parents {
        let deleted_before: usize = deleted_ranges
            .iter()
            .map(|(range, _)| {
                if range.end <= parent_pre_idx {
                    range.end - range.start
                } else if range.start <= parent_pre_idx {
                    // Parent inside a deletion range is impossible by
                    // construction (deletions start at entry lines, which
                    // are strictly below the parent). Skip defensively.
                    0
                } else {
                    0
                }
            })
            .sum();
        let new_idx = parent_pre_idx.saturating_sub(deleted_before);
        if new_idx >= new_lines.len() {
            continue;
        }
        if has_remaining_children(new_lines, new_idx) {
            continue;
        }
        // Append ` {}` to the header line, preserving any trailing
        // whitespace / line ending semantics. `new_lines` was produced
        // by `content.split(line_ending)`, so trailing whitespace is
        // already trimmed and the line ending is added back on join.
        let original = new_lines[new_idx].clone();
        let trimmed_end = original.trim_end();
        let trailing = &original[trimmed_end.len()..];
        new_lines[new_idx] = format!("{trimmed_end} {{}}{trailing}");
    }
}

/// Return true if `parent_idx` in `lines` is followed by at least one
/// child line (indent strictly greater than the parent's). Comments and
/// blank lines are skipped; a sibling-or-shallower non-blank line means
/// the parent has no children.
fn has_remaining_children(lines: &[String], parent_idx: usize) -> bool {
    let parent_indent = leading_spaces(&lines[parent_idx]);
    for line in lines.iter().skip(parent_idx + 1) {
        let stripped = line.trim_end();
        let content = stripped.trim_start();
        if content.is_empty() || content.starts_with('#') {
            continue;
        }
        let indent = leading_spaces(stripped);
        return indent > parent_indent;
    }
    false
}

fn skip_record(
    entry: &UnusedCatalogEntry,
    skip_reason: &str,
    description: &str,
    output: OutputFormat,
    relative_path: &Path,
) -> serde_json::Value {
    if !matches!(output, OutputFormat::Json) {
        eprintln!(
            "Skipped catalog entry {}:{} `{}` ({skip_reason})",
            relative_path.display(),
            entry.line,
            entry.entry_name,
        );
    }
    let consumers: Option<serde_json::Value> =
        if skip_reason == "hardcoded_consumers" && !entry.hardcoded_consumers.is_empty() {
            Some(serde_json::Value::Array(
                entry
                    .hardcoded_consumers
                    .iter()
                    .map(|p| {
                        // Normalize separators to match the check-side
                        // `hardcoded_consumers` shape (which uses
                        // `serde_path::serialize_vec` doing `.replace('\\', "/")`)
                        // so agents correlating check + fix output see the
                        // same path strings on Windows.
                        serde_json::Value::String(p.to_string_lossy().replace('\\', "/"))
                    })
                    .collect(),
            ))
        } else {
            None
        };
    let mut value = serde_json::json!({
        "type": "remove_catalog_entry",
        "entry_name": entry.entry_name,
        "catalog_name": entry.catalog_name,
        "file": relative_path.to_string_lossy().replace('\\', "/"),
        "line": entry.line,
        "applied": false,
        "skipped": true,
        "skip_reason": skip_reason,
        "description": description,
    });
    if let Some(consumers) = consumers
        && let serde_json::Value::Object(map) = &mut value
    {
        map.insert("consumers".to_string(), consumers);
    }
    value
}

fn remove_record(
    entry: &UnusedCatalogEntry,
    range: &std::ops::Range<usize>,
    applied: bool,
    relative_path: &Path,
) -> serde_json::Value {
    let removed_lines = range.end - range.start;
    let mut value = serde_json::json!({
        "type": "remove_catalog_entry",
        "entry_name": entry.entry_name,
        "catalog_name": entry.catalog_name,
        "file": relative_path.to_string_lossy().replace('\\', "/"),
        "line": range.start + 1,
        "removed_lines": removed_lines,
    });
    if applied && let serde_json::Value::Object(map) = &mut value {
        map.insert("applied".to_string(), serde_json::Value::Bool(true));
    }
    value
}

fn format_consumer_summary(consumers: &[std::path::PathBuf]) -> String {
    match consumers.len() {
        0 => String::new(),
        1 => format!("`{}`", consumers[0].display()),
        2 => format!(
            "`{}` and `{}`",
            consumers[0].display(),
            consumers[1].display()
        ),
        _ => format!(
            "`{}` and {} other consumer(s)",
            consumers[0].display(),
            consumers.len() - 1,
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn make_entry(name: &str, catalog: &str, line: u32) -> UnusedCatalogEntry {
        UnusedCatalogEntry {
            entry_name: name.to_string(),
            catalog_name: catalog.to_string(),
            path: PathBuf::from("pnpm-workspace.yaml"),
            line,
            hardcoded_consumers: vec![],
        }
    }

    fn make_entry_with_consumers(
        name: &str,
        catalog: &str,
        line: u32,
        consumers: Vec<PathBuf>,
    ) -> UnusedCatalogEntry {
        UnusedCatalogEntry {
            entry_name: name.to_string(),
            catalog_name: catalog.to_string(),
            path: PathBuf::from("pnpm-workspace.yaml"),
            line,
            hardcoded_consumers: consumers,
        }
    }

    fn seed_workspace_file(root: &Path, content: &str) {
        let path = root.join("pnpm-workspace.yaml");
        std::fs::write(&path, content).unwrap();
    }

    #[test]
    fn removes_scalar_form_entry() {
        let dir = tempfile::tempdir().unwrap();
        let content = "catalog:\n  is-odd: ^1.0.0\n  is-even: ^1.0.0\n  left-pad: ^1.0.0\n";
        seed_workspace_file(dir.path(), content);

        let entries = vec![make_entry("is-even", "default", 3)];
        let mut fixes = Vec::new();
        let summary =
            apply_catalog_entry_fixes(dir.path(), &entries, OutputFormat::Json, false, &mut fixes);

        assert_eq!(summary.applied, 1);
        assert_eq!(summary.skipped, 0);
        assert!(!summary.write_error);

        let result = std::fs::read_to_string(dir.path().join("pnpm-workspace.yaml")).unwrap();
        assert_eq!(result, "catalog:\n  is-odd: ^1.0.0\n  left-pad: ^1.0.0\n");
        assert_eq!(fixes.len(), 1);
        assert_eq!(fixes[0]["applied"], serde_json::json!(true));
        assert_eq!(fixes[0]["removed_lines"], serde_json::json!(1));
    }

    #[test]
    fn removes_object_form_entry_with_nested_keys() {
        let dir = tempfile::tempdir().unwrap();
        let content = "catalog:\n  is-odd: ^1.0.0\n  react:\n    specifier: ^18.2.0\n    publishConfig:\n      access: public\n  is-even: ^1.0.0\n";
        seed_workspace_file(dir.path(), content);

        let entries = vec![make_entry("react", "default", 3)];
        let mut fixes = Vec::new();
        let summary =
            apply_catalog_entry_fixes(dir.path(), &entries, OutputFormat::Json, false, &mut fixes);

        assert_eq!(summary.applied, 1);
        assert_eq!(summary.skipped, 0);

        let result = std::fs::read_to_string(dir.path().join("pnpm-workspace.yaml")).unwrap();
        assert_eq!(result, "catalog:\n  is-odd: ^1.0.0\n  is-even: ^1.0.0\n");
        assert_eq!(fixes[0]["removed_lines"], serde_json::json!(4));
    }

    #[test]
    fn skips_entries_with_hardcoded_consumers() {
        let dir = tempfile::tempdir().unwrap();
        let content = "catalog:\n  is-even: ^1.0.0\n";
        seed_workspace_file(dir.path(), content);

        let entries = vec![make_entry_with_consumers(
            "is-even",
            "default",
            2,
            vec![PathBuf::from("apps/web/package.json")],
        )];
        let mut fixes = Vec::new();
        let summary =
            apply_catalog_entry_fixes(dir.path(), &entries, OutputFormat::Json, false, &mut fixes);

        assert_eq!(summary.applied, 0);
        assert_eq!(summary.skipped, 1);

        let result = std::fs::read_to_string(dir.path().join("pnpm-workspace.yaml")).unwrap();
        assert_eq!(result, content, "file must not be modified when skipping");

        assert_eq!(fixes.len(), 1);
        assert_eq!(fixes[0]["skipped"], serde_json::json!(true));
        assert_eq!(
            fixes[0]["skip_reason"],
            serde_json::json!("hardcoded_consumers")
        );
        assert!(
            fixes[0]["description"]
                .as_str()
                .unwrap()
                .contains("apps/web/package.json")
        );
        assert!(fixes[0]["consumers"].is_array());
    }

    #[test]
    fn dry_run_does_not_modify_file() {
        let dir = tempfile::tempdir().unwrap();
        let content = "catalog:\n  is-even: ^1.0.0\n";
        seed_workspace_file(dir.path(), content);

        let entries = vec![make_entry("is-even", "default", 2)];
        let mut fixes = Vec::new();
        let summary =
            apply_catalog_entry_fixes(dir.path(), &entries, OutputFormat::Json, true, &mut fixes);

        assert_eq!(summary.applied, 1);
        let result = std::fs::read_to_string(dir.path().join("pnpm-workspace.yaml")).unwrap();
        assert_eq!(result, content);
        assert_eq!(fixes[0].get("applied"), None);
    }

    #[test]
    fn removes_named_catalog_entry() {
        let dir = tempfile::tempdir().unwrap();
        let content = "catalogs:\n  react17:\n    react: ^17.0.2\n    react-dom: ^17.0.2\n";
        seed_workspace_file(dir.path(), content);

        let entries = vec![make_entry("react", "react17", 3)];
        let mut fixes = Vec::new();
        let summary =
            apply_catalog_entry_fixes(dir.path(), &entries, OutputFormat::Json, false, &mut fixes);

        assert_eq!(summary.applied, 1);
        let result = std::fs::read_to_string(dir.path().join("pnpm-workspace.yaml")).unwrap();
        assert_eq!(result, "catalogs:\n  react17:\n    react-dom: ^17.0.2\n");
    }

    #[test]
    fn preserves_trailing_inline_comment_on_sibling() {
        let dir = tempfile::tempdir().unwrap();
        let content = "catalog:\n  is-odd: ^1.0.0 # keep me\n  is-even: ^1.0.0 # remove me\n";
        seed_workspace_file(dir.path(), content);

        let entries = vec![make_entry("is-even", "default", 3)];
        let mut fixes = Vec::new();
        apply_catalog_entry_fixes(dir.path(), &entries, OutputFormat::Json, false, &mut fixes);

        let result = std::fs::read_to_string(dir.path().join("pnpm-workspace.yaml")).unwrap();
        assert_eq!(result, "catalog:\n  is-odd: ^1.0.0 # keep me\n");
    }

    #[test]
    fn leaves_leading_comment_as_orphan() {
        // The conservative comments policy preserves leading single-line
        // comments even when their sibling entry is removed. The orphaned
        // comment is mildly ugly but never destroys user intent.
        let dir = tempfile::tempdir().unwrap();
        let content = "catalog:\n  # mention is-even\n  is-even: ^1.0.0\n  is-odd: ^1.0.0\n";
        seed_workspace_file(dir.path(), content);

        let entries = vec![make_entry("is-even", "default", 3)];
        let mut fixes = Vec::new();
        apply_catalog_entry_fixes(dir.path(), &entries, OutputFormat::Json, false, &mut fixes);

        let result = std::fs::read_to_string(dir.path().join("pnpm-workspace.yaml")).unwrap();
        assert_eq!(result, "catalog:\n  # mention is-even\n  is-odd: ^1.0.0\n");
    }

    #[test]
    fn removes_multiple_adjacent_entries_in_one_pass() {
        let dir = tempfile::tempdir().unwrap();
        let content = "catalog:\n  is-odd: ^1.0.0\n  is-even: ^1.0.0\n  left-pad: ^1.0.0\n  right-pad: ^1.0.0\n";
        seed_workspace_file(dir.path(), content);

        let entries = vec![
            make_entry("is-even", "default", 3),
            make_entry("left-pad", "default", 4),
        ];
        let mut fixes = Vec::new();
        let summary =
            apply_catalog_entry_fixes(dir.path(), &entries, OutputFormat::Json, false, &mut fixes);

        assert_eq!(summary.applied, 2);
        let result = std::fs::read_to_string(dir.path().join("pnpm-workspace.yaml")).unwrap();
        assert_eq!(result, "catalog:\n  is-odd: ^1.0.0\n  right-pad: ^1.0.0\n");
    }

    #[test]
    fn rejects_multi_document_yaml() {
        let dir = tempfile::tempdir().unwrap();
        let content = "catalog:\n  is-even: ^1.0.0\n---\nfoo: bar\n";
        seed_workspace_file(dir.path(), content);

        let entries = vec![make_entry("is-even", "default", 2)];
        let mut fixes = Vec::new();
        let summary =
            apply_catalog_entry_fixes(dir.path(), &entries, OutputFormat::Json, false, &mut fixes);

        assert_eq!(summary.applied, 0);
        assert_eq!(summary.skipped, 1);
        assert_eq!(
            fixes[0]["skip_reason"],
            serde_json::json!("multi_document_yaml")
        );

        // File must not have been modified.
        let result = std::fs::read_to_string(dir.path().join("pnpm-workspace.yaml")).unwrap();
        assert_eq!(result, content);
    }

    #[test]
    fn skips_when_line_out_of_range() {
        let dir = tempfile::tempdir().unwrap();
        let content = "catalog:\n  is-even: ^1.0.0\n";
        seed_workspace_file(dir.path(), content);

        // line 99 is way past EOF (file has 3 lines including trailing newline)
        let entries = vec![make_entry("is-even", "default", 99)];
        let mut fixes = Vec::new();
        let summary =
            apply_catalog_entry_fixes(dir.path(), &entries, OutputFormat::Json, false, &mut fixes);

        assert_eq!(summary.applied, 0);
        assert_eq!(summary.skipped, 1);
        assert_eq!(
            fixes[0]["skip_reason"],
            serde_json::json!("line_out_of_range")
        );
    }

    #[test]
    fn preserves_crlf_line_endings() {
        let dir = tempfile::tempdir().unwrap();
        let content = "catalog:\r\n  is-odd: ^1.0.0\r\n  is-even: ^1.0.0\r\n";
        seed_workspace_file(dir.path(), content);

        let entries = vec![make_entry("is-even", "default", 3)];
        let mut fixes = Vec::new();
        apply_catalog_entry_fixes(dir.path(), &entries, OutputFormat::Json, false, &mut fixes);

        let result = std::fs::read_to_string(dir.path().join("pnpm-workspace.yaml")).unwrap();
        assert_eq!(result, "catalog:\r\n  is-odd: ^1.0.0\r\n");
    }

    #[test]
    fn rewrites_emptied_default_catalog_to_empty_map() {
        // Regression: pnpm rejects `catalog:\n` (null value) with
        // "Cannot convert undefined or null to object". When the fix
        // empties the default catalog, the header must be rewritten to
        // `catalog: {}` so the file stays installable.
        let dir = tempfile::tempdir().unwrap();
        let content = "catalog:\n  is-even: ^1.0.0\n";
        seed_workspace_file(dir.path(), content);

        let entries = vec![make_entry("is-even", "default", 2)];
        let mut fixes = Vec::new();
        apply_catalog_entry_fixes(dir.path(), &entries, OutputFormat::Json, false, &mut fixes);

        let result = std::fs::read_to_string(dir.path().join("pnpm-workspace.yaml")).unwrap();
        assert_eq!(result, "catalog: {}\n");
        let parsed: serde_yaml_ng::Value = serde_yaml_ng::from_str(&result).unwrap();
        assert!(
            parsed
                .get("catalog")
                .and_then(serde_yaml_ng::Value::as_mapping)
                .is_some_and(serde_yaml_ng::Mapping::is_empty),
            "catalog must be `{{}}`, not null"
        );
    }

    #[test]
    fn rewrites_emptied_named_catalog_to_empty_map() {
        // Regression: same as above for named catalogs. Reproduces the
        // issue-329 fixture's `react17` group after removing both its
        // entries.
        let dir = tempfile::tempdir().unwrap();
        let content = "catalogs:\n  react17:\n    react: ^17.0.2\n    react-dom: ^17.0.2\n  legacy:\n    is-odd: ^3.0.0\n";
        seed_workspace_file(dir.path(), content);

        let entries = vec![
            make_entry("react", "react17", 3),
            make_entry("react-dom", "react17", 4),
        ];
        let mut fixes = Vec::new();
        apply_catalog_entry_fixes(dir.path(), &entries, OutputFormat::Json, false, &mut fixes);

        let result = std::fs::read_to_string(dir.path().join("pnpm-workspace.yaml")).unwrap();
        assert_eq!(
            result,
            "catalogs:\n  react17: {}\n  legacy:\n    is-odd: ^3.0.0\n",
        );
        let parsed: serde_yaml_ng::Value = serde_yaml_ng::from_str(&result).unwrap();
        let react17 = parsed.get("catalogs").and_then(|c| c.get("react17"));
        assert!(
            react17
                .and_then(serde_yaml_ng::Value::as_mapping)
                .is_some_and(serde_yaml_ng::Mapping::is_empty),
            "react17 must be `{{}}`, not null. Got: {react17:?}"
        );
    }

    #[test]
    fn preserves_non_empty_sibling_named_catalogs() {
        // When one named catalog is emptied but a sibling stays populated,
        // only the emptied one gets the `{}` rewrite.
        let dir = tempfile::tempdir().unwrap();
        let content = "catalogs:\n  react17:\n    react: ^17.0.2\n  vue3:\n    vue: ^3.4.0\n";
        seed_workspace_file(dir.path(), content);

        let entries = vec![make_entry("react", "react17", 3)];
        let mut fixes = Vec::new();
        apply_catalog_entry_fixes(dir.path(), &entries, OutputFormat::Json, false, &mut fixes);

        let result = std::fs::read_to_string(dir.path().join("pnpm-workspace.yaml")).unwrap();
        assert_eq!(
            result,
            "catalogs:\n  react17: {}\n  vue3:\n    vue: ^3.4.0\n"
        );
    }

    #[test]
    fn leaves_partially_populated_catalog_alone() {
        // When only some entries of a catalog are removed and siblings
        // remain, no `{}` rewrite is needed.
        let dir = tempfile::tempdir().unwrap();
        let content = "catalog:\n  is-odd: ^1.0.0\n  is-even: ^1.0.0\n";
        seed_workspace_file(dir.path(), content);

        let entries = vec![make_entry("is-even", "default", 3)];
        let mut fixes = Vec::new();
        apply_catalog_entry_fixes(dir.path(), &entries, OutputFormat::Json, false, &mut fixes);

        let result = std::fs::read_to_string(dir.path().join("pnpm-workspace.yaml")).unwrap();
        assert_eq!(result, "catalog:\n  is-odd: ^1.0.0\n");
    }

    // -- compute_deletion_range unit tests ----------------------------------

    #[test]
    fn deletion_range_scalar_form_spans_one_line() {
        let lines: Vec<&str> = "catalog:\n  is-even: ^1.0.0\n  is-odd: ^1.0.0\n"
            .split('\n')
            .collect();
        let range = compute_deletion_range(&lines, 1);
        assert_eq!(range, 1..2);
    }

    #[test]
    fn deletion_range_object_form_spans_until_indent_drops() {
        let content = "catalog:\n  react:\n    specifier: ^18.2.0\n    publishConfig: {}\n  is-even: ^1.0.0\n";
        let lines: Vec<&str> = content.split('\n').collect();
        let range = compute_deletion_range(&lines, 1);
        assert_eq!(range, 1..4);
    }

    #[test]
    fn deletion_range_stops_at_blank_line() {
        let content = "catalog:\n  is-even: ^1.0.0\n\n  is-odd: ^1.0.0\n";
        let lines: Vec<&str> = content.split('\n').collect();
        let range = compute_deletion_range(&lines, 1);
        assert_eq!(range, 1..2);
    }

    #[test]
    fn is_multi_document_detects_separator() {
        assert!(is_multi_document_yaml("foo: bar\n---\nbaz: qux\n"));
        assert!(is_multi_document_yaml("---\nfoo: bar\n"));
        assert!(!is_multi_document_yaml("catalog:\n  is-even: ^1.0.0\n"));
        // A `---` inside a quoted value or as a substring is not a separator.
        assert!(!is_multi_document_yaml("catalog:\n  foo: \"---\"\n"));
    }
}
