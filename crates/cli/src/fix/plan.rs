//! Batch-atomicity layer for `fallow fix`.
//!
//! Fixers stage writes into a shared [`FixPlan`], then the orchestrator
//! commits them atomically per file and records any skips.

use std::path::{Path, PathBuf};

use rustc_hash::{FxHashMap, FxHashSet};
use tempfile::NamedTempFile;

/// One file's staged content.
struct PlannedWrite {
    path: PathBuf,
    content: Vec<u8>,
}

/// Why a file was skipped during a fix run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum SkipReason {
    /// The file changed after analysis, so the offsets are stale.
    ContentChanged,
    /// The file mixes CRLF and LF line endings.
    MixedLineEndings,
    /// Conservative skip for files whose consumers may be off-graph.
    LowConfidenceOffGraph,
    /// Conservative skip for files with unresolved imports.
    LowConfidenceUnresolvedImports,
}

impl SkipReason {
    pub(super) fn as_wire_str(self) -> &'static str {
        match self {
            Self::ContentChanged => "content_changed",
            Self::MixedLineEndings => "mixed_line_endings",
            Self::LowConfidenceOffGraph => "low_confidence_off_graph",
            Self::LowConfidenceUnresolvedImports => "low_confidence_unresolved_imports",
        }
    }

    /// True for conservative skips rather than recoverable failures.
    pub(super) fn is_intentional(self) -> bool {
        matches!(
            self,
            Self::LowConfidenceOffGraph | Self::LowConfidenceUnresolvedImports
        )
    }

    pub(super) fn human_message(self, path: &Path) -> String {
        match self {
            Self::ContentChanged => format!(
                "Skipping {}: file content changed since `fallow dead-code` ran. Re-run `fallow fix` to refresh the analysis first.",
                path.display(),
            ),
            Self::MixedLineEndings => format!(
                "Skipping {}: file has mixed CRLF/LF line endings. Normalize it, then re-run `fallow fix`.",
                path.display(),
            ),
            Self::LowConfidenceOffGraph => format!(
                "Kept unused export(s) in {}: consumer coverage is incomplete, so the export was preserved.",
                path.display(),
            ),
            Self::LowConfidenceUnresolvedImports => format!(
                "Kept unused export(s) in {}: unresolved imports make the usage graph incomplete.",
                path.display(),
            ),
        }
    }
}

/// One file's skip record.
pub(super) struct SkippedFile {
    pub path: PathBuf,
    pub reason: SkipReason,
}

/// Outcome of [`FixPlan::commit`].
pub(super) struct CommitOutcome {
    /// Absolute paths whose new content landed on disk.
    #[allow(
        dead_code,
        reason = "test-only reader; `#[expect]` is unfulfilled under `--all-targets` because the test cfg satisfies dead_code while the lib cfg would fire it"
    )]
    pub written: FxHashSet<PathBuf>,
    /// Per-path errors.
    pub failed: Vec<(PathBuf, std::io::Error)>,
}

impl CommitOutcome {
    fn empty() -> Self {
        Self {
            written: FxHashSet::default(),
            failed: Vec::new(),
        }
    }
}

/// Accumulator for batched writes during a `fallow fix` run.
pub(super) struct FixPlan {
    entries: Vec<PlannedWrite>,
    skipped: Vec<SkippedFile>,
}

impl FixPlan {
    pub(super) fn new() -> Self {
        Self {
            entries: Vec::new(),
            skipped: Vec::new(),
        }
    }

    /// Queue a write. The last call for a path wins.
    pub(super) fn stage(&mut self, path: PathBuf, content: Vec<u8>) {
        if let Some(existing) = self.entries.iter_mut().find(|e| e.path == path) {
            existing.content = content;
            return;
        }
        self.entries.push(PlannedWrite { path, content });
    }

    /// Return the currently-staged content for `path`, if any.
    pub(super) fn staged_content(&self, path: &Path) -> Option<&[u8]> {
        self.entries
            .iter()
            .find(|e| e.path == path)
            .map(|e| e.content.as_slice())
    }

