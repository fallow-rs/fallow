use std::path::Path;

/// Encoding shape of a source file, captured at read time so the matching
/// write call can round-trip the same shape: same line ending, BOM preserved
/// if the input had one. See issue #475.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct EncodingMetadata {
    /// Detected line ending (`"\r\n"` or `"\n"`). Pure CRLF and pure LF
    /// files are both supported; mixed files are surfaced via
    /// [`EncodingError::MixedLineEndings`] before this struct is built.
    pub line_ending: &'static str,
    /// True when the input file started with a UTF-8 BOM (`\u{FEFF}`).
    /// `stage_fixed_content` re-prepends the BOM bytes when this flag is
    /// set so `fallow fix` does not silently re-encode a Windows-authored
    /// file.
    pub had_bom: bool,
}

/// Errors that block a file from being fixed safely. Today there is one
/// variant; we keep the enum shape so future encoding checks (e.g. invalid
/// UTF-8 bytes) can land additively.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum EncodingError {
    /// The file contains both CRLF and bare-LF line endings, or a bare CR
    /// line ending. The fix pipeline detects line endings by presence check
    /// then splits / joins on the detected style; on a mixed file that would
    /// silently rewrite to the wrong offsets. A bare CR ends a line for the
    /// parser but not for the `\n` line split of the fixers, so its file has
    /// the same offset problem. Skipping is safer than a destructive guess.
    MixedLineEndings {
        crlf_count: usize,
        lf_only_count: usize,
        lone_cr_count: usize,
    },
}

/// Read a source file, validate it is within the project root, strip an
/// optional UTF-8 BOM, detect line endings, and reject mixed CRLF/LF
/// content.
///
/// Returns:
/// - `Ok(Some((content_post_bom, metadata)))` on success.
/// - `Ok(None)` when the path is outside the project root, resolves into a
///   `.git` or `node_modules` directory below it, or is unreadable
///   (existing skip-with-warn semantics preserved).
/// - `Err(EncodingError::MixedLineEndings { .. })` when the file mixes CRLF
///   and bare-LF line endings or has a bare CR line ending; the caller translates this into a per-file
///   skip with a clear remediation message.
pub(super) fn read_source(
    root: &Path,
    path: &Path,
) -> Result<Option<(String, EncodingMetadata)>, EncodingError> {
    let Ok(canonical_root) = std::fs::canonicalize(root) else {
        tracing::warn!(root = %root.display(), "Skipping fix because project root cannot be resolved");
        return Ok(None);
    };
    let Ok(canonical_path) = std::fs::canonicalize(path) else {
        return Ok(None);
    };
    if !canonical_path.starts_with(&canonical_root) {
        tracing::warn!(path = %path.display(), "Skipping fix for path outside project root");
        return Ok(None);
    }
    if fallow_types::path_guard::is_protected_link_target(&canonical_root, &canonical_path) {
        tracing::warn!(
            path = %path.display(),
            "Skipping fix for path that resolves into a .git or node_modules directory"
        );
        return Ok(None);
    }
    let Ok(raw) = std::fs::read_to_string(canonical_path) else {
        return Ok(None);
    };
    classify_source(&raw).map(|(content, metadata)| Some((content, metadata)))
}

/// Pure shape-classification helper: strip BOM, detect mixed CRLF/LF and
/// bare CR line endings, and
/// pick the line-ending style. Factored out of `read_source` so the staged
/// fast path in `plan.rs` (cross-fixer composition) can reuse the same
/// classifier on in-memory bytes without a disk round-trip.
pub(super) fn classify_source(raw: &str) -> Result<(String, EncodingMetadata), EncodingError> {
    let had_bom = raw.starts_with('\u{FEFF}');
    let content = raw.strip_prefix('\u{FEFF}').unwrap_or(raw).to_owned();

    let crlf_count = content.matches("\r\n").count();
    let lf_total = content.matches('\n').count();
    let lf_only_count = lf_total.saturating_sub(crlf_count);
    let lone_cr_count = content.matches('\r').count().saturating_sub(crlf_count);
    if (crlf_count > 0 && lf_only_count > 0) || lone_cr_count > 0 {
        return Err(EncodingError::MixedLineEndings {
            crlf_count,
            lf_only_count,
            lone_cr_count,
        });
    }

    let line_ending = if crlf_count > 0 { "\r\n" } else { "\n" };
    Ok((
        content,
        EncodingMetadata {
            line_ending,
            had_bom,
        },
    ))
}

