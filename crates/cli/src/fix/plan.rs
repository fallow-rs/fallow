//! Batch-atomicity layer for `fallow fix`.
//!
//! Each per-issue-type fixer (`exports`, `enum_members`, `deps`, `catalog`)
//! accumulates `(PathBuf, Vec<u8>)` entries on a shared [`FixPlan`] instead
//! of writing directly. After all fixers run, the orchestrator commits the
//! plan: every entry is first staged to a `NamedTempFile` in the same
//! directory as the target, and only when every stage has succeeded does
//! the commit promote each temp to its final path via the existing atomic
//! rename. A failure at the stage step leaves the project untouched. A
//! failure at the rename step is reported per-path; some renames may have
//! already landed (POSIX rename is per-file atomic; there is no atomic
//! multi-rename primitive).
//!
//! The plan also carries skipped-file records (e.g. hash mismatch between
//! the in-process analysis and the on-disk content at fix time); the
//! orchestrator surfaces these in the JSON envelope and non-zero exit code.

use std::path::{Path, PathBuf};

use rustc_hash::{FxHashMap, FxHashSet};
use tempfile::NamedTempFile;

/// One file's content waiting to be written.
struct PlannedWrite {
    path: PathBuf,
    content: Vec<u8>,
}

/// Why a file was skipped during a fix run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum SkipReason {
    /// The file's content hash at fix time differs from the hash captured
    /// during the in-process analysis. Applying offsets computed against
    /// the analyzed bytes would land them on the wrong source.
    ContentChanged,
}

