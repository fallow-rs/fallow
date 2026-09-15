//! The `--explain-skipped` note for source files a built-in discovery ignore
//! pattern removed (issue #2638).
//!
//! The walk always records the typed `excluded-by-default-ignore` entries, so
//! `workspace_diagnostics[]` carries them in every JSON envelope regardless of
//! any flag. Whether a human run mentions them is a presentation choice, and it
//! belongs here rather than in the walk: the exclusions are designed behavior
//! on generated output, so a default stderr line would fire on most monorepos
//! and say nothing actionable. `--explain-skipped` already means "expand the
//! skipped-file note" for duplication; this widens it to discovery.

use std::fmt::Write as _;
use std::path::Path;

use fallow_config::{OutputFormat, WorkspaceDiagnostic, WorkspaceDiagnosticKind};

/// Build the per-pattern note for the built-in ignores that removed candidate
/// source files, or `None` when this run excluded none.
///
/// Pure so the singular and plural forms, the per-pattern rows, and the empty
/// case are unit-testable without a tracing subscriber or a real project,
/// mirroring the duplication note beside it.
#[must_use]
pub fn build_default_ignore_exclusion_note(
    root: &Path,
    diagnostics: &[WorkspaceDiagnostic],
) -> Option<String> {
    let rows: Vec<(u32, &str, String)> = diagnostics
        .iter()
        .filter_map(|diagnostic| match &diagnostic.kind {
            WorkspaceDiagnosticKind::ExcludedByDefaultIgnore {
                pattern,
                file_count,
            } => Some((
                *file_count,
                pattern.as_str(),
                display_relative(root, &diagnostic.path),
            )),
            _ => None,
        })
        .collect();
    if rows.is_empty() {
        return None;
    }

    let total: u64 = rows.iter().map(|(count, _, _)| u64::from(*count)).sum();
    let noun = if total == 1 { "file" } else { "files" };
    let mut note = format!(
        "note: skipped {total} source {noun} matching fallow's built-in discovery ignores:"
    );
    for (count, pattern, directory) in &rows {
        let _ = write!(
            note,
            "\n  {count:>5}  {pattern}  (mostly under {directory})"
        );
    }
    note.push_str(
        "\n  built-in ignores cannot be switched off through ignorePatterns; \
         analyze a directory on its own with fallow --root <dir>",
    );
    Some(note)
}

/// Render a diagnostic path relative to the project root with forward slashes,
/// matching how every JSON envelope emits it.
fn display_relative(root: &Path, path: &Path) -> String {
    let relative = path.strip_prefix(root).unwrap_or(path);
    let rendered = relative.display().to_string().replace('\\', "/");
    if rendered.is_empty() {
        ".".to_owned()
    } else {
        rendered
    }
}

/// Print the note on stderr when the run asked for it and the output format is
/// one a human reads.
///
/// Takes the stage's OWN diagnostics list. Re-reading the process-global
/// registry here would report whichever walk wrote last, which varies between
/// runs of the same combined command.
pub fn print_default_ignore_exclusion_note(
    root: &Path,
    diagnostics: &[WorkspaceDiagnostic],
    explain_skipped: bool,
    quiet: bool,
    output: OutputFormat,
) {
    if !explain_skipped
        || quiet
        || !matches!(
            output,
            OutputFormat::Human
                | OutputFormat::Markdown
                | OutputFormat::PrCommentGithub
                | OutputFormat::PrCommentGitlab
                | OutputFormat::ReviewGithub
                | OutputFormat::ReviewGitlab
        )
    {
        return;
    }
    if let Some(note) = build_default_ignore_exclusion_note(root, diagnostics) {
        eprintln!("{note}");
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    fn excluded(path: &str, pattern: &str, file_count: u32) -> WorkspaceDiagnostic {
        WorkspaceDiagnostic::new(
            Path::new("/repo"),
            PathBuf::from("/repo").join(path),
            WorkspaceDiagnosticKind::ExcludedByDefaultIgnore {
                pattern: pattern.to_owned(),
                file_count,
            },
        )
    }

    #[test]
    fn no_exclusions_produce_no_note() {
        assert!(build_default_ignore_exclusion_note(Path::new("/repo"), &[]).is_none());
    }

    #[test]
    fn unrelated_diagnostics_produce_no_note() {
        let other = WorkspaceDiagnostic::new(
            Path::new("/repo"),
            PathBuf::from("/repo/node_modules"),
            WorkspaceDiagnosticKind::NodeModulesMissing,
        );
        assert!(build_default_ignore_exclusion_note(Path::new("/repo"), &[other]).is_none());
    }

    #[test]
    fn a_single_excluded_file_reads_as_one_file() {
        let note = build_default_ignore_exclusion_note(
            Path::new("/repo"),
            &[excluded("dist", "**/dist/**", 1)],
        )
        .expect("one exclusion produces a note");
        assert!(note.contains("skipped 1 source file matching"), "{note}");
        assert!(note.contains("**/dist/**"), "{note}");
        assert!(note.contains("(mostly under dist)"), "{note}");
    }

    #[test]
    fn several_patterns_each_get_a_row_and_the_total_is_their_sum() {
        let note = build_default_ignore_exclusion_note(
            Path::new("/repo"),
            &[
                excluded("packages/web/build", "**/build/**", 4),
                excluded("coverage", "**/coverage/**", 2),
            ],
        )
        .expect("two exclusions produce a note");
        assert!(note.contains("skipped 6 source files matching"), "{note}");
        assert!(note.contains("**/build/**"), "{note}");
        assert!(note.contains("packages/web/build"), "{note}");
        assert!(note.contains("**/coverage/**"), "{note}");
        assert_eq!(note.lines().count(), 4, "one header, two rows, one remedy");
    }

    #[test]
    fn the_note_advertises_root_and_not_a_config_edit() {
        let note = build_default_ignore_exclusion_note(
            Path::new("/repo"),
            &[excluded("dist", "**/dist/**", 3)],
        )
        .expect("note");
        assert!(note.contains("fallow --root"), "{note}");
        assert!(
            note.contains("cannot be switched off through ignorePatterns"),
            "the union only ever adds, so a negation is never the remedy: {note}"
        );
    }

    #[test]
    fn a_root_anchored_exclusion_renders_as_dot() {
        let note = build_default_ignore_exclusion_note(
            Path::new("/repo"),
            &[excluded("", "**/*.min.js", 1)],
        )
        .expect("note");
        assert!(note.contains("(mostly under .)"), "{note}");
    }
}
