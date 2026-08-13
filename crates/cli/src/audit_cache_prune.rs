//! `fallow audit-cache prune`: run the audit cache GC policy on demand and
//! report every considered entry.
//!
//! The decision logic is [`sweep_reusable_caches_with_report`], the same code
//! path every `fallow audit` run applies silently; this module only resolves
//! the threshold, orders the report, and renders it.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use crate::base_worktree::{
    CacheMaxAgeSource, ResolvedCacheMaxAge, SweepDecision, SweepEntry, SweepMode, SweepPass,
    SweepSizes, log_sweep_entries, resolve_cache_max_age_with_source,
    sweep_reusable_caches_with_report,
};
use crate::report::{format_bytes, plural};
use crate::{emit_error, report};

pub struct AuditCachePruneOptions<'a> {
    pub root: &'a Path,
    pub config_path: Option<&'a PathBuf>,
    pub allow_remote_extends: bool,
    pub dry_run: bool,
    pub max_age_days: Option<u32>,
    pub output: fallow_config::OutputFormat,
    pub json_style: crate::json_style::JsonStyle,
    pub quiet: bool,
}

pub fn run_audit_cache_prune(opts: &AuditCachePruneOptions<'_>) -> ExitCode {
    let resolved = resolve_cache_max_age_with_source(
        opts.root,
        opts.config_path,
        opts.allow_remote_extends,
        opts.max_age_days,
    );
    let scan_root = std::env::temp_dir();
    if let Err(error) = std::fs::read_dir(&scan_root) {
        return emit_error(
            &format!(
                "failed to read the audit cache scan root {}: {error}",
                scan_root.display()
            ),
            2,
            opts.output,
        );
    }
    let mode = if opts.dry_run {
        SweepMode::DryRun
    } else {
        SweepMode::Apply
    };
    let mut report = sweep_reusable_caches_with_report(
        opts.root,
        resolved.max_age,
        &scan_root,
        mode,
        SweepSizes::Measure,
    );
    report
        .entries
        .sort_by(|left, right| left.path.cmp(&right.path));
    log_sweep_entries(&report, mode, resolved.max_age);

    let counts = PruneCounts::tally(&report.entries);
    if matches!(opts.output, fallow_config::OutputFormat::Json) {
        // `--quiet` never suppresses the JSON envelope (`emit_report_json`
        // precedent shared with `audit-cache remove`).
        return emit_prune_json(opts, &scan_root, &resolved, &report.entries, &counts);
    }
    if !opts.quiet {
        print_prune_human(opts, &scan_root, &resolved, &report.entries, &counts);
    }
    ExitCode::SUCCESS
}

/// Aggregate decision counts. `removed + kept + skipped + failed == found ==
/// entries.len()` holds by construction, and per-entry outcomes never move
/// the exit code (a shared runner's foreign entries would otherwise fail
/// every invocation); machine consumers gate on `complete`.
struct PruneCounts {
    found: usize,
    removed: usize,
    kept: usize,
    skipped: usize,
    failed: usize,
    lock_only: usize,
    owner_live: usize,
    /// Legacy pre-#1815 registrations cleared while the cache directory stayed
    /// warm on disk; a subset of `kept`, and never part of `reclaimed_bytes`.
    deregistered: usize,
    reclaimed_bytes: u64,
}

impl PruneCounts {
    fn tally(entries: &[SweepEntry]) -> Self {
        let mut counts = Self {
            found: entries.len(),
            removed: 0,
            kept: 0,
            skipped: 0,
            failed: 0,
            lock_only: 0,
            owner_live: 0,
            deregistered: 0,
            reclaimed_bytes: 0,
        };
        for entry in entries {
            match entry.disposition.decision() {
                SweepDecision::Removed => {
                    counts.removed += 1;
                    counts.reclaimed_bytes = counts
                        .reclaimed_bytes
                        .saturating_add(entry.size_bytes.unwrap_or(0));
                }
                SweepDecision::Kept => counts.kept += 1,
                SweepDecision::Skipped => counts.skipped += 1,
                SweepDecision::Failed => counts.failed += 1,
            }
            match entry.disposition.reason() {
                "lock-only" => counts.lock_only += 1,
                "owner-live" => counts.owner_live += 1,
                "legacy-deregistered" => counts.deregistered += 1,
                _ => {}
            }
        }
        counts
    }

