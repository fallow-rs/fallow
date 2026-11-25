//! The diagnostics that withhold `low_confidence_incomplete_analysis` fixes,
//! and the step that unblocks each one.
//!
//! A fix carries that skip reason when its finding has a reachability caveat.
//! The caveat names the consequence, not the cause, so without this list the
//! only hint was "resolve the workspace diagnostics". That list also holds
//! diagnostics that withhold no fix (`undeclared-workspace`), so a user could
//! resolve every listed diagnostic and still get no fix (issue #5127).

use std::path::Path;

use fallow_config::{ResolvedConfig, WorkspaceDiagnostic, WorkspaceDiagnosticKind};
use fallow_output::IncompleteAnalysisDiagnostic;

use super::plan::SkipReason;

/// Lines of the human list before the rest collapses into one count line.
const HUMAN_LIST_CAP: usize = 10;

/// Select the diagnostics of the run that can raise a reachability caveat,
/// each with its action. The list is empty when no fix of `fixes` is withheld
/// for an incomplete analysis.
///
/// A file that the run never read comes first. Its caveat reaches every
/// verdict of the run. A parse failure comes after it: the caveat pass
/// narrows a parse failure in an unreachable module to member and
/// dependency verdicts, so it is the less likely cause of a withheld export.
pub(super) fn blocking_diagnostics(
    config: &ResolvedConfig,
    diagnostics: &[WorkspaceDiagnostic],
    fixes: &[serde_json::Value],
) -> Vec<IncompleteAnalysisDiagnostic> {
    if !fixes.iter().any(is_incomplete_analysis_skip) {
        return Vec::new();
    }
    let mut blocking: Vec<IncompleteAnalysisDiagnostic> = diagnostics
        .iter()
        .filter(|diagnostic| {
            !is_parse_failure(&diagnostic.kind)
                || !config
                    .ignore_findings
                    .is_ignored_path(&config.root, &diagnostic.path)
        })
        .filter_map(|diagnostic| {
            let relative = diagnostic.clone().into_root_relative(&config.root);
            let action = action(&relative.kind, &display(&relative.path))?;
            Some(IncompleteAnalysisDiagnostic {
                diagnostic: relative,
                action,
            })
        })
        .collect();
    sort_and_dedup(&mut blocking);
    blocking
}

/// Put files that the run never read before parse failures, then sort by
/// path, kind and action, and drop duplicates.
fn sort_and_dedup(blocking: &mut Vec<IncompleteAnalysisDiagnostic>) {
    blocking.sort_by(|a, b| {
        (
            is_parse_failure(&a.diagnostic.kind),
            &a.diagnostic.path,
            a.diagnostic.kind.id(),
            &a.action,
        )
            .cmp(&(
                is_parse_failure(&b.diagnostic.kind),
                &b.diagnostic.path,
                b.diagnostic.kind.id(),
                &b.action,
            ))
    });
    blocking.dedup_by(|a, b| a.diagnostic.path == b.diagnostic.path && a.action == b.action);
}

/// Replace the generic help of each withheld entry with one that names the
/// blocking diagnostic and its action. With no known cause the generic help
/// stays.
pub(super) fn name_blocking_diagnostics_in_help(
    fixes: &mut [serde_json::Value],
    blocking: &[IncompleteAnalysisDiagnostic],
) {
    let Some(help) = specific_help(blocking) else {
        return;
    };
    for fix in fixes
        .iter_mut()
        .filter(|fix| is_incomplete_analysis_skip(fix))
    {
        fix["help"] = serde_json::Value::String(help.clone());
    }
}

/// Print the blocking diagnostics with their actions on stderr. This names a
/// fix that fallow refused to make, so `--quiet` does not drop it.
pub(super) fn print_blocking_diagnostics(
    fixes: &[serde_json::Value],
    blocking: &[IncompleteAnalysisDiagnostic],
) {
    if blocking.is_empty() {
        if fixes.iter().any(is_incomplete_analysis_skip) {
            eprintln!("{}", SkipReason::LowConfidenceIncompleteAnalysis.help());
        }
        return;
    }
    eprintln!(
        "Fixes were held back because fallow did not fully analyze {}:",
        if blocking.len() == 1 {
            "this path"
        } else {
            "these paths"
        }
    );
    for entry in blocking.iter().take(HUMAN_LIST_CAP) {
        eprintln!(
            "  - {} ({}): {}",
            display(&entry.diagnostic.path),
            entry.diagnostic.kind.id(),
            entry.action
        );
    }
    if blocking.len() > HUMAN_LIST_CAP {
        eprintln!(
            "  - and {} more (run with --format json for each one).",
            blocking.len() - HUMAN_LIST_CAP
        );
    }
    eprintln!("Then run `fallow fix --dry-run` and review the new preview.");
}

fn is_incomplete_analysis_skip(fix: &serde_json::Value) -> bool {
    fix.get("skip_reason").and_then(serde_json::Value::as_str)
        == Some(SkipReason::LowConfidenceIncompleteAnalysis.as_wire_str())
}

fn specific_help(blocking: &[IncompleteAnalysisDiagnostic]) -> Option<String> {
    let first = blocking.first()?;
    let path = display(&first.diagnostic.path);
    let cause = if blocking.len() == 1 {
        format!("fallow did not fully analyze `{path}`")
    } else {
        format!(
            "fallow did not fully analyze {} paths (see `incomplete_analysis_diagnostics`), the first is `{path}`",
            blocking.len()
        )
    };
    Some(format!(
        "Preserved because the import graph is incomplete: {cause}. {} Then run `fallow fix --dry-run` and review the new preview.",
        first.action
    ))
}