    /// Record that a file was skipped. Deduped on `(path, reason)`.
    pub(super) fn skip(&mut self, path: PathBuf, reason: SkipReason) {
        if self
            .skipped
            .iter()
            .any(|existing| existing.path == path && existing.reason == reason)
        {
            return;
        }
        self.skipped.push(SkippedFile { path, reason });
    }

    pub(super) fn skipped(&self) -> &[SkippedFile] {
        &self.skipped
    }

    #[allow(
        dead_code,
        reason = "test-only consumer; same reason as `written` above"
    )]
    pub(super) fn entries_paths(&self) -> impl Iterator<Item = &Path> {
        self.entries.iter().map(|e| e.path.as_path())
    }

    /// Stage every entry to a sibling `NamedTempFile`, then promote each to
    /// its final path.
    pub(super) fn commit(self) -> CommitOutcome {
        if self.entries.is_empty() {
            return CommitOutcome::empty();
        }

        let mut staged: Vec<StagedEntry> = Vec::with_capacity(self.entries.len());
        for entry in self.entries {
            match stage_one(&entry.path, &entry.content) {
                Ok(stage) => staged.push(stage),
                Err(e) => {
                    return CommitOutcome {
                        written: FxHashSet::default(),
                        failed: vec![(entry.path, e)],
                    };
                }
            }
        }

        staged.sort_by(|a, b| a.requested.cmp(&b.requested));

        let mut written = FxHashSet::default();
        let mut failed = Vec::new();
        for stage in staged {
            match stage.handle.persist(&stage.resolved) {
                Ok(_) => {
                    written.insert(stage.requested);
                }
                Err(err) => {
                    failed.push((stage.requested, err.error));
                }
            }
        }

        CommitOutcome { written, failed }
    }
}

/// One staged write: a `NamedTempFile` plus the absolute paths the
/// caller asked for (`requested`) and the symlink-resolved path the
/// rename will actually write through (`resolved`). Tracking both is
/// required so the rename writes through symlinks (matching
/// `fallow_config::atomic_write`) while user-facing reporting still
/// references the path the user knows.
struct StagedEntry {
    handle: NamedTempFile,
    requested: PathBuf,
    resolved: PathBuf,
}

fn stage_one(target: &Path, content: &[u8]) -> std::io::Result<StagedEntry> {
    let resolved = std::fs::canonicalize(target).unwrap_or_else(|_| target.to_path_buf());
    let dir = resolved.parent().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "fix plan target has no parent directory",
        )
    })?;
    let mut handle = NamedTempFile::new_in(dir)?;
    use std::io::Write;
    handle.write_all(content)?;
    handle.as_file().sync_all()?;
    fallow_config::preserve_target_mode(handle.path(), &resolved);
    Ok(StagedEntry {
        handle,
        requested: target.to_path_buf(),
        resolved,
    })
}

/// Map of absolute file path to the xxh3 content hash captured during the
/// in-process analysis run. Source files (TS / JS / Vue / Svelte / Astro)
/// are present; package.json and pnpm-workspace.yaml are NOT (those layers
/// re-parse and look up by key rather than by byte offset, so the race
/// window is naturally narrower).
pub(super) type CapturedHashes = FxHashMap<PathBuf, u64>;

