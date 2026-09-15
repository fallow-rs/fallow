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

use colored::Colorize as _;

use fallow_config::{OutputFormat, WorkspaceDiagnostic, WorkspaceDiagnosticKind};
use fallow_types::workspace::glob_first_literal_segment;

/// One rendered row of the note: the count, the built-in glob, where the
/// largest group sat, and how many directories the pattern touched.
struct ExclusionRow<'a> {
    file_count: u32,
    directory_count: u32,
    pattern: &'a str,
    directory: String,
}

impl ExclusionRow<'_> {
    /// True when re-rooting inside the matched directory lifts this pattern.
    /// A file-name glob keeps matching at every root, so it gets a different
    /// remedy line rather than a `--root` command that does nothing.
    fn is_directory_shaped(&self) -> bool {
        glob_first_literal_segment(self.pattern).is_some()
    }
}

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
    let rows: Vec<ExclusionRow<'_>> = diagnostics
        .iter()
        .filter_map(|diagnostic| match &diagnostic.kind {
            WorkspaceDiagnosticKind::ExcludedByDefaultIgnore {
                pattern,
                file_count,
                directory_count,
            } => Some(ExclusionRow {
                file_count: *file_count,
                directory_count: *directory_count,
                pattern: pattern.as_str(),
                directory: display_relative(root, &diagnostic.path),
            }),
            _ => None,
        })
        .collect();
    if rows.is_empty() {
        return None;
    }

    let total: u64 = rows.iter().map(|row| u64::from(row.file_count)).sum();
    let noun = if total == 1 { "file" } else { "files" };
    let mut note = format!(
        "note: skipped {total} source {noun} matching fallow's built-in discovery ignores:"
    );
    for row in &rows {
        let count = row.file_count;
        let pattern = row.pattern;
        let directory = &row.directory;
        // "largest group", never "mostly": one excluded file in each of ten
        // sibling package directories makes every one of them the largest, and
        // a majority claim there is simply false.
        let scope = if row.directory_count > 1 {
            format!(
                "{directory} (largest of {} directories)",
                row.directory_count
            )
        } else {
            directory.clone()
        };
        let _ = write!(note, "\n  {count:>5}  {pattern}  {scope}");
    }
    note.push_str("\n  built-in ignores cannot be switched off through ignorePatterns");
    if rows.iter().any(ExclusionRow::is_directory_shaped) {
        note.push_str(
            "\n  a directory pattern lifts when you analyze that directory on its own: \
             fallow --root <dir>",
        );
    }
    if rows.iter().any(|row| !row.is_directory_shaped()) {
        note.push_str(
            "\n  a file-name pattern matches at any root, so --root does not help; rename \
             first-party source that only looks generated",
        );
    }
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

/// Build the default (unflagged) warning for a run that discovered no source
/// files while a built-in ignore pattern excluded some, or `None` when either
/// half of that is untrue.
///
/// This is the one line the exclusions get outside `--explain-skipped`, and
/// the guard is what makes it safe: a project that discovered even one source
/// file never reaches it, so the note cannot become permanent noise on a
/// monorepo with a non-gitignored `dist/`. Without it the issue's headline
/// case, pointing fallow at a directory a built-in matches, still prints a
/// green "No issues found" with exit 0 and no hint that the flag exists.
///
/// The sentence reports two measured facts and joins them with a period, not
/// with a colon. The tally is not the only way a run ends with no files:
/// `--production` drops test-only source AFTER the ignore check, a skipped
/// hidden directory or a size or minification skip can take the rest, and none
/// of those is visible here. Naming the pattern as the cause would be wrong on
/// exactly those runs, and this is the line that fires without any flag, so a
/// false causal claim is more expensive here than anywhere else.
#[must_use]
pub fn build_all_source_excluded_warning(
    diagnostics: &[WorkspaceDiagnostic],
    discovered_file_count: usize,
    explain_skipped: bool,
) -> Option<String> {
    if discovered_file_count > 0 {
        return None;
    }
    let excluded: Vec<(u32, &str)> = diagnostics
        .iter()
        .filter_map(|diagnostic| match &diagnostic.kind {
            WorkspaceDiagnosticKind::ExcludedByDefaultIgnore {
                pattern,
                file_count,
                ..
            } => Some((*file_count, pattern.as_str())),
            _ => None,
        })
        .collect();
    let total: u64 = excluded.iter().map(|(count, _)| u64::from(*count)).sum();
    if total == 0 {
        return None;
    }
    let noun = if total == 1 { "file" } else { "files" };
    let subject = if let [(_, pattern)] = excluded.as_slice() {
        format!("The built-in ignore pattern '{pattern}'")
    } else {
        "The built-in ignore patterns".to_owned()
    };
    // The breakdown is already on the page when the flag is set, so pointing at
    // the flag there would be the only wrong half of the sentence.
    let pointer = if explain_skipped {
        "."
    } else {
        "; run with --explain-skipped for the breakdown."
    };
    Some(format!(
        "No source files were analyzed. {subject} excluded {total} {noun}{pointer}"
    ))
}