    const fn complete(&self) -> bool {
        self.skipped == 0 && self.failed == 0
    }
}

fn emit_prune_json(
    opts: &AuditCachePruneOptions<'_>,
    scan_root: &Path,
    resolved: &ResolvedCacheMaxAge,
    entries: &[SweepEntry],
    counts: &PruneCounts,
) -> ExitCode {
    let entries_json: Vec<serde_json::Value> = entries
        .iter()
        .map(|entry| {
            serde_json::json!({
                "path": entry.path,
                "pass": entry.pass.as_str(),
                "disposition": entry.disposition.decision().as_str(),
                "reason": entry.disposition.reason(),
                "age_days": entry.age_days,
                "size_bytes": entry.size_bytes,
                "owner_root": entry.owner_root.as_deref().map(sanitize_owner_path),
            })
        })
        .collect();
    let value = serde_json::json!({
        "kind": "audit-cache-prune",
        "schema_version": 1,
        "command": "audit-cache prune",
        "root": opts.root,
        "scan_root": scan_root,
        "dry_run": opts.dry_run,
        "max_age_days": resolved.days,
        "max_age_source": resolved.source.as_str(),
        "entries": entries_json,
        "found": counts.found,
        "removed": counts.removed,
        "kept": counts.kept,
        "skipped": counts.skipped,
        "failed": counts.failed,
        "deregistered": counts.deregistered,
        "complete": counts.complete(),
        "reclaimed_bytes": counts.reclaimed_bytes,
    });
    report::emit_report_json(&value, "audit cache prune", opts.json_style)
}

fn print_prune_human(
    opts: &AuditCachePruneOptions<'_>,
    scan_root: &Path,
    resolved: &ResolvedCacheMaxAge,
    entries: &[SweepEntry],
    counts: &PruneCounts,
) {
    if entries.is_empty() {
        println!(
            "audit cache prune: no cache entries found in {}",
            scan_root.display()
        );
        return;
    }
    println!("{}", header_line(scan_root, resolved));
    for entry in entries {
        // Lock-only leftovers can number in the hundreds (issue #2169); they
        // collapse into one trailing count line instead of one row each.
        if entry.disposition.reason() == "lock-only" {
            continue;
        }
        println!("{}", entry_row(entry, opts.dry_run));
    }
    if counts.lock_only > 0 {
        let s = plural(counts.lock_only);
        println!(
            "  {} lock sidecar{s} with no cache remain (harmless, kept by design)",
            counts.lock_only,
        );
    }
    if counts.owner_live > 0 {
        let plural_entries = if counts.owner_live == 1 {
            "entry"
        } else {
            "entries"
        };
        println!(
            "  kept {} {plural_entries} owned by other live projects; run `fallow audit-cache remove --root <path> --yes` in a project to clear its own cache",
            counts.owner_live,
        );
    }
    if counts.deregistered > 0 {
        let verb = if opts.dry_run {
            "would deregister"
        } else {
            "deregistered"
        };
        let s = plural(counts.deregistered);
        println!(
            "  {verb} {} legacy git registration{s}; the cache stays warm on disk (not counted as reclaimed)",
            counts.deregistered,
        );
    }
    let verb = if opts.dry_run {
        "would reclaim"
    } else {
        "reclaimed"
    };
    let plural_entries = if counts.found == 1 {
        "entry"
    } else {
        "entries"
    };
    println!(
        "{verb} {} across {} of {} {plural_entries}",
        format_bytes(counts.reclaimed_bytes),
        counts.removed,
        counts.found,
    );
}