/// Read `path`, validate its current content hash against the captured
/// hash, and return the source on match. On mismatch, push a
/// [`SkipReason::ContentChanged`] entry to the plan and return `None`. If
/// the path is not in `hashes` (file kind not parsed by extract: e.g.
/// package.json, pnpm-workspace.yaml), the read proceeds without a hash
/// check. If the file is unreadable or outside `root`, returns `None` via
/// the inner [`super::io::read_source`] guard.
///
/// **Cross-fixer composition.** If `plan` already carries a staged
/// rewrite for `path` (a prior fixer in the orchestrator's per-issue-type
/// sequence touched the same source file), this returns the staged bytes
/// without re-hashing them. That hand-off composes the second fixer's
/// edits on top of the first's: the second fixer sees the post-first-fix
/// view of the file, computes its edits against that, and stages the
/// composed result. Without this hand-off, both fixers would read the
/// original disk content, each compute a fresh whole-file rewrite, and
/// the second `stage` would overwrite the first via last-write-wins,
/// silently losing the first fixer's edits.
pub(super) fn read_source_with_hash_check(
    root: &Path,
    path: &Path,
    hashes: &CapturedHashes,
    plan: &mut FixPlan,
) -> Option<(String, super::io::EncodingMetadata)> {
    if let Some(staged) = plan.staged_content(path) {
        let raw = String::from_utf8(staged.to_vec()).ok()?;
        return match super::io::classify_source(&raw) {
            Ok((content, meta)) => Some((content, meta)),
            Err(super::io::EncodingError::MixedLineEndings { .. }) => {
                plan.skip(path.to_path_buf(), SkipReason::MixedLineEndings);
                None
            }
        };
    }
    let read_result = match super::io::read_source(root, path) {
        Ok(opt) => opt,
        Err(super::io::EncodingError::MixedLineEndings { .. }) => {
            plan.skip(path.to_path_buf(), SkipReason::MixedLineEndings);
            return None;
        }
    };
    let (content, meta) = read_result?;
    if let Some(&expected) = hashes.get(path) {
        let actual = xxhash_rust::xxh3::xxh3_64(content.as_bytes());
        if actual != expected {
            plan.skip(path.to_path_buf(), SkipReason::ContentChanged);
            return None;
        }
    }
    Some((content, meta))
}