impl SkipReason {
    pub(super) fn as_wire_str(self) -> &'static str {
        match self {
            Self::ContentChanged => "content_changed",
        }
    }

    pub(super) fn human_message(self, path: &Path) -> String {
        match self {
            Self::ContentChanged => format!(
                "Skipping {}: file content changed since `fallow check` ran. Re-run `fallow fix` to refresh the analysis first.",
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
    /// Absolute paths whose new content landed on disk. Held for
    /// observability and post-commit verification by integration tests;
    /// the orchestrator only inspects `failed` (every fixer sets
    /// `applied: true` optimistically before commit, then we flip to
    /// false on failure via the `__target` sidechannel).
    #[allow(
        dead_code,
        reason = "test-only reader; `#[expect]` is unfulfilled under `--all-targets` because the test cfg satisfies dead_code while the lib cfg would fire it"
    )]
    pub written: FxHashSet<PathBuf>,
    /// Per-path errors. `failed.is_empty() && written == plan.entries` is
    /// the success case.
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

    /// Queue a write. The last call for a given path wins; the caller is
    /// responsible for composing edits on top of any prior staged content
    /// (via `read_source_with_hash_check`, which returns the staged bytes
    /// when present so the next fixer's edits compose rather than collide).
    pub(super) fn stage(&mut self, path: PathBuf, content: Vec<u8>) {
        if let Some(existing) = self.entries.iter_mut().find(|e| e.path == path) {
            existing.content = content;
            return;
        }
        self.entries.push(PlannedWrite { path, content });
    }

    /// Return the currently-staged content for `path`, if any. Used by
    /// `read_source_with_hash_check` so a second fixer reads its starting
    /// bytes from the first fixer's pending plan entry instead of from
    /// disk; this composes cross-fixer edits on the same file (e.g.
    /// removing both an unused export AND an unused enum member from
    /// the same source) into a single coherent rewrite. Without this
    /// hand-off, the second stage would overwrite the first with a
    /// plan-fresh-from-disk view, silently losing the first fixer's
    /// edits.
    pub(super) fn staged_content(&self, path: &Path) -> Option<&[u8]> {
        self.entries
            .iter()
            .find(|e| e.path == path)
            .map(|e| e.content.as_slice())
    }

    /// Record that a file was skipped. The orchestrator uses this to
    /// surface a clear diagnostic and set a non-zero exit code.
    pub(super) fn skip(&mut self, path: PathBuf, reason: SkipReason) {
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

    /// Stage every entry to a sibling `NamedTempFile`, then promote each
    /// to its final path. If staging any entry fails, returns immediately
    /// without renaming anything: the project is untouched. If a rename
    /// fails (rare, filesystem-level), the entries that already renamed
    /// stay applied and the failure is reported per-path.
    pub(super) fn commit(self) -> CommitOutcome {
        if self.entries.is_empty() {
            return CommitOutcome::empty();
        }

        // Stage every entry first. Hold the NamedTempFile handles until we
        // know every stage succeeded; on staging failure, all handles drop
        // here and the temp files are removed before any rename runs. We
        // also carry the RESOLVED (canonicalized) path so the final
        // rename writes through symlinks, matching `fallow_config::atomic_write`'s
        // contract; persisting to the original path would replace the
        // symlink itself with a regular file and leave the real target
        // untouched.
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

        // Sort by REQUESTED path (the user-visible identity) for
        // deterministic rename order. Stable per-path ordering matters
        // for debugability (failure logs name files in a predictable
        // order across runs).
        staged.sort_by(|a, b| a.requested.cmp(&b.requested));

        let mut written = FxHashSet::default();
        let mut failed = Vec::new();
        for stage in staged {
            match stage.handle.persist(&stage.resolved) {
                Ok(_) => {
                    written.insert(stage.requested);
                }
                Err(err) => {
                    // PersistError -> io::Error preserves the original errno.
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
    // Match atomic_write's behavior: canonicalize through symlinks so the
    // temp file lands in the directory of the resolved target AND the
    // final rename promotes the temp into the resolved path. Persisting
    // to the original (non-canonical) path replaces the symlink with the
    // temp file, leaving the real target untouched; that regresses the
    // pre-#454 atomic_write contract.
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
    // Preserve the target's existing file mode on Unix. NamedTempFile creates
    // the temp with 0600 by default; persisting directly would downgrade a
    // target previously at 0644 (or any other mode) to owner-only, breaking
    // shared workspaces and CI runners that rely on the existing read bit.
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
) -> Option<(String, &'static str)> {
    // Cross-fixer composition: prefer the in-plan staged content over a
    // disk read. Staged bytes are internal and always valid UTF-8 (every
    // fixer produces text via `String::into_bytes`); a UTF-8 failure here
    // would indicate a programmer error, not a user-facing bug.
    if let Some(staged) = plan.staged_content(path) {
        let content = String::from_utf8(staged.to_vec()).ok()?;
        let line_ending = if content.contains("\r\n") {
            "\r\n"
        } else {
            "\n"
        };
        return Some((content, line_ending));
    }
    let (content, line_ending) = super::io::read_source(root, path)?;
    if let Some(&expected) = hashes.get(path) {
        let actual = xxhash_rust::xxh3::xxh3_64(content.as_bytes());
        if actual != expected {
            plan.skip(path.to_path_buf(), SkipReason::ContentChanged);
            return None;
        }
    }
    Some((content, line_ending))
}

/// Join modified lines, preserve the original trailing newline, and stage
/// the result on `plan`. Replaces the `write_fixed_content` direct-write
/// shape with a queued one; the orchestrator commits the plan after all
/// fixers have run.
pub(super) fn stage_fixed_content(
    plan: &mut FixPlan,
    path: &Path,
    lines: &[String],
    line_ending: &str,
    original_content: &str,
) {
    let mut result = lines.join(line_ending);
    if original_content.ends_with(line_ending) && !result.ends_with(line_ending) {
        result.push_str(line_ending);
    }
    plan.stage(path.to_path_buf(), result.into_bytes());
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
        // Force staging to fail by pointing at a path whose parent does
        // not exist; no temp can be created. The other entry must NOT
        // be promoted.
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
        // Downstream JSON consumers gate on these strings; flag rename
        // bombs at PR-review time.
        assert_eq!(SkipReason::ContentChanged.as_wire_str(), "content_changed");
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
        // Files not produced by the extract layer (package.json, YAML)
        // are not in the captured-hash map. They must proceed without a
        // skip (atomic_write per-file is the existing safety net).
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
        // Regression for the issue #454 cross-fixer composition gap
        // (codex parallel review BLOCK): when two fixers touch the same
        // file, the second must read the FIRST's staged content (not the
        // original disk bytes), so its rewrite composes instead of
        // overwriting via last-write-wins.
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

        // First fixer: removes "line b" (whole-file rewrite).
        let first_view = read_source_with_hash_check(dir.path(), &file, &hashes, &mut plan)
            .expect("first read succeeds");
        assert_eq!(first_view.0, original);
        plan.stage(file.clone(), b"line a\nline c\n".to_vec());

        // Second fixer: reads the same path; MUST see the first fixer's
        // staged content, not the disk content, so its edits compose.
        let second_view = read_source_with_hash_check(dir.path(), &file, &hashes, &mut plan)
            .expect("second read sees staged content");
        assert_eq!(
            second_view.0, "line a\nline c\n",
            "second fixer must read the first fixer's staged rewrite, not the original disk bytes"
        );
        // Second fixer mutates "line a" -> "edited a", stages the result.
        plan.stage(file.clone(), b"edited a\nline c\n".to_vec());

        // Commit and confirm the on-disk file carries BOTH edits.
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
        // Regression: NamedTempFile defaults to 0600. Without an explicit
        // chmod step before persist, a target previously at 0644 would land
        // at 0600 post-fix, silently downgrading the read bit for group +
        // other. The commit MUST preserve the target's pre-existing mode.
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
        // Regression for the issue #454 symlink BLOCK (codex parallel
        // review): the previous shape canonicalized only to choose the
        // temp directory but persisted to the original (non-canonical)
        // path, so the rename replaced the symlink itself with a regular
        // file and the real target was never touched.
        let dir = tempfile::tempdir().unwrap();
        let real = dir.path().join("real.ts");
        let link = dir.path().join("link.ts");
        std::fs::write(&real, "original").unwrap();
        std::os::unix::fs::symlink(&real, &link).unwrap();

        let mut plan = FixPlan::new();
        plan.stage(link.clone(), b"rewritten".to_vec());
        let outcome = plan.commit();
        assert!(outcome.failed.is_empty());

        // The symlink must still BE a symlink (not replaced by a regular
        // file), and the rewrite must have flowed through to the real
        // target.
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
        // Sanity check: the existing atomic_write entry point used by
        // config.rs (which is intentionally NOT batched) still works.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        fallow_config::atomic_write(&path, b"{}").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "{}");
    }
}