fn header_line(scan_root: &Path, resolved: &ResolvedCacheMaxAge) -> String {
    let scan_root = scan_root.display();
    if resolved.max_age.is_none() {
        // `git gc --prune=now` trains the opposite expectation for `0`, so the
        // disabled case says exactly what still happens.
        let zero_source = match resolved.source {
            CacheMaxAgeSource::Flag => "--max-age-days 0",
            CacheMaxAgeSource::Env => "FALLOW_AUDIT_CACHE_MAX_AGE_DAYS=0",
            // The built-in default is 30, never 0.
            CacheMaxAgeSource::Config | CacheMaxAgeSource::Default => "audit.cacheMaxAgeDays 0",
        };
        return format!(
            "audit cache prune (age-based reclaim disabled by {zero_source}; reclaiming orphaned entries only) in {scan_root}"
        );
    }
    format!(
        "audit cache prune (threshold {}d from {}) in {scan_root}",
        resolved.days,
        resolved.source.as_str(),
    )
}

fn entry_row(entry: &SweepEntry, dry_run: bool) -> String {
    // The legacy current-path case only clears the git registration; "kept"
    // would hide the mutation and "removed" would claim one that never
    // happens, so the action names the deregistration itself.
    let action = if entry.disposition.reason() == "legacy-deregistered" {
        if dry_run {
            "would deregister"
        } else {
            "deregistered"
        }
    } else {
        match (entry.disposition.decision(), dry_run) {
            (SweepDecision::Removed, true) => "would remove",
            (SweepDecision::Removed, false) => "removed",
            (SweepDecision::Kept, _) => "kept",
            (SweepDecision::Skipped, _) => "skipped",
            (SweepDecision::Failed, _) => "failed",
        }
    };
    let name = entry.path.file_name().map_or_else(
        || entry.path.display().to_string(),
        |name| name.to_string_lossy().into_owned(),
    );
    let mut segments: Vec<String> = Vec::new();
    if let Some(size) = entry.size_bytes {
        segments.push(format_bytes(size));
    }
    match entry.disposition.reason() {
        "aged-out" => {
            push_age(&mut segments, entry);
            if entry.pass == SweepPass::Foreign
                && let Some(owner) = entry.owner_root.as_deref()
            {
                segments.push(format!("owner missing: {}", sanitize_owner_path(owner)));
            }
        }
        "orphaned-sidecars" => segments.push("orphaned sidecars".to_string()),
        "legacy-registered" => segments.push("legacy git registration".to_string()),
        "legacy-deregistered" => {
            segments.push("legacy git registration; cache stays warm on disk".to_string());
        }
        "fresh" => push_age(&mut segments, entry),
        "owner-live" => push_owner(&mut segments, entry, "owner live"),
        "owner-unverifiable" => push_owner(&mut segments, entry, "owner unverifiable"),
        "age-gc-disabled" => segments.push("age-based reclaim disabled".to_string()),
        "grace-seeded" => segments.push("first seen, ages from now".to_string()),
        "recreated" => segments.push("recreated during prune".to_string()),
        "not-owned" => segments.push("owned by another user".to_string()),
        "lock-contention" => segments.push("lock held by another process".to_string()),
        "remove-failed" => segments.push("removal failed".to_string()),
        _ => {}
    }
    let mut row = format!("  {action:<12} {name}");
    for segment in segments {
        row.push_str("  ");
        row.push_str(&segment);
    }
    row
}

fn push_age(segments: &mut Vec<String>, entry: &SweepEntry) {
    if let Some(age) = entry.age_days {
        segments.push(format!("aged {age}d"));
    }
}

fn push_owner(segments: &mut Vec<String>, entry: &SweepEntry, label: &str) {
    if let Some(owner) = entry.owner_root.as_deref() {
        segments.push(format!("{label}: {}", sanitize_owner_path(owner)));
    }
}