/// Join modified lines, preserve the original trailing newline, re-prepend
/// the UTF-8 BOM when the source had one, and stage the result on `plan`.
/// Replaces the `write_fixed_content` direct-write shape with a queued one;
/// the orchestrator commits the plan after all fixers have run.
///
/// `original_content` is the post-BOM-strip view returned by
/// `read_source_with_hash_check`; the BOM bytes are reconstructed here on
/// the wire from `meta.had_bom` so the round-trip preserves whatever the
/// source file had on disk. Issue #475.
pub(super) fn stage_fixed_content(
    plan: &mut FixPlan,
    path: &Path,
    lines: &[String],
    meta: &super::io::EncodingMetadata,
    original_content: &str,
) {
    let mut result = lines.join(meta.line_ending);
    if original_content.ends_with(meta.line_ending) && !result.ends_with(meta.line_ending) {
        result.push_str(meta.line_ending);
    }
    let bytes = if meta.had_bom {
        let bom_bytes = "\u{FEFF}".as_bytes();
        let mut buf = Vec::with_capacity(result.len() + bom_bytes.len());
        buf.extend_from_slice(bom_bytes);
        buf.extend_from_slice(result.as_bytes());
        buf
    } else {
        result.into_bytes()
    };
    plan.stage(path.to_path_buf(), bytes);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commit_writes_every_staged_entry() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a.txt");
        let b = dir.path().join("b.txt");
        std::fs::write(&a, "original_a").unwrap();
        std::fs::write(&b, "original_b").unwrap();

        let mut plan = FixPlan::new();
        plan.stage(a.clone(), b"new_a".to_vec());
        plan.stage(b.clone(), b"new_b".to_vec());

        let outcome = plan.commit();
        assert!(outcome.failed.is_empty());
        assert_eq!(outcome.written.len(), 2);
        assert_eq!(std::fs::read_to_string(&a).unwrap(), "new_a");
        assert_eq!(std::fs::read_to_string(&b).unwrap(), "new_b");
    }

    #[test]
    fn commit_stage_failure_leaves_project_untouched() {
        let dir = tempfile::tempdir().unwrap();
        let good = dir.path().join("good.txt");
        let bad = dir.path().join("nonexistent").join("bad.txt");
        std::fs::write(&good, "original_good").unwrap();

        let mut plan = FixPlan::new();
        plan.stage(good.clone(), b"new_good".to_vec());
        plan.stage(bad, b"new_bad".to_vec());

        let outcome = plan.commit();
        assert!(!outcome.failed.is_empty(), "bad path should surface");
        assert!(outcome.written.is_empty(), "no rename should have run");
        assert_eq!(
            std::fs::read_to_string(&good).unwrap(),
            "original_good",
            "the good file must be untouched when any stage in the batch fails"
        );
    }

    #[test]
    fn commit_empty_plan_is_noop() {
        let plan = FixPlan::new();
        let outcome = plan.commit();
        assert!(outcome.written.is_empty());
        assert!(outcome.failed.is_empty());
    }

    #[test]
    fn skip_reason_wire_value_is_stable() {
        assert_eq!(SkipReason::ContentChanged.as_wire_str(), "content_changed");
        assert_eq!(
            SkipReason::LowConfidenceOffGraph.as_wire_str(),
            "low_confidence_off_graph"
        );
        assert_eq!(
            SkipReason::LowConfidenceUnresolvedImports.as_wire_str(),
            "low_confidence_unresolved_imports"
        );
    }

    #[test]
    fn low_confidence_skips_are_intentional_others_are_not() {
        assert!(SkipReason::LowConfidenceOffGraph.is_intentional());
        assert!(SkipReason::LowConfidenceUnresolvedImports.is_intentional());
        assert!(!SkipReason::ContentChanged.is_intentional());
        assert!(!SkipReason::MixedLineEndings.is_intentional());
    }

    #[test]
    fn skip_records_reach_skipped_list() {
        let mut plan = FixPlan::new();
        plan.skip(PathBuf::from("a.ts"), SkipReason::ContentChanged);
        assert_eq!(plan.skipped().len(), 1);
        assert_eq!(plan.skipped()[0].reason, SkipReason::ContentChanged);
    }

    #[test]
    fn stage_with_duplicate_path_keeps_last_write() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("dup.txt");
        std::fs::write(&p, "orig").unwrap();

        let mut plan = FixPlan::new();
        plan.stage(p.clone(), b"first".to_vec());
        plan.stage(p.clone(), b"second".to_vec());

        let outcome = plan.commit();
        assert!(outcome.failed.is_empty());
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "second");
    }

    #[test]
    fn read_source_with_hash_check_skips_on_mismatch() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("sample.ts");
        std::fs::write(&file, "const x = 1;\n").unwrap();
        let stale_hash: u64 = 0xDEAD_BEEF; // intentionally wrong
        let mut hashes = CapturedHashes::default();
        hashes.insert(file.clone(), stale_hash);

        let mut plan = FixPlan::new();
        let result = read_source_with_hash_check(dir.path(), &file, &hashes, &mut plan);
        assert!(result.is_none(), "mismatch must skip");
        assert_eq!(plan.skipped().len(), 1);
        assert_eq!(plan.skipped()[0].path, file);
        assert_eq!(plan.skipped()[0].reason, SkipReason::ContentChanged);
    }

    #[test]
    fn read_source_with_hash_check_proceeds_when_path_not_in_map() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("package.json");
        std::fs::write(&file, "{}").unwrap();
        let hashes = CapturedHashes::default();

        let mut plan = FixPlan::new();
        let result = read_source_with_hash_check(dir.path(), &file, &hashes, &mut plan);
        assert!(result.is_some(), "missing hash must proceed, not skip");
        assert!(plan.skipped().is_empty());
    }

    #[test]
    fn read_source_with_hash_check_passes_on_match() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("sample.ts");
        let body = "const x = 1;\n";
        std::fs::write(&file, body).unwrap();
        let correct_hash = xxhash_rust::xxh3::xxh3_64(body.as_bytes());
        let mut hashes = CapturedHashes::default();
        hashes.insert(file.clone(), correct_hash);

        let mut plan = FixPlan::new();
        let result = read_source_with_hash_check(dir.path(), &file, &hashes, &mut plan);
        let (content, _) = result.expect("match must proceed");
        assert_eq!(content, body);
        assert!(plan.skipped().is_empty());
    }

    #[test]
    fn staged_content_lets_a_second_fixer_compose_on_top_of_the_first() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("sample.ts");
        let original = "line a\nline b\nline c\n";
        std::fs::write(&file, original).unwrap();
        let mut hashes = CapturedHashes::default();
        hashes.insert(
            file.clone(),
            xxhash_rust::xxh3::xxh3_64(original.as_bytes()),
        );

        let mut plan = FixPlan::new();

        let first_view = read_source_with_hash_check(dir.path(), &file, &hashes, &mut plan)
            .expect("first read succeeds");
        assert_eq!(first_view.0, original);
        plan.stage(file.clone(), b"line a\nline c\n".to_vec());

        let second_view = read_source_with_hash_check(dir.path(), &file, &hashes, &mut plan)
            .expect("second read sees staged content");
        assert_eq!(
            second_view.0, "line a\nline c\n",
            "second fixer must read the first fixer's staged rewrite, not the original disk bytes"
        );
        plan.stage(file.clone(), b"edited a\nline c\n".to_vec());

        let outcome = plan.commit();
        assert!(outcome.failed.is_empty());
        assert_eq!(
            std::fs::read_to_string(&file).unwrap(),
            "edited a\nline c\n",
            "both fixers' edits must compose into the final commit",
        );
    }

    #[cfg(unix)]
    #[test]
    fn commit_preserves_target_file_mode() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("source.ts");
        std::fs::write(&file, "original\n").unwrap();
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o644)).unwrap();

        let mut plan = FixPlan::new();
        plan.stage(file.clone(), b"rewritten\n".to_vec());
        let outcome = plan.commit();
        assert!(outcome.failed.is_empty());

        let post_mode = std::fs::metadata(&file).unwrap().permissions().mode() & 0o7777;
        assert_eq!(
            post_mode, 0o644,
            "post-commit mode must match pre-commit mode, not the NamedTempFile default"
        );
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "rewritten\n");
    }

    #[cfg(unix)]
    #[test]
    fn commit_writes_through_symlink_to_the_real_target() {
        let dir = tempfile::tempdir().unwrap();
        let real = dir.path().join("real.ts");
        let link = dir.path().join("link.ts");
        std::fs::write(&real, "original").unwrap();
        std::os::unix::fs::symlink(&real, &link).unwrap();

        let mut plan = FixPlan::new();
        plan.stage(link.clone(), b"rewritten".to_vec());
        let outcome = plan.commit();
        assert!(outcome.failed.is_empty());

        assert!(
            std::fs::symlink_metadata(&link)
                .unwrap()
                .file_type()
                .is_symlink(),
            "symlink must survive commit",
        );
        assert_eq!(std::fs::read_to_string(&real).unwrap(), "rewritten");
    }

    #[test]
    fn entries_paths_yields_every_staged_path() {
        let mut plan = FixPlan::new();
        plan.stage(PathBuf::from("/tmp/a"), b"x".to_vec());
        plan.stage(PathBuf::from("/tmp/b"), b"y".to_vec());
        assert_eq!(plan.entries_paths().count(), 2);
    }

    #[test]
    fn _atomic_write_still_works_for_callers_not_routed_through_the_plan() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        fallow_config::atomic_write(&path, b"{}").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "{}");
    }

    #[test]
    fn skip_deduplicates_repeat_entries_for_same_path_and_reason() {
        let mut plan = FixPlan::new();
        let path = PathBuf::from("/tmp/mixed.ts");
        plan.skip(path.clone(), SkipReason::MixedLineEndings);
        plan.skip(path.clone(), SkipReason::MixedLineEndings);
        plan.skip(path.clone(), SkipReason::MixedLineEndings);
        assert_eq!(
            plan.skipped().len(),
            1,
            "multiple skip calls for the same (path, reason) must dedupe to one entry",
        );
        plan.skip(path, SkipReason::ContentChanged);
        assert_eq!(
            plan.skipped().len(),
            2,
            "distinct reasons on the same path stay separate",
        );
        plan.skip(PathBuf::from("/tmp/other.ts"), SkipReason::MixedLineEndings);
        assert_eq!(plan.skipped().len(), 3);
    }

    #[test]
    fn read_source_with_hash_check_skips_on_mixed_line_endings() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("mixed.ts");
        std::fs::write(&file, "a\r\nb\nc\r\n").unwrap();
        let mut hashes = CapturedHashes::default();
        hashes.insert(file.clone(), 0xDEAD_BEEF);

        let mut plan = FixPlan::new();
        let result = read_source_with_hash_check(dir.path(), &file, &hashes, &mut plan);
        assert!(result.is_none(), "mixed-EOL file must be skipped");
        assert_eq!(plan.skipped().len(), 1);
        assert_eq!(plan.skipped()[0].path, file);
        assert_eq!(plan.skipped()[0].reason, SkipReason::MixedLineEndings);
    }

    #[test]
    fn read_source_with_hash_check_dedupes_mixed_eol_across_two_fixer_calls() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("mixed.ts");
        std::fs::write(&file, "a\r\nb\nc\r\n").unwrap();
        let hashes = CapturedHashes::default();

        let mut plan = FixPlan::new();

        let first = read_source_with_hash_check(dir.path(), &file, &hashes, &mut plan);
        assert!(first.is_none(), "first fixer call must skip");

        let second = read_source_with_hash_check(dir.path(), &file, &hashes, &mut plan);
        assert!(second.is_none(), "second fixer call must also skip");

        assert_eq!(
            plan.skipped().len(),
            1,
            "two fixers hitting the same mixed-EOL file must produce ONE skip entry, not two",
        );
        assert_eq!(plan.skipped()[0].reason, SkipReason::MixedLineEndings);
    }

    #[test]
    fn skip_reason_mixed_line_endings_wire_value_is_stable() {
        assert_eq!(
            SkipReason::MixedLineEndings.as_wire_str(),
            "mixed_line_endings"
        );
    }

    #[test]
    fn stage_fixed_content_preserves_bom_on_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("bom.ts");
        let body = "export const a = 1;\nexport const b = 2;\n";
        std::fs::write(&file, format!("\u{FEFF}{body}")).unwrap();

        let mut plan = FixPlan::new();
        let (content, meta) = crate::fix::io::read_source(dir.path(), &file)
            .unwrap()
            .unwrap();
        assert!(meta.had_bom, "preconditions: read must flag had_bom = true");
        assert_eq!(
            content.as_str(),
            body,
            "post-strip content must omit the BOM"
        );

        let new_lines: Vec<String> = vec!["export const a = 1;".to_owned()];
        stage_fixed_content(&mut plan, &file, &new_lines, &meta, &content);

        let outcome = plan.commit();
        assert!(outcome.failed.is_empty(), "commit must succeed");

        let on_disk = std::fs::read(&file).unwrap();
        assert_eq!(
            &on_disk[..3],
            &[0xEF, 0xBB, 0xBF],
            "BOM must be re-prepended on round-trip; got {:?}",
            &on_disk[..on_disk.len().min(8)],
        );
        let rest = std::str::from_utf8(&on_disk[3..]).unwrap();
        assert_eq!(rest, "export const a = 1;\n");
    }

    #[test]
    fn staged_content_round_trip_through_second_fixer_preserves_bom() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("bom-multi.ts");
        let body = "line a\nline b\nline c\n";
        std::fs::write(&file, format!("\u{FEFF}{body}")).unwrap();
        let mut hashes = CapturedHashes::default();
        hashes.insert(file.clone(), xxhash_rust::xxh3::xxh3_64(body.as_bytes()));

        let mut plan = FixPlan::new();

        let (first_content, first_meta) =
            read_source_with_hash_check(dir.path(), &file, &hashes, &mut plan).unwrap();
        assert!(first_meta.had_bom);
        let first_new_lines: Vec<String> =
            vec!["line a".to_owned(), "line c".to_owned(), String::new()];
        stage_fixed_content(
            &mut plan,
            &file,
            &first_new_lines,
            &first_meta,
            &first_content,
        );

        let (second_content, second_meta) =
            read_source_with_hash_check(dir.path(), &file, &hashes, &mut plan).unwrap();
        assert!(
            second_meta.had_bom,
            "second fixer must re-detect BOM from staged bytes; had_bom dropped silently",
        );
        assert!(
            !second_content.starts_with('\u{FEFF}'),
            "second fixer content must be post-BOM-strip",
        );
        let second_new_lines: Vec<String> =
            vec!["edited a".to_owned(), "line c".to_owned(), String::new()];
        stage_fixed_content(
            &mut plan,
            &file,
            &second_new_lines,
            &second_meta,
            &second_content,
        );

        let outcome = plan.commit();
        assert!(outcome.failed.is_empty());
        let on_disk = std::fs::read(&file).unwrap();
        assert_eq!(
            &on_disk[..3],
            &[0xEF, 0xBB, 0xBF],
            "BOM must survive both fixers' round trips; got {:?}",
            &on_disk[..on_disk.len().min(8)],
        );
        let rest = std::str::from_utf8(&on_disk[3..]).unwrap();
        assert_eq!(rest, "edited a\nline c\n");
    }
}
