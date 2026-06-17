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

use fallow_config::{CatalogPrecedingCommentPolicy, OutputFormat};
use fallow_core::results::{
    EmptyCatalogGroup, EmptyCatalogGroupFinding, UnusedCatalogEntry, UnusedCatalogEntryFinding,
};

use super::plan::{CapturedHashes, FixPlan, read_source_with_hash_check};

const PNPM_WORKSPACE_FILE: &str = "pnpm-workspace.yaml";

/// Apply unused-catalog-entry fixes to `pnpm-workspace.yaml`.
///
/// Returns `(had_write_error, applied_count, skipped_count)` so the
/// orchestrator can build the top-level fix-output summary. The returned
/// `skipped_count` only counts entries that were intentionally not
/// removed (hardcoded consumer, multi-doc YAML, line out of range); it
/// does NOT count entries that produced a write error.
pub(super) struct CatalogFixContext<'a> {
    pub(super) hashes: &'a CapturedHashes,
    pub(super) plan: &'a mut FixPlan,
    pub(super) output: OutputFormat,
    pub(super) dry_run: bool,
    pub(super) fixes: &'a mut Vec<serde_json::Value>,
}

type CatalogRemoval<'a> = (std::ops::Range<usize>, &'a UnusedCatalogEntry);

pub(super) fn apply_catalog_entry_fixes(
    root: &Path,
    entries: &[UnusedCatalogEntryFinding],
    preceding_comment_policy: CatalogPrecedingCommentPolicy,
    ctx: CatalogFixContext<'_>,
) -> CatalogFixSummary {
    let CatalogFixContext {
        hashes,
        plan,
        output,
        dry_run,
        fixes,
    } = ctx;
    let mut summary = CatalogFixSummary::default();

    if entries.is_empty() {
        return summary;
    }

    let by_path = group_unused_catalog_entries_by_path(entries);

    for (relative_path, file_entries) in by_path {
        if !is_pnpm_catalog_source(relative_path) {
            skip_unsupported_catalog_source_entries(
                &file_entries,
                &mut summary,
                fixes,
                output,
                relative_path,
            );
            continue;
        }

        let absolute = root.join(relative_path);
        let Some((content, meta)) = read_source_with_hash_check(root, &absolute, hashes, plan)
        else {
            continue;
        };

        if is_multi_document_yaml(&content) {
            skip_multi_document_catalog_entries(
                &file_entries,
                &mut summary,
                fixes,
                output,
                relative_path,
            );
            continue;
        }

        let lines: Vec<&str> = content.split(meta.line_ending).collect();

        let to_remove = collect_catalog_entry_removals(&mut CatalogEntryRemovalInput {
            file_entries: &file_entries,
            lines: &lines,
            preceding_comment_policy,
            summary: &mut summary,
            fixes,
            output,
            relative_path,
        });

        if to_remove.is_empty() {
            continue;
        }

        let deduped = dedupe_catalog_removals(to_remove);

        if dry_run {
            record_catalog_removal_dry_run(&deduped, &mut summary, fixes, output, relative_path);
            continue;
        }

        let parent_header_indices: Vec<usize> = deduped
            .iter()
            .filter_map(|(_, entry)| find_parent_header_line(&lines, entry))
            .collect();

        let mut new_lines: Vec<String> = lines.iter().map(ToString::to_string).collect();
        for (range, _) in &deduped {
            new_lines.drain(range.clone());
        }
        rewrite_empty_catalog_parents(&mut new_lines, &parent_header_indices, &deduped);

        let mut new_content = new_lines.join(meta.line_ending);
        if content.ends_with(meta.line_ending) && !new_content.ends_with(meta.line_ending) {
            new_content.push_str(meta.line_ending);
        }

        if serde_yaml_ng::from_str::<serde_yaml_ng::Value>(&new_content).is_err() {
            summary.write_error = true;
            eprintln!(
                "Error: refusing to write {}: post-edit content failed YAML reparse. The file was not modified.",
                relative_path.display(),
            );
            continue;
        }

        plan.stage(
            absolute.clone(),
            super::io::bytes_with_optional_bom(new_content, &meta),
        );

        for (range, entry) in &deduped {
            let mut record = remove_record(entry, range, true, relative_path);
            record["__target"] = serde_json::json!(absolute.display().to_string());
            fixes.push(record);
            let entry_idx = entry.line.saturating_sub(1) as usize;
            summary.comment_lines_removed += entry_idx.saturating_sub(range.start);
        }
        summary.applied += deduped.len();
    }

    summary
}

struct CatalogEntryRemovalInput<'a, 'b> {
    file_entries: &'b [&'a UnusedCatalogEntry],
    lines: &'b [&'b str],
    preceding_comment_policy: CatalogPrecedingCommentPolicy,
    summary: &'b mut CatalogFixSummary,
    fixes: &'b mut Vec<serde_json::Value>,
    output: OutputFormat,
    relative_path: &'b Path,
}

fn collect_catalog_entry_removals<'a>(
    input: &mut CatalogEntryRemovalInput<'a, '_>,
) -> Vec<CatalogRemoval<'a>> {
    let mut to_remove = Vec::new();
    for entry in input.file_entries {
        if !entry.hardcoded_consumers.is_empty() {
            skip_hardcoded_catalog_consumers(
                entry,
                input.summary,
                input.fixes,
                input.output,
                input.relative_path,
            );
            continue;
        }

        let line_idx = entry.line.saturating_sub(1) as usize;
        if line_idx >= input.lines.len() {
            skip_out_of_range_catalog_entry(
                entry,
                input.summary,
                input.fixes,
                input.output,
                input.relative_path,
            );
            continue;
        }

        let range =
            compute_deletion_range(input.lines, line_idx, entry, input.preceding_comment_policy);
        to_remove.push((range, *entry));
    }
    to_remove
}