/// Convert `body` into wire bytes, re-prepending the UTF-8 BOM when
/// `meta.had_bom` is true. Used by fixers that build their own whole-file
/// rewrite as a `String` (catalog YAML writer) and stage the resulting
/// `Vec<u8>` directly on the plan, in place of `stage_fixed_content`. The
/// BOM is structurally legal in YAML (the parser tolerates it) but rare in
/// practice; preserving it matters for symmetry with the source-code path.
/// Issue #475.
pub(super) fn bytes_with_optional_bom(body: String, meta: &EncodingMetadata) -> Vec<u8> {
    if meta.had_bom {
        let bom_bytes = "\u{FEFF}".as_bytes();
        let mut buf = Vec::with_capacity(body.len() + bom_bytes.len());
        buf.extend_from_slice(bom_bytes);
        buf.extend_from_slice(body.as_bytes());
        buf
    } else {
        body.into_bytes()
    }
}

#[cfg(test)]
pub(super) use fallow_config::atomic_write;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn atomic_write_creates_file_with_content() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.ts");
        atomic_write(&path, b"hello world").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "hello world");
    }

    #[test]
    fn atomic_write_overwrites_existing_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.ts");
        std::fs::write(&path, "old content").unwrap();
        atomic_write(&path, b"new content").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "new content");
    }

    #[test]
    fn atomic_write_no_leftover_temp_on_success() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.ts");
        atomic_write(&path, b"data").unwrap();
        let entries: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .filter_map(Result::ok)
            .collect();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].file_name(), "test.ts");
    }

    #[test]
    fn atomic_write_to_nonexistent_dir_fails() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nonexistent_dir").join("file.ts");
        let result = atomic_write(&path, b"content");
        assert!(result.is_err());
    }

    #[test]
    fn atomic_write_empty_content() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("empty.ts");
        atomic_write(&path, b"").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "");
    }

    #[test]
    fn atomic_write_binary_content() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("binary.dat");
        let data: Vec<u8> = (0..=255).collect();
        atomic_write(&path, &data).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), data);
    }

    #[test]
    fn read_source_returns_none_for_path_outside_root() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("project");
        std::fs::create_dir_all(&root).unwrap();
        let outside = dir.path().join("outside.ts");
        std::fs::write(&outside, "content").unwrap();

        let result = read_source(&root, &outside).unwrap();
        assert!(result.is_none());
    }

    #[cfg(unix)]
    #[test]
    fn read_source_returns_none_for_symlink_target_outside_root() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("project");
        std::fs::create_dir(&root).unwrap();
        let outside = dir.path().join("outside.ts");
        let link = root.join("link.ts");
        std::fs::write(&outside, "private content").unwrap();
        std::os::unix::fs::symlink(&outside, &link).unwrap();

        let result = read_source(&root, &link).unwrap();
        assert!(result.is_none());
    }

    #[cfg(unix)]
    #[test]
    fn read_source_returns_none_for_symlink_target_in_the_git_dir() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("project");
        std::fs::create_dir_all(root.join(".git")).unwrap();
        std::fs::write(root.join(".git/HEAD"), "ref: refs/heads/main\n").unwrap();
        let link = root.join("head.ts");
        std::os::unix::fs::symlink(root.join(".git/HEAD"), &link).unwrap();

        let result = read_source(&root, &link).unwrap();
        assert!(result.is_none());
    }

    /// A worktree of a bare repository lies inside the common Git directory.
    /// Its own files stay readable, and a link into the Git directory is
    /// still refused.
    #[cfg(unix)]
    #[test]
    fn read_source_reads_a_file_of_a_worktree_inside_a_bare_repository() {
        let dir = tempfile::tempdir().unwrap();
        let bare = dir.path().join("proj.git");
        let linked = bare.join("worktrees/main");
        std::fs::create_dir_all(&linked).unwrap();
        std::fs::create_dir_all(bare.join("hooks")).unwrap();
        std::fs::write(linked.join("commondir"), "../..\n").unwrap();
        let root = bare.join("main");
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join(".git"), format!("gitdir: {}\n", linked.display())).unwrap();
        let file = root.join("src/a.ts");
        std::fs::write(&file, "export const a = 1;\n").unwrap();
        std::fs::write(bare.join("hooks/pre-commit"), "keep").unwrap();
        let link = root.join("src/hook.ts");
        std::os::unix::fs::symlink(bare.join("hooks/pre-commit"), &link).unwrap();

        assert!(read_source(&root, &file).unwrap().is_some());
        assert!(read_source(&root, &link).unwrap().is_none());
    }

    #[test]
    fn read_source_returns_none_for_nonexistent_file() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let missing = root.join("missing.ts");

        let result = read_source(root, &missing).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn read_source_detects_lf_line_ending() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let file = root.join("lf.ts");
        std::fs::write(&file, "line1\nline2\n").unwrap();

        let (content, meta) = read_source(root, &file).unwrap().unwrap();
        assert_eq!(meta.line_ending, "\n");
        assert!(!meta.had_bom);
        assert_eq!(content, "line1\nline2\n");
    }

    #[test]
    fn read_source_detects_crlf_line_ending() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let file = root.join("crlf.ts");
        std::fs::write(&file, "line1\r\nline2\r\n").unwrap();

        let (content, meta) = read_source(root, &file).unwrap().unwrap();
        assert_eq!(meta.line_ending, "\r\n");
        assert!(!meta.had_bom);
        assert_eq!(content, "line1\r\nline2\r\n");
    }

    #[test]
    fn read_source_empty_file() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let file = root.join("empty.ts");
        std::fs::write(&file, "").unwrap();

        let (content, meta) = read_source(root, &file).unwrap().unwrap();
        assert_eq!(content, "");
        assert_eq!(meta.line_ending, "\n");
        assert!(!meta.had_bom);
    }

    #[test]
    fn read_source_strips_utf8_bom_and_flags_metadata() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let file = root.join("bom.ts");
        std::fs::write(&file, "\u{FEFF}export const x = 1;\nexport const y = 2;\n").unwrap();

        let (content, meta) = read_source(root, &file).unwrap().unwrap();
        assert!(meta.had_bom, "BOM presence must be flagged on metadata");
        assert!(
            !content.starts_with('\u{FEFF}'),
            "returned content must have the BOM stripped",
        );
        assert_eq!(content, "export const x = 1;\nexport const y = 2;\n");
        assert_eq!(meta.line_ending, "\n");
    }

    #[test]
    fn read_source_detects_pure_crlf_with_bom() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let file = root.join("bom-crlf.ts");
        std::fs::write(&file, "\u{FEFF}line1\r\nline2\r\n").unwrap();

        let (content, meta) = read_source(root, &file).unwrap().unwrap();
        assert!(meta.had_bom);
        assert_eq!(meta.line_ending, "\r\n");
        assert_eq!(content, "line1\r\nline2\r\n");
    }

    #[test]
    fn read_source_detects_mixed_crlf_lf_and_returns_err() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let file = root.join("mixed.ts");
        std::fs::write(
            &file,
            "export const a = 1;\r\nexport const b = 2;\nexport const c = 3;\r\n",
        )
        .unwrap();

        let err = read_source(root, &file).unwrap_err();
        match err {
            EncodingError::MixedLineEndings {
                crlf_count,
                lf_only_count,
                lone_cr_count,
            } => {
                assert_eq!(crlf_count, 2, "two CRLF lines");
                assert_eq!(lf_only_count, 1, "one bare-LF line");
                assert_eq!(lone_cr_count, 0, "no bare CR");
            }
        }
    }

    #[test]
    fn read_source_mixed_with_bom_is_still_mixed_after_strip() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let file = root.join("bom-mixed.ts");
        std::fs::write(&file, "\u{FEFF}a\r\nb\nc\r\n").unwrap();

        let err = read_source(root, &file).unwrap_err();
        assert!(matches!(err, EncodingError::MixedLineEndings { .. }));
    }

    /// The fixers split lines on `\n` only. A bare CR does not end a line for
    /// them, but it ends a line for the parser, so the edit offsets would be
    /// wrong. The file is refused like a mixed CRLF and LF file.
    #[test]
    fn classify_source_refuses_a_bare_carriage_return_line_ending() {
        for raw in [
            "export const a = 1;\rexport const b = 2;\r",
            "export const a = 1;\nexport const b = 2;\rexport const c = 3;\n",
            "export const a = 1;\r\nexport const b = 2;\rexport const c = 3;\r\n",
        ] {
            let err = classify_source(raw).unwrap_err();
            assert!(
                matches!(
                    err,
                    EncodingError::MixedLineEndings {
                        lone_cr_count: 1..,
                        ..
                    }
                ),
                "{raw:?} must be refused, got {err:?}",
            );
        }
    }

    #[test]
    fn classify_source_pure_lf_no_bom_round_trips() {
        let (content, meta) = classify_source("a\nb\nc\n").unwrap();
        assert_eq!(content, "a\nb\nc\n");
        assert_eq!(meta.line_ending, "\n");
        assert!(!meta.had_bom);
    }

    #[test]
    fn classify_source_single_line_no_newline_defaults_to_lf() {
        let (_, meta) = classify_source("single line").unwrap();
        assert_eq!(meta.line_ending, "\n");
        assert!(!meta.had_bom);
    }
}