/// A parse failure raises a caveat only when `ignoreFindings` does not cover
/// the file. The caveat pass skips a covered file (see `GraphConfidenceContext`).
const fn is_parse_failure(kind: &WorkspaceDiagnosticKind) -> bool {
    matches!(
        kind,
        WorkspaceDiagnosticKind::SourceParseDegraded { .. }
            | WorkspaceDiagnosticKind::SourceNestingTooDeep { .. }
    )
}

/// The step that lets the run analyze `path`, for each kind that can raise a
/// reachability caveat. `None` for every other kind.
fn action(kind: &WorkspaceDiagnosticKind, path: &str) -> Option<String> {
    let text = match kind {
        WorkspaceDiagnosticKind::SkippedSourceDotdir => format!(
            "Fallow skips hidden folders. To analyze this one, add `!{path}/**` to `ignorePatterns` in the fallow config."
        ),
        WorkspaceDiagnosticKind::SkippedLargeFile { .. } => format!(
            "Fallow skipped this large file. Raise the limit with `--max-file-size <MB>`, or add `{path}` to `ignorePatterns`."
        ),
        WorkspaceDiagnosticKind::SkippedMinifiedFile { .. } => format!(
            "Fallow skipped this file as minified code. Add `{path}` to `ignorePatterns`, or rename it with a `.min.js` suffix."
        ),
        WorkspaceDiagnosticKind::SourceReadFailure { .. } => format!(
            "Fallow could not read this file. Restore the file or its read permissions, or add `{path}` to `ignorePatterns`."
        ),
        WorkspaceDiagnosticKind::SourceParseDegraded { .. } => format!(
            "This file has syntax errors. Fix the syntax, or add `{path}` to `ignoreFindings` if the file is broken on purpose."
        ),
        WorkspaceDiagnosticKind::SourceNestingTooDeep { .. } => format!(
            "Fallow did not parse this file because it nests too deep. Reduce its nesting, or add `{path}` to `ignoreFindings` if it is generated code."
        ),
        _ => return None,
    };
    Some(text)
}

fn display(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_kind_that_raises_a_caveat_has_an_action() {
        let kinds = [
            WorkspaceDiagnosticKind::SkippedSourceDotdir,
            WorkspaceDiagnosticKind::SkippedLargeFile { size_bytes: 1 },
            WorkspaceDiagnosticKind::SkippedMinifiedFile { size_bytes: 1 },
            WorkspaceDiagnosticKind::SourceReadFailure {
                error: String::new(),
            },
            WorkspaceDiagnosticKind::SourceParseDegraded {
                error_count: 1,
                panicked: false,
            },
            WorkspaceDiagnosticKind::SourceNestingTooDeep { limit: 1 },
        ];
        for kind in kinds {
            assert!(action(&kind, "a").is_some(), "{} has no action", kind.id());
        }
        assert!(action(&WorkspaceDiagnosticKind::UndeclaredWorkspace, "a").is_none());
    }

    #[test]
    fn a_file_the_run_never_read_comes_before_a_parse_failure() {
        let root = Path::new("/r");
        let entry = |path: &str, kind: WorkspaceDiagnosticKind| {
            let action = action(&kind, path).unwrap();
            IncompleteAnalysisDiagnostic {
                diagnostic: WorkspaceDiagnostic::new(root, root.join(path), kind)
                    .into_root_relative(root),
                action,
            }
        };
        let parse = || WorkspaceDiagnosticKind::SourceParseDegraded {
            error_count: 1,
            panicked: false,
        };
        // `lib/a.ts` sorts before `src/.tool` by path alone, so only the
        // rank of the kind puts the hidden folder first.
        let mut blocking = vec![
            entry("lib/a.ts", parse()),
            entry("src/.tool", WorkspaceDiagnosticKind::SkippedSourceDotdir),
            entry("lib/a.ts", parse()),
            entry(".b", WorkspaceDiagnosticKind::SkippedSourceDotdir),
        ];
        sort_and_dedup(&mut blocking);
        let paths: Vec<String> = blocking
            .iter()
            .map(|entry| display(&entry.diagnostic.path))
            .collect();
        assert_eq!(paths, [".b", "src/.tool", "lib/a.ts"]);
    }

    #[test]
    fn the_minified_file_action_does_not_lift_every_size_limit() {
        let text = action(
            &WorkspaceDiagnosticKind::SkippedMinifiedFile { size_bytes: 1 },
            "a.js",
        )
        .unwrap();
        assert!(!text.contains("--max-file-size"), "{text}");
    }

    #[test]
    fn help_counts_the_paths_and_names_the_first() {
        let root = Path::new("/r");
        let entry = |path: &str| IncompleteAnalysisDiagnostic {
            diagnostic: WorkspaceDiagnostic::new(
                root,
                root.join(path),
                WorkspaceDiagnosticKind::SkippedSourceDotdir,
            )
            .into_root_relative(root),
            action: format!("Add `!{path}/**` to `ignorePatterns`."),
        };
        let help = specific_help(&[entry(".a"), entry(".b")]).unwrap();
        assert!(help.contains("2 paths"), "{help}");
        assert!(help.contains("the first is `.a`"), "{help}");
        assert!(help.contains("Add `!.a/**`"), "{help}");
        assert!(specific_help(&[]).is_none());
    }
}