/// Print the all-source-excluded warning on stderr, on the human surface only.
pub fn print_all_source_excluded_warning(
    diagnostics: &[WorkspaceDiagnostic],
    discovered_file_count: usize,
    explain_skipped: bool,
    quiet: bool,
    output: OutputFormat,
) {
    if quiet || !matches!(output, OutputFormat::Human) {
        return;
    }
    if let Some(warning) =
        build_all_source_excluded_warning(diagnostics, discovered_file_count, explain_skipped)
    {
        eprintln!("{}", format!("  \u{26a0} {warning}").yellow());
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    fn excluded(path: &str, pattern: &str, file_count: u32) -> WorkspaceDiagnostic {
        scattered(path, pattern, file_count, 1)
    }

    fn scattered(
        path: &str,
        pattern: &str,
        file_count: u32,
        directory_count: u32,
    ) -> WorkspaceDiagnostic {
        WorkspaceDiagnostic::new(
            Path::new("/repo"),
            PathBuf::from("/repo").join(path),
            WorkspaceDiagnosticKind::ExcludedByDefaultIgnore {
                pattern: pattern.to_owned(),
                file_count,
                directory_count,
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
        assert!(note.contains("  **/dist/**  dist"), "{note}");
        assert!(
            !note.contains("mostly"),
            "a single directory holds all of them, so there is nothing to hedge: {note}"
        );
    }

    /// One excluded file in each of ten sibling packages makes every directory
    /// "the largest". The row says which claim it is making and how many
    /// directories it left unnamed.
    #[test]
    fn a_scattered_exclusion_row_says_it_names_the_largest_group() {
        let note = build_default_ignore_exclusion_note(
            Path::new("/repo"),
            &[scattered("packages/a/dist", "**/dist/**", 10, 10)],
        )
        .expect("note");
        assert!(
            note.contains("packages/a/dist (largest of 10 directories)"),
            "{note}"
        );
        assert!(!note.contains("mostly"), "{note}");
    }

    /// The `--root` line is advice for directory patterns only: re-running
    /// under `--root vendor` re-excludes `vendor/lib.min.js`.
    #[test]
    fn a_file_shaped_pattern_gets_the_rename_line_and_not_the_root_line() {
        let note = build_default_ignore_exclusion_note(
            Path::new("/repo"),
            &[excluded("vendor", "**/*.min.js", 2)],
        )
        .expect("note");
        assert!(
            !note.contains("fallow --root <dir>"),
            "the note explains why re-rooting fails, it does not prescribe it: {note}"
        );
        assert!(note.contains("rename first-party source"), "{note}");
    }

    /// Issue #2638's headline case: the whole source tree sat under a matched
    /// directory, so the run has nothing to report and the default output has
    /// to say why rather than printing a green result.
    #[test]
    fn a_run_that_discovered_nothing_names_the_pattern_that_took_everything() {
        let warning =
            build_all_source_excluded_warning(&[excluded("build", "**/build/**", 3)], 0, false)
                .expect("a run with no files and an exclusion warns");
        assert!(warning.contains("**/build/**"), "{warning}");
        assert!(warning.contains("3 files"), "{warning}");
        assert!(warning.contains("--explain-skipped"), "{warning}");
    }

    /// The line fires on every run that discovered nothing, and a built-in
    /// exclusion is not always the reason one did: production excludes, a
    /// skipped hidden directory, and a size or minification skip each empty
    /// the file list on their own, and none of them is in this tally. So the
    /// sentence states the two facts it measured and leaves the link to the
    /// reader instead of naming a cause it cannot establish.
    #[test]
    fn the_warning_states_measured_facts_and_asserts_no_cause() {
        let warning =
            build_all_source_excluded_warning(&[excluded("build", "**/build/**", 3)], 0, false)
                .expect("warning");
        assert_eq!(
            warning,
            "No source files were analyzed. The built-in ignore pattern '**/build/**' excluded \
             3 files; run with --explain-skipped for the breakdown."
        );
    }

    /// The guard that keeps this off every healthy project: one discovered
    /// source file is enough for the run to have something to say, and the
    /// exclusions go back to being flag-gated.
    #[test]
    fn a_run_that_discovered_files_stays_silent_about_exclusions() {
        assert!(
            build_all_source_excluded_warning(&[excluded("dist", "**/dist/**", 40)], 1, false)
                .is_none()
        );
    }

    /// An empty project is not an excluded project.
    #[test]
    fn a_run_with_no_files_and_no_exclusions_warns_about_nothing() {
        assert!(build_all_source_excluded_warning(&[], 0, false).is_none());
    }

    /// With several patterns no single one took everything, so the warning
    /// names the total and sends the reader to the breakdown.
    #[test]
    fn several_patterns_are_summarised_rather_than_named_one_by_one() {
        let warning = build_all_source_excluded_warning(
            &[
                excluded("build", "**/build/**", 3),
                excluded("dist", "**/dist/**", 1),
            ],
            0,
            false,
        )
        .expect("warning");
        assert!(warning.contains("built-in ignore patterns"), "{warning}");
        assert!(warning.contains("4 files"), "{warning}");
    }

    /// With the flag on, the per-pattern note is already on the page, so the
    /// warning must not send the reader after it.
    #[test]
    fn the_warning_drops_its_pointer_when_the_breakdown_is_already_printed() {
        let warning =
            build_all_source_excluded_warning(&[excluded("build", "**/build/**", 3)], 0, true)
                .expect("warning");
        assert!(!warning.contains("--explain-skipped"), "{warning}");
        assert!(warning.ends_with("excluded 3 files."), "{warning}");
    }

    /// A run that hits both shapes carries both remedy lines, each attached to
    /// the shape it is true for.
    #[test]
    fn a_mixed_run_carries_both_remedy_lines() {
        let note = build_default_ignore_exclusion_note(
            Path::new("/repo"),
            &[
                excluded("packages/web/build", "**/build/**", 2),
                excluded("vendor", "**/*.min.js", 1),
            ],
        )
        .expect("note");
        assert!(note.contains("fallow --root <dir>"), "{note}");
        assert!(note.contains("rename first-party source"), "{note}");
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
        assert_eq!(
            note.lines().count(),
            5,
            "one header, two rows, the ignorePatterns line, and the directory remedy"
        );
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
        assert!(
            note.contains("  **/*.min.js  ."),
            "an empty relative path is not a location: {note}"
        );
    }
}