fn dedupe_catalog_removals(mut to_remove: Vec<CatalogRemoval<'_>>) -> Vec<CatalogRemoval<'_>> {
    to_remove.sort_by(|a, b| {
        b.0.start
            .cmp(&a.0.start)
            .then_with(|| b.0.end.cmp(&a.0.end))
    });

    let mut deduped: Vec<CatalogRemoval<'_>> = Vec::new();
    for (range, entry) in to_remove {
        if let Some((last_range, _)) = deduped.last()
            && last_range.start < range.end
            && range.start < last_range.end
        {
            continue;
        }
        deduped.push((range, entry));
    }
    deduped
}

fn record_catalog_removal_dry_run(
    deduped: &[CatalogRemoval<'_>],
    summary: &mut CatalogFixSummary,
    fixes: &mut Vec<serde_json::Value>,
    output: OutputFormat,
    relative_path: &Path,
) {
    for (range, entry) in deduped {
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
}

fn group_unused_catalog_entries_by_path(
    entries: &[UnusedCatalogEntryFinding],
) -> rustc_hash::FxHashMap<&Path, Vec<&UnusedCatalogEntry>> {
    let mut by_path: rustc_hash::FxHashMap<&Path, Vec<&UnusedCatalogEntry>> =
        rustc_hash::FxHashMap::default();
    for entry in entries {
        let entry = &entry.entry;
        by_path.entry(entry.path.as_path()).or_default().push(entry);
    }
    by_path
}

fn is_pnpm_catalog_source(path: &Path) -> bool {
    path == Path::new(PNPM_WORKSPACE_FILE)
}

fn skip_unsupported_catalog_source_entries(
    entries: &[&UnusedCatalogEntry],
    summary: &mut CatalogFixSummary,
    fixes: &mut Vec<serde_json::Value>,
    output: OutputFormat,
    relative_path: &Path,
) {
    for entry in entries {
        summary.skipped += 1;
        fixes.push(skip_record(
            entry,
            "unsupported_catalog_source",
            "Skipped: fallow fix only edits pnpm-workspace.yaml catalog entries; edit Bun package.json catalogs manually",
            output,
            relative_path,
        ));
    }
}

fn skip_out_of_range_catalog_entry(
    entry: &UnusedCatalogEntry,
    summary: &mut CatalogFixSummary,
    fixes: &mut Vec<serde_json::Value>,
    output: OutputFormat,
    relative_path: &Path,
) {
    summary.skipped += 1;
    fixes.push(skip_record(
        entry,
        "line_out_of_range",
        "Skipped: the reported line is past the end of pnpm-workspace.yaml; the file may have been edited since fallow dead-code ran",
        output,
        relative_path,
    ));
}

fn skip_multi_document_catalog_entries(
    entries: &[&UnusedCatalogEntry],
    summary: &mut CatalogFixSummary,
    fixes: &mut Vec<serde_json::Value>,
    output: OutputFormat,
    relative_path: &Path,
) {
    for entry in entries {
        summary.skipped += 1;
        fixes.push(skip_record(
            entry,
            "multi_document_yaml",
            "Skipped: pnpm-workspace.yaml contains a `---` document separator; fallow fix does not support multi-document YAML",
            output,
            relative_path,
        ));
    }
}

fn skip_hardcoded_catalog_consumers(
    entry: &UnusedCatalogEntry,
    summary: &mut CatalogFixSummary,
    fixes: &mut Vec<serde_json::Value>,
    output: OutputFormat,
    relative_path: &Path,
) {
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
}

/// Apply empty-catalog-group fixes to `pnpm-workspace.yaml`.
///
/// Deletes only the named catalog header line. Comments or blank lines between
/// that header and the next sibling remain in place, matching the conservative
/// comment-preservation policy used by the catalog entry fixer.
pub(super) fn apply_empty_catalog_group_fixes(
    root: &Path,
    groups: &[EmptyCatalogGroupFinding],
    hashes: &CapturedHashes,
    plan: &mut FixPlan,
    output: OutputFormat,
    dry_run: bool,
    fixes: &mut Vec<serde_json::Value>,
) -> CatalogFixSummary {
    let mut summary = CatalogFixSummary::default();

    if groups.is_empty() {
        return summary;
    }

    let by_path = group_empty_catalog_groups_by_path(groups);

    for (relative_path, file_groups) in by_path {
        if !is_pnpm_catalog_source(relative_path) {
            for group in &file_groups {
                summary.skipped += 1;
                fixes.push(skip_group_record(
                    group,
                    "unsupported_catalog_source",
                    "Skipped: fallow fix only edits pnpm-workspace.yaml catalog entries; edit Bun package.json catalogs manually",
                    output,
                    relative_path,
                ));
            }
            continue;
        }

        let absolute = root.join(relative_path);
        let Some((content, meta)) = read_source_with_hash_check(root, &absolute, hashes, plan)
        else {
            continue;
        };

        if is_multi_document_yaml(&content) {
            for group in &file_groups {
                summary.skipped += 1;
                fixes.push(skip_group_record(
                    group,
                    "multi_document_yaml",
                    "Skipped: pnpm-workspace.yaml contains a `---` document separator; fallow fix does not support multi-document YAML",
                    output,
                    relative_path,
                ));
            }
            continue;
        }

        let lines: Vec<&str> = content.split(meta.line_ending).collect();
        let mut to_remove = collect_empty_catalog_group_removals(
            &file_groups,
            &lines,
            &mut summary,
            fixes,
            output,
            relative_path,
        );
        if to_remove.is_empty() {
            continue;
        }

        to_remove.sort_by_key(|(line_idx, _)| std::cmp::Reverse(*line_idx));
        to_remove.dedup_by_key(|(line_idx, _)| *line_idx);

        if dry_run {
            record_empty_catalog_group_dry_run(&to_remove, output, relative_path, fixes);
            summary.applied += to_remove.len();
            continue;
        }

        let mut new_lines: Vec<String> = lines.iter().map(ToString::to_string).collect();
        for (line_idx, _) in &to_remove {
            new_lines.remove(*line_idx);
        }

        let mut new_content = new_lines.join(meta.line_ending);
        if content.ends_with(meta.line_ending) && !new_content.ends_with(meta.line_ending) {
            new_content.push_str(meta.line_ending);
        }

        if serde_yaml_ng::from_str::<serde_yaml_ng::Value>(&new_content).is_err() {
            summary.write_error = true;
            eprintln!(
                "Error: refusing to write {}: post-edit content failed YAML reparse. The file was not modified.",
                relative_path.display(),
            );
            continue;
        }

        plan.stage(
            absolute.clone(),
            super::io::bytes_with_optional_bom(new_content, &meta),
        );

        for (line_idx, group) in &to_remove {
            let mut record = remove_group_record(group, *line_idx, true, relative_path);
            record["__target"] = serde_json::json!(absolute.display().to_string());
            fixes.push(record);
        }
        summary.applied += to_remove.len();
    }

    summary
}

fn group_empty_catalog_groups_by_path(
    groups: &[EmptyCatalogGroupFinding],
) -> rustc_hash::FxHashMap<&Path, Vec<&EmptyCatalogGroup>> {
    let mut by_path: rustc_hash::FxHashMap<&Path, Vec<&EmptyCatalogGroup>> =
        rustc_hash::FxHashMap::default();
    for group in groups {
        let group = &group.group;
        by_path.entry(group.path.as_path()).or_default().push(group);
    }
    by_path
}

fn collect_empty_catalog_group_removals<'a>(
    file_groups: &[&'a EmptyCatalogGroup],
    lines: &[&str],
    summary: &mut CatalogFixSummary,
    fixes: &mut Vec<serde_json::Value>,
    output: OutputFormat,
    relative_path: &Path,
) -> Vec<(usize, &'a EmptyCatalogGroup)> {
    let mut to_remove: Vec<(usize, &EmptyCatalogGroup)> = Vec::new();
    for group in file_groups {
        let line_idx = group.line.saturating_sub(1) as usize;
        if line_idx >= lines.len() {
            summary.skipped += 1;
            fixes.push(skip_group_record(
                group,
                "line_out_of_range",
                "Skipped: the reported line is past the end of pnpm-workspace.yaml; the file may have been edited since fallow dead-code ran",
                output,
                relative_path,
            ));
            continue;
        }
        to_remove.push((line_idx, group));
    }
    to_remove
}

fn record_empty_catalog_group_dry_run(
    to_remove: &[(usize, &EmptyCatalogGroup)],
    output: OutputFormat,
    relative_path: &Path,
    fixes: &mut Vec<serde_json::Value>,
) {
    for (line_idx, group) in to_remove {
        if !matches!(output, OutputFormat::Json) {
            eprintln!(
                "Would remove empty catalog group from {}:{} `{}`",
                relative_path.display(),
                line_idx + 1,
                group.catalog_name,
            );
        }
        fixes.push(remove_group_record(group, *line_idx, false, relative_path));
    }
}

/// Output of `apply_catalog_entry_fixes` consumed by the orchestrator.
#[derive(Debug, Default)]
pub(super) struct CatalogFixSummary {
    pub applied: usize,
    pub skipped: usize,
    pub write_error: bool,
    /// Total leading-comment lines absorbed across all applied fixes.
    /// Surfaced in the human summary so users see that comments were
    /// removed alongside entries (`Fixed N issue(s) (+M comment lines)`).
    pub comment_lines_removed: usize,
}

/// Compute the deletion range `[start, end)` (line indices) for a catalog
/// entry whose key sits on `entry_idx`. Object-form entries
/// (`react:\n    specifier: ^18.2.0`) consume every subsequent line with
/// strictly greater indent. Blank lines and lines at the entry's own
/// indent (or shallower) stop the forward scan: blank lines are
/// conservatively treated as inter-entry whitespace that should be
/// preserved.
///
/// Depending on `preceding_comment_policy`, the range may also extend
/// backward to include a contiguous YAML comment block immediately above
/// the entry.
fn compute_deletion_range(
    lines: &[&str],
    entry_idx: usize,
    entry: &UnusedCatalogEntry,
    preceding_comment_policy: CatalogPrecedingCommentPolicy,
) -> std::ops::Range<usize> {
    let start_idx =
        comment_block_start(lines, entry_idx, entry, preceding_comment_policy).unwrap_or(entry_idx);
    let entry_indent = leading_spaces(lines[entry_idx]);
    let mut end_idx = entry_idx + 1;
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

fn comment_block_start(
    lines: &[&str],
    entry_idx: usize,
    entry: &UnusedCatalogEntry,
    policy: CatalogPrecedingCommentPolicy,
) -> Option<usize> {
    if matches!(policy, CatalogPrecedingCommentPolicy::Never) || entry_idx == 0 {
        return None;
    }

    let entry_indent = leading_spaces(lines[entry_idx]);
    let mut comment_start = entry_idx;
    while comment_start > 0 && is_entry_comment(lines[comment_start - 1], entry_indent) {
        comment_start -= 1;
    }
    if comment_start == entry_idx {
        return None;
    }

    let block = &lines[comment_start..entry_idx];
    if block.iter().any(|line| line.contains("fallow-keep")) {
        return None;
    }

    match policy {
        CatalogPrecedingCommentPolicy::Always => Some(comment_start),
        CatalogPrecedingCommentPolicy::Never => None,
        CatalogPrecedingCommentPolicy::Auto => {
            if block.iter().any(|line| is_section_banner_line(line)) {
                return None;
            }
            let before_comment = comment_start.checked_sub(1)?;
            if lines[before_comment].trim().is_empty()
                || find_parent_header_line(lines, entry) == Some(before_comment)
            {
                Some(comment_start)
            } else {
                None
            }
        }
    }
}

fn is_entry_comment(line: &str, entry_indent: usize) -> bool {
    leading_spaces(line) == entry_indent && line.trim_start().starts_with('#')
}

/// Recognize banner-shaped comment lines like `# ====`, `# ----`, `# ====
/// React 18 pins ====`. Returns true when the comment body (after `#` and
/// optional leading whitespace) starts with 3+ repeats of `=`, `-`, `*`,
/// `_`, `~`, `+`, or `#`. Used by the Auto policy to preserve section
/// dividers above the next catalog entry.
fn is_section_banner_line(line: &str) -> bool {
    let Some(after_hash) = line.trim_start().strip_prefix('#') else {
        return false;
    };
    let body = after_hash.trim_start();
    let Some(first) = body.chars().next() else {
        return false;
    };
    if !matches!(first, '=' | '-' | '*' | '_' | '~' | '+' | '#') {
        return false;
    }
    body.chars().take(3).all(|c| c == first)
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
    let mut unique_parents: Vec<usize> = parent_indices.to_vec();
    unique_parents.sort_unstable();
    unique_parents.dedup();

    for parent_pre_idx in unique_parents {
        let deleted_before: usize = deleted_ranges
            .iter()
            .map(|(range, _)| {
                if range.end <= parent_pre_idx {
                    range.end - range.start
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
                    .map(|p| serde_json::Value::String(p.to_string_lossy().replace('\\', "/")))
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
        "entry_line": entry.line,
        "removed_lines": removed_lines,
    });
    if applied && let serde_json::Value::Object(map) = &mut value {
        map.insert("applied".to_string(), serde_json::Value::Bool(true));
    }
    value
}

fn skip_group_record(
    group: &EmptyCatalogGroup,
    skip_reason: &str,
    description: &str,
    output: OutputFormat,
    relative_path: &Path,
) -> serde_json::Value {
    if !matches!(output, OutputFormat::Json) {
        eprintln!(
            "Skipped empty catalog group {}:{} `{}` ({skip_reason})",
            relative_path.display(),
            group.line,
            group.catalog_name,
        );
    }
    serde_json::json!({
        "type": "remove_empty_catalog_group",
        "catalog_name": group.catalog_name,
        "file": relative_path.to_string_lossy().replace('\\', "/"),
        "line": group.line,
        "applied": false,
        "skipped": true,
        "skip_reason": skip_reason,
        "description": description,
    })
}

fn remove_group_record(
    group: &EmptyCatalogGroup,
    line_idx: usize,
    applied: bool,
    relative_path: &Path,
) -> serde_json::Value {
    let mut value = serde_json::json!({
        "type": "remove_empty_catalog_group",
        "catalog_name": group.catalog_name,
        "file": relative_path.to_string_lossy().replace('\\', "/"),
        "line": line_idx + 1,
        "removed_lines": 1,
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

    fn make_entry(name: &str, catalog: &str, line: u32) -> UnusedCatalogEntryFinding {
        UnusedCatalogEntryFinding::with_actions(UnusedCatalogEntry {
            entry_name: name.to_string(),
            catalog_name: catalog.to_string(),
            path: PathBuf::from("pnpm-workspace.yaml"),
            line,
            hardcoded_consumers: vec![],
        })
    }

    fn make_entry_with_consumers(
        name: &str,
        catalog: &str,
        line: u32,
        consumers: Vec<PathBuf>,
    ) -> UnusedCatalogEntryFinding {
        UnusedCatalogEntryFinding::with_actions(UnusedCatalogEntry {
            entry_name: name.to_string(),
            catalog_name: catalog.to_string(),
            path: PathBuf::from("pnpm-workspace.yaml"),
            line,
            hardcoded_consumers: consumers,
        })
    }

    fn make_package_json_entry(name: &str, catalog: &str, line: u32) -> UnusedCatalogEntryFinding {
        UnusedCatalogEntryFinding::with_actions(UnusedCatalogEntry {
            entry_name: name.to_string(),
            catalog_name: catalog.to_string(),
            path: PathBuf::from("package.json"),
            line,
            hardcoded_consumers: vec![],
        })
    }

    fn make_group(name: &str, line: u32) -> EmptyCatalogGroupFinding {
        EmptyCatalogGroupFinding::with_actions(EmptyCatalogGroup {
            catalog_name: name.to_string(),
            path: PathBuf::from("pnpm-workspace.yaml"),
            line,
        })
    }

    fn make_package_json_group(name: &str, line: u32) -> EmptyCatalogGroupFinding {
        EmptyCatalogGroupFinding::with_actions(EmptyCatalogGroup {
            catalog_name: name.to_string(),
            path: PathBuf::from("package.json"),
            line,
        })
    }

    fn seed_workspace_file(root: &Path, content: &str) {
        let path = root.join("pnpm-workspace.yaml");
        std::fs::write(&path, content).unwrap();
    }

    fn seed_package_json(root: &Path, content: &str) {
        let path = root.join("package.json");
        std::fs::write(&path, content).unwrap();
    }

    /// Thin wrappers preserving the pre-#454 test API surface: build a
    /// FixPlan + CapturedHashes around the entry-fix / group-fix call
    /// and commit. Commit failures fold into `summary.write_error` so
    /// pre-existing tests that assert on that field keep working.
    fn run_catalog_entry_fix(
        root: &Path,
        entries: &[UnusedCatalogEntryFinding],
        policy: CatalogPrecedingCommentPolicy,
        output: OutputFormat,
        dry_run: bool,
        fixes: &mut Vec<serde_json::Value>,
    ) -> CatalogFixSummary {
        let mut plan = FixPlan::new();
        let hashes = CapturedHashes::default();
        let mut summary = apply_catalog_entry_fixes(
            root,
            entries,
            policy,
            CatalogFixContext {
                hashes: &hashes,
                plan: &mut plan,
                output,
                dry_run,
                fixes,
            },
        );
        if !dry_run && !plan.commit().failed.is_empty() {
            summary.write_error = true;
        }
        summary
    }

    fn run_empty_catalog_group_fix(
        root: &Path,
        groups: &[EmptyCatalogGroupFinding],
        output: OutputFormat,
        dry_run: bool,
        fixes: &mut Vec<serde_json::Value>,
    ) -> CatalogFixSummary {
        let mut plan = FixPlan::new();
        let hashes = CapturedHashes::default();
        let mut summary = apply_empty_catalog_group_fixes(
            root, groups, &hashes, &mut plan, output, dry_run, fixes,
        );
        if !dry_run && !plan.commit().failed.is_empty() {
            summary.write_error = true;
        }
        summary
    }

    #[test]
    fn removes_empty_named_catalog_group_header_only() {
        let dir = tempfile::tempdir().unwrap();
        let content = "catalogs:\n  react17: {}\n  # keep this note\n  vue3:\n    vue: ^3.4.0\n";
        seed_workspace_file(dir.path(), content);
        let groups = vec![make_group("react17", 2)];
        let mut fixes = Vec::new();

        let summary =
            run_empty_catalog_group_fix(dir.path(), &groups, OutputFormat::Json, false, &mut fixes);

        assert!(!summary.write_error);
        assert_eq!(summary.applied, 1);
        let result = std::fs::read_to_string(dir.path().join("pnpm-workspace.yaml")).unwrap();
        assert_eq!(
            result,
            "catalogs:\n  # keep this note\n  vue3:\n    vue: ^3.4.0\n"
        );
        assert_eq!(fixes[0]["type"], "remove_empty_catalog_group");
        assert_eq!(fixes[0]["applied"], true);
    }

    #[test]
    fn removes_scalar_form_entry() {
        let dir = tempfile::tempdir().unwrap();
        let content = "catalog:\n  is-odd: ^1.0.0\n  is-even: ^1.0.0\n  left-pad: ^1.0.0\n";
        seed_workspace_file(dir.path(), content);

        let entries = vec![make_entry("is-even", "default", 3)];
        let mut fixes = Vec::new();
        let summary = run_catalog_entry_fix(
            dir.path(),
            &entries,
            CatalogPrecedingCommentPolicy::Auto,
            OutputFormat::Json,
            false,
            &mut fixes,
        );

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
    fn skips_bun_package_json_catalog_entries_without_mutating_json() {
        let dir = tempfile::tempdir().unwrap();
        let content = "{\n  \"workspaces\": {\n    \"catalog\": {\n      \"unused\": \"^1.0.0\"\n    }\n  }\n}\n";
        seed_package_json(dir.path(), content);

        let entries = vec![make_package_json_entry("unused", "default", 4)];
        let mut fixes = Vec::new();
        let summary = run_catalog_entry_fix(
            dir.path(),
            &entries,
            CatalogPrecedingCommentPolicy::Auto,
            OutputFormat::Json,
            false,
            &mut fixes,
        );

        assert_eq!(summary.applied, 0);
        assert_eq!(summary.skipped, 1);
        assert!(!summary.write_error);
        assert_eq!(
            std::fs::read_to_string(dir.path().join("package.json")).unwrap(),
            content
        );
        assert_eq!(fixes[0]["skip_reason"], "unsupported_catalog_source");
        assert_eq!(fixes[0]["file"], "package.json");
    }

    #[test]
    fn skips_bun_package_json_empty_catalog_groups_without_mutating_json() {
        let dir = tempfile::tempdir().unwrap();
        let content =
            "{\n  \"workspaces\": {\n    \"catalogs\": {\n      \"empty\": {}\n    }\n  }\n}\n";
        seed_package_json(dir.path(), content);

        let groups = vec![make_package_json_group("empty", 4)];
        let mut fixes = Vec::new();
        let summary =
            run_empty_catalog_group_fix(dir.path(), &groups, OutputFormat::Json, false, &mut fixes);

        assert_eq!(summary.applied, 0);
        assert_eq!(summary.skipped, 1);
        assert!(!summary.write_error);
        assert_eq!(
            std::fs::read_to_string(dir.path().join("package.json")).unwrap(),
            content
        );
        assert_eq!(fixes[0]["skip_reason"], "unsupported_catalog_source");
        assert_eq!(fixes[0]["file"], "package.json");
    }

    #[test]
    fn removes_object_form_entry_with_nested_keys() {
        let dir = tempfile::tempdir().unwrap();
        let content = "catalog:\n  is-odd: ^1.0.0\n  react:\n    specifier: ^18.2.0\n    publishConfig:\n      access: public\n  is-even: ^1.0.0\n";
        seed_workspace_file(dir.path(), content);

        let entries = vec![make_entry("react", "default", 3)];
        let mut fixes = Vec::new();
        let summary = run_catalog_entry_fix(
            dir.path(),
            &entries,
            CatalogPrecedingCommentPolicy::Auto,
            OutputFormat::Json,
            false,
            &mut fixes,
        );

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
        let summary = run_catalog_entry_fix(
            dir.path(),
            &entries,
            CatalogPrecedingCommentPolicy::Auto,
            OutputFormat::Json,
            false,
            &mut fixes,
        );

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
        let summary = run_catalog_entry_fix(
            dir.path(),
            &entries,
            CatalogPrecedingCommentPolicy::Auto,
            OutputFormat::Json,
            true,
            &mut fixes,
        );

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
        let summary = run_catalog_entry_fix(
            dir.path(),
            &entries,
            CatalogPrecedingCommentPolicy::Auto,
            OutputFormat::Json,
            false,
            &mut fixes,
        );

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
        run_catalog_entry_fix(
            dir.path(),
            &entries,
            CatalogPrecedingCommentPolicy::Auto,
            OutputFormat::Json,
            false,
            &mut fixes,
        );

        let result = std::fs::read_to_string(dir.path().join("pnpm-workspace.yaml")).unwrap();
        assert_eq!(result, "catalog:\n  is-odd: ^1.0.0 # keep me\n");
    }

    #[test]
    fn auto_deletes_leading_comment_after_parent_header() {
        let dir = tempfile::tempdir().unwrap();
        let content = "catalog:\n  # mention is-even\n  is-even: ^1.0.0\n  is-odd: ^1.0.0\n";
        seed_workspace_file(dir.path(), content);

        let entries = vec![make_entry("is-even", "default", 3)];
        let mut fixes = Vec::new();
        let summary = run_catalog_entry_fix(
            dir.path(),
            &entries,
            CatalogPrecedingCommentPolicy::Auto,
            OutputFormat::Json,
            false,
            &mut fixes,
        );

        let result = std::fs::read_to_string(dir.path().join("pnpm-workspace.yaml")).unwrap();
        assert_eq!(result, "catalog:\n  is-odd: ^1.0.0\n");
        assert_eq!(fixes[0]["line"], serde_json::json!(2));
        assert_eq!(fixes[0]["entry_line"], serde_json::json!(3));
        assert_eq!(fixes[0]["removed_lines"], serde_json::json!(2));
        assert_eq!(summary.comment_lines_removed, 1);
    }

    #[test]
    fn auto_preserves_block_with_fallow_keep_marker() {
        let dir = tempfile::tempdir().unwrap();
        let content = "catalog:\n  # fallow-keep: audit trail for CVE-2024-XXXX\n  is-even: ^1.0.0\n  is-odd: ^1.0.0\n";
        seed_workspace_file(dir.path(), content);

        let entries = vec![make_entry("is-even", "default", 3)];
        let mut fixes = Vec::new();
        let summary = run_catalog_entry_fix(
            dir.path(),
            &entries,
            CatalogPrecedingCommentPolicy::Auto,
            OutputFormat::Json,
            false,
            &mut fixes,
        );

        let result = std::fs::read_to_string(dir.path().join("pnpm-workspace.yaml")).unwrap();
        assert_eq!(
            result,
            "catalog:\n  # fallow-keep: audit trail for CVE-2024-XXXX\n  is-odd: ^1.0.0\n"
        );
        assert_eq!(summary.comment_lines_removed, 0);
    }

    #[test]
    fn always_preserves_block_with_fallow_keep_marker() {
        let dir = tempfile::tempdir().unwrap();
        let content = "catalog:\n  # fallow-keep\n  is-even: ^1.0.0\n  is-odd: ^1.0.0\n";
        seed_workspace_file(dir.path(), content);

        let entries = vec![make_entry("is-even", "default", 3)];
        let mut fixes = Vec::new();
        run_catalog_entry_fix(
            dir.path(),
            &entries,
            CatalogPrecedingCommentPolicy::Always,
            OutputFormat::Json,
            false,
            &mut fixes,
        );

        let result = std::fs::read_to_string(dir.path().join("pnpm-workspace.yaml")).unwrap();
        assert_eq!(result, "catalog:\n  # fallow-keep\n  is-odd: ^1.0.0\n");
    }

    #[test]
    fn auto_preserves_section_banner_block() {
        let dir = tempfile::tempdir().unwrap();
        let content =
            "catalog:\n  # === React 18 production pins ===\n  is-even: ^1.0.0\n  is-odd: ^1.0.0\n";
        seed_workspace_file(dir.path(), content);

        let entries = vec![make_entry("is-even", "default", 3)];
        let mut fixes = Vec::new();
        let summary = run_catalog_entry_fix(
            dir.path(),
            &entries,
            CatalogPrecedingCommentPolicy::Auto,
            OutputFormat::Json,
            false,
            &mut fixes,
        );

        let result = std::fs::read_to_string(dir.path().join("pnpm-workspace.yaml")).unwrap();
        assert_eq!(
            result,
            "catalog:\n  # === React 18 production pins ===\n  is-odd: ^1.0.0\n"
        );
        assert_eq!(summary.comment_lines_removed, 0);
    }

    #[test]
    fn always_deletes_section_banner_block() {
        let dir = tempfile::tempdir().unwrap();
        let content = "catalog:\n  # ====\n  is-even: ^1.0.0\n  is-odd: ^1.0.0\n";
        seed_workspace_file(dir.path(), content);

        let entries = vec![make_entry("is-even", "default", 3)];
        let mut fixes = Vec::new();
        run_catalog_entry_fix(
            dir.path(),
            &entries,
            CatalogPrecedingCommentPolicy::Always,
            OutputFormat::Json,
            false,
            &mut fixes,
        );

        let result = std::fs::read_to_string(dir.path().join("pnpm-workspace.yaml")).unwrap();
        assert_eq!(result, "catalog:\n  is-odd: ^1.0.0\n");
    }

    #[test]
    fn section_banner_detector_recognizes_separator_runs() {
        assert!(is_section_banner_line("# === banner ==="));
        assert!(is_section_banner_line("  # ----"));
        assert!(is_section_banner_line("# ***"));
        assert!(is_section_banner_line("# ___"));
        assert!(is_section_banner_line("#==="));
        assert!(!is_section_banner_line("# mention is-even"));
        assert!(!is_section_banner_line("# = single sep"));
        assert!(!is_section_banner_line("# -- two seps only"));
        assert!(!is_section_banner_line("not a comment"));
    }

    #[test]
    fn auto_deletes_leading_comment_after_blank_separator() {
        let dir = tempfile::tempdir().unwrap();
        let content = "catalog:\n  is-odd: ^1.0.0\n\n  # mention is-even\n  is-even: ^1.0.0\n";
        seed_workspace_file(dir.path(), content);

        let entries = vec![make_entry("is-even", "default", 5)];
        let mut fixes = Vec::new();
        run_catalog_entry_fix(
            dir.path(),
            &entries,
            CatalogPrecedingCommentPolicy::Auto,
            OutputFormat::Json,
            false,
            &mut fixes,
        );

        let result = std::fs::read_to_string(dir.path().join("pnpm-workspace.yaml")).unwrap();
        assert_eq!(result, "catalog:\n  is-odd: ^1.0.0\n\n");
    }

    #[test]
    fn auto_preserves_leading_comment_after_sibling_entry() {
        let dir = tempfile::tempdir().unwrap();
        let content = "catalog:\n  is-odd: ^1.0.0\n  # shared note\n  is-even: ^1.0.0\n";
        seed_workspace_file(dir.path(), content);

        let entries = vec![make_entry("is-even", "default", 4)];
        let mut fixes = Vec::new();
        run_catalog_entry_fix(
            dir.path(),
            &entries,
            CatalogPrecedingCommentPolicy::Auto,
            OutputFormat::Json,
            false,
            &mut fixes,
        );

        let result = std::fs::read_to_string(dir.path().join("pnpm-workspace.yaml")).unwrap();
        assert_eq!(result, "catalog:\n  is-odd: ^1.0.0\n  # shared note\n");
    }

    #[test]
    fn auto_deletes_named_catalog_leading_comment_after_named_header() {
        let dir = tempfile::tempdir().unwrap();
        let content = "catalogs:\n  react17:\n    # pinned for old peer deps\n    react: ^17.0.2\n    react-dom: ^17.0.2\n";
        seed_workspace_file(dir.path(), content);

        let entries = vec![make_entry("react", "react17", 4)];
        let mut fixes = Vec::new();
        run_catalog_entry_fix(
            dir.path(),
            &entries,
            CatalogPrecedingCommentPolicy::Auto,
            OutputFormat::Json,
            false,
            &mut fixes,
        );

        let result = std::fs::read_to_string(dir.path().join("pnpm-workspace.yaml")).unwrap();
        assert_eq!(result, "catalogs:\n  react17:\n    react-dom: ^17.0.2\n");
    }

    #[test]
    fn always_deletes_leading_comment_after_sibling_entry() {
        let dir = tempfile::tempdir().unwrap();
        let content = "catalog:\n  is-odd: ^1.0.0\n  # force remove\n  is-even: ^1.0.0\n";
        seed_workspace_file(dir.path(), content);

        let entries = vec![make_entry("is-even", "default", 4)];
        let mut fixes = Vec::new();
        run_catalog_entry_fix(
            dir.path(),
            &entries,
            CatalogPrecedingCommentPolicy::Always,
            OutputFormat::Json,
            false,
            &mut fixes,
        );

        let result = std::fs::read_to_string(dir.path().join("pnpm-workspace.yaml")).unwrap();
        assert_eq!(result, "catalog:\n  is-odd: ^1.0.0\n");
    }

    #[test]
    fn never_preserves_leading_comment_after_parent_header() {
        let dir = tempfile::tempdir().unwrap();
        let content = "catalog:\n  # keep always\n  is-even: ^1.0.0\n  is-odd: ^1.0.0\n";
        seed_workspace_file(dir.path(), content);

        let entries = vec![make_entry("is-even", "default", 3)];
        let mut fixes = Vec::new();
        run_catalog_entry_fix(
            dir.path(),
            &entries,
            CatalogPrecedingCommentPolicy::Never,
            OutputFormat::Json,
            false,
            &mut fixes,
        );

        let result = std::fs::read_to_string(dir.path().join("pnpm-workspace.yaml")).unwrap();
        assert_eq!(result, "catalog:\n  # keep always\n  is-odd: ^1.0.0\n");
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
        let summary = run_catalog_entry_fix(
            dir.path(),
            &entries,
            CatalogPrecedingCommentPolicy::Auto,
            OutputFormat::Json,
            false,
            &mut fixes,
        );

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
        let summary = run_catalog_entry_fix(
            dir.path(),
            &entries,
            CatalogPrecedingCommentPolicy::Auto,
            OutputFormat::Json,
            false,
            &mut fixes,
        );

        assert_eq!(summary.applied, 0);
        assert_eq!(summary.skipped, 1);
        assert_eq!(
            fixes[0]["skip_reason"],
            serde_json::json!("multi_document_yaml")
        );

        let result = std::fs::read_to_string(dir.path().join("pnpm-workspace.yaml")).unwrap();
        assert_eq!(result, content);
    }

    #[test]
    fn skips_when_line_out_of_range() {
        let dir = tempfile::tempdir().unwrap();
        let content = "catalog:\n  is-even: ^1.0.0\n";
        seed_workspace_file(dir.path(), content);

        let entries = vec![make_entry("is-even", "default", 99)];
        let mut fixes = Vec::new();
        let summary = run_catalog_entry_fix(
            dir.path(),
            &entries,
            CatalogPrecedingCommentPolicy::Auto,
            OutputFormat::Json,
            false,
            &mut fixes,
        );

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
        run_catalog_entry_fix(
            dir.path(),
            &entries,
            CatalogPrecedingCommentPolicy::Auto,
            OutputFormat::Json,
            false,
            &mut fixes,
        );

        let result = std::fs::read_to_string(dir.path().join("pnpm-workspace.yaml")).unwrap();
        assert_eq!(result, "catalog:\r\n  is-odd: ^1.0.0\r\n");
    }

    #[test]
    fn rewrites_emptied_default_catalog_to_empty_map() {
        let dir = tempfile::tempdir().unwrap();
        let content = "catalog:\n  is-even: ^1.0.0\n";
        seed_workspace_file(dir.path(), content);

        let entries = vec![make_entry("is-even", "default", 2)];
        let mut fixes = Vec::new();
        run_catalog_entry_fix(
            dir.path(),
            &entries,
            CatalogPrecedingCommentPolicy::Auto,
            OutputFormat::Json,
            false,
            &mut fixes,
        );

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
        let dir = tempfile::tempdir().unwrap();
        let content = "catalogs:\n  react17:\n    react: ^17.0.2\n    react-dom: ^17.0.2\n  legacy:\n    is-odd: ^3.0.0\n";
        seed_workspace_file(dir.path(), content);

        let entries = vec![
            make_entry("react", "react17", 3),
            make_entry("react-dom", "react17", 4),
        ];
        let mut fixes = Vec::new();
        run_catalog_entry_fix(
            dir.path(),
            &entries,
            CatalogPrecedingCommentPolicy::Auto,
            OutputFormat::Json,
            false,
            &mut fixes,
        );

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
        let dir = tempfile::tempdir().unwrap();
        let content = "catalogs:\n  react17:\n    react: ^17.0.2\n  vue3:\n    vue: ^3.4.0\n";
        seed_workspace_file(dir.path(), content);

        let entries = vec![make_entry("react", "react17", 3)];
        let mut fixes = Vec::new();
        run_catalog_entry_fix(
            dir.path(),
            &entries,
            CatalogPrecedingCommentPolicy::Auto,
            OutputFormat::Json,
            false,
            &mut fixes,
        );

        let result = std::fs::read_to_string(dir.path().join("pnpm-workspace.yaml")).unwrap();
        assert_eq!(
            result,
            "catalogs:\n  react17: {}\n  vue3:\n    vue: ^3.4.0\n"
        );
    }

    #[test]
    fn leaves_partially_populated_catalog_alone() {
        let dir = tempfile::tempdir().unwrap();
        let content = "catalog:\n  is-odd: ^1.0.0\n  is-even: ^1.0.0\n";
        seed_workspace_file(dir.path(), content);

        let entries = vec![make_entry("is-even", "default", 3)];
        let mut fixes = Vec::new();
        run_catalog_entry_fix(
            dir.path(),
            &entries,
            CatalogPrecedingCommentPolicy::Auto,
            OutputFormat::Json,
            false,
            &mut fixes,
        );

        let result = std::fs::read_to_string(dir.path().join("pnpm-workspace.yaml")).unwrap();
        assert_eq!(result, "catalog:\n  is-odd: ^1.0.0\n");
    }

    #[test]
    fn deletion_range_scalar_form_spans_one_line() {
        let lines: Vec<&str> = "catalog:\n  is-even: ^1.0.0\n  is-odd: ^1.0.0\n"
            .split('\n')
            .collect();
        let entry = make_entry("is-even", "default", 2).entry;
        let range = compute_deletion_range(&lines, 1, &entry, CatalogPrecedingCommentPolicy::Auto);
        assert_eq!(range, 1..2);
    }

    #[test]
    fn deletion_range_object_form_spans_until_indent_drops() {
        let content = "catalog:\n  react:\n    specifier: ^18.2.0\n    publishConfig: {}\n  is-even: ^1.0.0\n";
        let lines: Vec<&str> = content.split('\n').collect();
        let entry = make_entry("react", "default", 2).entry;
        let range = compute_deletion_range(&lines, 1, &entry, CatalogPrecedingCommentPolicy::Auto);
        assert_eq!(range, 1..4);
    }

    #[test]
    fn deletion_range_stops_at_blank_line() {
        let content = "catalog:\n  is-even: ^1.0.0\n\n  is-odd: ^1.0.0\n";
        let lines: Vec<&str> = content.split('\n').collect();
        let entry = make_entry("is-even", "default", 2).entry;
        let range = compute_deletion_range(&lines, 1, &entry, CatalogPrecedingCommentPolicy::Auto);
        assert_eq!(range, 1..2);
    }

    #[test]
    fn is_multi_document_detects_separator() {
        assert!(is_multi_document_yaml("foo: bar\n---\nbaz: qux\n"));
        assert!(is_multi_document_yaml("---\nfoo: bar\n"));
        assert!(!is_multi_document_yaml("catalog:\n  is-even: ^1.0.0\n"));
        assert!(!is_multi_document_yaml("catalog:\n  foo: \"---\"\n"));
    }
}