/// The recorded owner root is untrusted sidecar content promoted to rendered
/// output: keep the first line only and strip control characters.
fn sanitize_owner_path(owner: &Path) -> String {
    let raw = owner.display().to_string();
    raw.lines()
        .next()
        .unwrap_or_default()
        .chars()
        .filter(|character| !character.is_control())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::base_worktree::SweepDisposition;

    fn entry(disposition: SweepDisposition, size_bytes: Option<u64>) -> SweepEntry {
        SweepEntry {
            path: PathBuf::from("/tmp/fallow-audit-base-cache-aa-root-bb"),
            pass: SweepPass::Foreign,
            disposition,
            age_days: Some(42),
            owner_root: Some(PathBuf::from("/repo/owner")),
            size_bytes,
        }
    }

    #[test]
    fn counts_partition_every_decision_and_close_over_found() {
        let entries = vec![
            entry(SweepDisposition::ReclaimedAged, Some(10)),
            entry(SweepDisposition::ReclaimedOrphan, None),
            entry(SweepDisposition::KeptFresh, Some(5)),
            entry(SweepDisposition::KeptNotOwned, Some(7)),
            entry(SweepDisposition::KeptLockOnly, None),
            entry(SweepDisposition::KeptOwnerLive, Some(3)),
            entry(SweepDisposition::KeptLegacyDeregistered, Some(64)),
            entry(SweepDisposition::SkippedLocked, Some(1)),
            entry(SweepDisposition::RemoveFailed, Some(2)),
        ];
        let counts = PruneCounts::tally(&entries);
        assert_eq!(counts.found, entries.len());
        assert_eq!(
            counts.removed + counts.kept + counts.skipped + counts.failed,
            counts.found,
        );
        assert_eq!(counts.removed, 2);
        assert_eq!(counts.kept, 5);
        assert_eq!(counts.skipped, 1);
        assert_eq!(counts.failed, 1);
        assert_eq!(counts.lock_only, 1);
        assert_eq!(counts.owner_live, 1);
        assert_eq!(counts.deregistered, 1);
        assert_eq!(
            counts.reclaimed_bytes, 10,
            "a deregistered-but-kept legacy entry must not add its size to reclaimed bytes",
        );
        assert!(!counts.complete());
        assert!(PruneCounts::tally(&[entry(SweepDisposition::KeptFresh, None)]).complete());
    }

    #[test]
    fn entry_rows_use_dry_run_wording_and_sanitized_owner() {
        let mut aged = entry(SweepDisposition::ReclaimedAged, Some(1_288_490_188));
        aged.owner_root = Some(PathBuf::from("/repo/owner\nsecond line\u{7}"));
        assert_eq!(
            entry_row(&aged, false),
            "  removed      fallow-audit-base-cache-aa-root-bb  1.2 GiB  aged 42d  owner missing: /repo/owner",
        );
        assert_eq!(
            entry_row(&aged, true),
            "  would remove fallow-audit-base-cache-aa-root-bb  1.2 GiB  aged 42d  owner missing: /repo/owner",
        );
        let seeded = entry(SweepDisposition::KeptGraceSeeded, Some(2048));
        assert_eq!(
            entry_row(&seeded, false),
            "  kept         fallow-audit-base-cache-aa-root-bb  2 KiB  first seen, ages from now",
        );
        let deregistered = entry(SweepDisposition::KeptLegacyDeregistered, Some(2048));
        assert_eq!(
            entry_row(&deregistered, false),
            "  deregistered fallow-audit-base-cache-aa-root-bb  2 KiB  legacy git registration; cache stays warm on disk",
        );
        assert_eq!(
            entry_row(&deregistered, true),
            "  would deregister fallow-audit-base-cache-aa-root-bb  2 KiB  legacy git registration; cache stays warm on disk",
        );
    }
}
