//! Notes about a loaded dead-code baseline that no report states by itself.
//!
//! Each note prints regardless of `--quiet`: it explains findings that the
//! report shows as new, and `--ci` implies `--quiet`.

use fallow_config::ResolvedConfig;
use fallow_output::{BaselineMode, BaselineScopeReasons, ScopeReason};
use fallow_types::workspace::WorkspaceDiagnostic;

use super::{BaselineIo, IssueFilters};
use crate::baseline_gate::LoadedBaselineStaleness;

impl IssueFilters {
    /// Whether an active filter keeps the findings of one dead-code baseline
    /// field. Mirrors [`Self::apply`]. A field with no mapping here reads as not
    /// reported, so a new field keeps the run inert on a file that holds it.
    pub fn reports_baseline_category(&self, category: &str) -> bool {
        match category {
            "unused_files" => self.unused_files,
            "unused_exports" => self.unused_exports,
            "unused_types" => self.unused_types,
            "private_type_leaks" => self.private_type_leaks,
            "deprecated_exports_in_use" => self.deprecated_exports_in_use,
            "unused_dependencies"
            | "unused_dev_dependencies"
            | "unused_optional_dependencies"
            | "type_only_dependencies"
            | "test_only_dependencies"
            | "dev_dependencies_in_production" => self.unused_deps,
            "unused_enum_members" => self.unused_enum_members,
            "unused_class_members" => self.unused_class_members,
            "unused_store_members" => self.unused_store_members,
            "unlisted_dependencies" => self.unlisted_deps,
            "unprovided_injects" => self.unprovided_injects,
            "unrendered_components" => self.unrendered_components,
            "absent_component_props" => self.absent_component_props,
            "unused_component_props" => self.unused_component_props,
            "unused_component_emits" => self.unused_component_emits,
            "unused_component_inputs" => self.unused_component_inputs,
            "unused_component_outputs" => self.unused_component_outputs,
            "unused_svelte_events" => self.unused_svelte_events,
            "unused_effect_layers" => self.unused_effect_layers,
            "unused_tagged_errors" => self.unused_tagged_errors,
            "unproduced_catch_tags" => self.unproduced_catch_tags,
            "unused_server_actions" => self.unused_server_actions,
            "unused_load_data_keys" => self.unused_load_data_keys,
            "unresolved_imports" => self.unresolved_imports,
            "invalid_client_exports" => self.invalid_client_exports,
            "mixed_client_server_barrels" => self.mixed_client_server_barrels,
            "misplaced_directives" => self.misplaced_directives,
            "route_collisions" => self.route_collisions,
            "dynamic_segment_name_conflicts" => self.dynamic_segment_name_conflicts,
            "duplicate_exports" => self.duplicate_exports,
            "circular_dependencies" => self.circular_deps,
            "re_export_cycles" => self.re_export_cycles,
            "package_cycles" => self.package_cycles,
            "boundary_violations" | "boundary_coverage_violations" | "boundary_call_violations" => {
                self.boundary_violations
            }
            "policy_violations" => self.policy_violations,
            "stale_suppressions" => self.stale_suppressions,
            "unused_catalog_entries" => self.unused_catalog_entries,
            "empty_catalog_groups" => self.empty_catalog_groups,
            "unresolved_catalog_references" => self.unresolved_catalog_references,
            "unused_dependency_overrides" => self.unused_dependency_overrides,
            "misconfigured_dependency_overrides" => self.misconfigured_dependency_overrides,
            _ => false,
        }
    }
}

/// What the comparison of one run with one saved file found.
pub(super) struct BaselineComparison {
    /// The channels that stop this run from judging the file. Empty means the
    /// run can judge it.
    pub(super) reasons: BaselineScopeReasons,
    /// The mode the file records.
    pub(super) saved_mode: BaselineMode,
    /// The non-empty categories of the file that the filter does not report.
    pub(super) uncovered_categories: Vec<String>,
}

/// Compare the channels of this run with the file, one channel at a time.
///
/// A saved `production` or `include-entry-exports` describes its channel
/// completely, so the channel counts only when one side uses it. An issue-type
/// filter counts only when the file holds a category that the filter does not
/// report. Every other channel always stays, because the file records its name
/// and not its value. A file with no record reads as a file saved without the
/// channel, which keeps a production run inert on an old file.
pub(super) fn compare_with_saved_baseline(
    content: &str,
    run: BaselineScopeReasons,
    filters: &IssueFilters,
) -> BaselineComparison {
    let saved_names = fallow_engine::baseline::saved_scope_reasons(content);
    let saved_mode = BaselineMode::from_saved_names(&saved_names);
    let run_mode = BaselineMode::from_reasons(run);
    let uncovered_categories: Vec<String> = if run.contains(ScopeReason::IssueTypeFilter) {
        fallow_engine::baseline::saved_finding_categories(content)
            .into_iter()
            .filter(|category| !filters.reports_baseline_category(category))
            .collect()
    } else {
        Vec::new()
    };
    let reasons = run
        .without(ScopeReason::Production)
        .without(ScopeReason::IncludeEntryExports)
        .insert_if(
            saved_mode.production != run_mode.production,
            ScopeReason::Production,
        )
        .insert_if(
            saved_mode.include_entry_exports != run_mode.include_entry_exports,
            ScopeReason::IncludeEntryExports,
        );
    let reasons = if uncovered_categories.is_empty() {
        reasons.without(ScopeReason::IssueTypeFilter)
    } else {
        reasons
    };
    BaselineComparison {
        reasons,
        saved_mode,
        uncovered_categories,
    }
}

/// A baseline saved from a narrowed run holds only part of the findings. A
/// wider run that loads it reports the findings outside the saved scope as
/// new, so the run says why before the report does.
///
/// Printed regardless of `--quiet`, like the note for a baseline of another
/// format: no report states this fact, and `--ci` implies `--quiet`.
pub(super) fn warn_about_a_narrower_saved_baseline(
    content: &str,
    baseline_path: &std::path::Path,
    io: &BaselineIo<'_>,
) {
    let current: Vec<&str> = io
        .scope_reasons
        .iter()
        .map(fallow_output::ScopeReason::as_str)
        .collect();
    let wider: Vec<String> = fallow_engine::baseline::saved_scope_reasons(content)
        .into_iter()
        .filter(|reason| !current.contains(&reason.as_str()))
        .collect();
    if wider.is_empty() {
        return;
    }
    let only_modes = wider.iter().all(|reason| {
        reason == ScopeReason::Production.as_str()
            || reason == ScopeReason::IncludeEntryExports.as_str()
    });
    let remedy = if only_modes {
        " Add the same mode flags to compare with that scope."
    } else if wider.iter().any(|reason| reason == "package-baselines") {
        " Save it again with --no-package-baselines to cover the whole project."
    } else {
        " Save it again from a run without that narrowing to cover the whole project."
    };
    eprintln!(
        "Warning: the baseline {} was saved from a run narrowed by {}, so it lists only part of \
         the findings. This run is wider and can report old findings as new.{remedy}",
        baseline_path.display(),
        wider.join(", "),
    );
}

/// The file suppresses nothing, so every finding stays in the report and the
/// counts say the baseline carried no entry (issue #2738).
pub(super) fn note_unrecognised_file(
    baseline_path: &std::path::Path,
    saved_by: Option<fallow_engine::baseline::BaselineKind>,
    io: &BaselineIo<'_>,
) {
    if io.bare_run {
        crate::baseline_gate::note_unrecognised_bare_run_baseline(
            Some(baseline_path),
            true,
            saved_by,
        );
    } else {
        crate::baseline_gate::note_unrecognised_baseline(
            Some(baseline_path),
            true,
            saved_by,
            fallow_engine::baseline::BaselineKind::DeadCode,
            io.load_flag,
        );
    }
}

/// A baseline saved with `node_modules` present and compared without it, or
/// the reverse, can report baseline findings as new. Returns the
/// `baseline-install-state-mismatch` diagnostic for the envelope, and prints
/// its message.
///
/// A legacy baseline does not record its install state, so it gives no note.
pub(super) fn warn_about_another_install_state(
    content: &str,
    baseline_path: &std::path::Path,
    io: &BaselineIo<'_>,
) -> Option<WorkspaceDiagnostic> {
    let diagnostic = fallow_engine::baseline::install_state_diagnostic(
        io.root,
        baseline_path,
        content,
        fallow_config::node_modules_state(io.root),
    )?;
    eprintln!("Warning: {}", diagnostic.message);
    Some(diagnostic)
}

/// The notes that the run adds to `workspace_diagnostics`: the unmatched
/// config patterns, and the install-state note of the loaded baseline.
pub(super) fn run_notes(
    config: &ResolvedConfig,
    reports_dependencies: bool,
    loaded: Option<&LoadedBaselineStaleness>,
) -> Vec<WorkspaceDiagnostic> {
    let mut notes =
        fallow_engine::dead_code::config_pattern_diagnostics(config, reports_dependencies);
    notes.extend(loaded.and_then(|loaded| loaded.install_state.clone()));
    notes
}

/// Warn when a loaded baseline no longer describes the current project: either
/// nothing in it matched, or a large enough share of it matched nothing.
///
/// Every guard lives in the shared decision, so `dead-code`, `dupes` and
/// `health` are silent in the same situations: a run narrowed to part of the
/// project (re-saving from one would drop every entry the run never looked at)
/// and a run that produced no findings at all (where a cleaned project and a
/// rotted baseline are indistinguishable from here).
pub(super) fn warn_on_staleness(
    staleness: crate::baseline::BaselineStaleness,
    baseline_path: &std::path::Path,
    saved_mode: fallow_output::BaselineMode,
) {
    let baseline_entries = staleness.entries;
    let stale_entries = staleness.stale_entries();
    match staleness.warning() {
        crate::baseline::BaselineStalenessWarning::None => {}
        crate::baseline::BaselineStalenessWarning::ZeroOverlap => eprintln!(
            "Warning: baseline has {baseline_entries} entries but matched \
             0 current issues. Your paths may have changed, or the baseline \
             was saved on a different machine. Re-save with: \
             --save-baseline {}",
            baseline_path.display(),
        ),
        crate::baseline::BaselineStalenessWarning::Partial => eprintln!(
            "Warning: baseline is partially stale: {stale_entries} of \
             {baseline_entries} entries matched no current issue, so the \
             gate protects less than what was saved. Remove only the stale \
             entries with: {}, or re-save with: --save-baseline {}",
            fallow_engine::baseline::BaselineKind::DeadCode
                .prune_command(baseline_path, saved_mode),
            baseline_path.display(),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(reasons: &[ScopeReason]) -> BaselineScopeReasons {
        reasons
            .iter()
            .fold(BaselineScopeReasons::empty(), |set, reason| {
                set.with(*reason)
            })
    }

    fn file(saved: &[&str], categories: &[&str]) -> String {
        let mut value = serde_json::json!({ "scope_reasons": saved });
        for category in categories {
            value[*category] = serde_json::json!(["entry"]);
        }
        value.to_string()
    }

    fn filter(flag: &str) -> IssueFilters {
        let mut filters = IssueFilters::default();
        assert!(filters.enable_cli_filter_flag(flag), "{flag}");
        filters
    }

    fn reasons(content: &str, run_set: BaselineScopeReasons, filters: &IssueFilters) -> String {
        compare_with_saved_baseline(content, run_set, filters)
            .reasons
            .join()
    }

    #[test]
    fn a_complete_channel_counts_only_when_one_side_uses_it() {
        let none = IssueFilters::default();
        for (reason, name) in [
            (ScopeReason::Production, "production"),
            (ScopeReason::IncludeEntryExports, "include-entry-exports"),
        ] {
            assert_eq!(reasons(&file(&[name], &[]), run(&[reason]), &none), "");
            assert_eq!(reasons(&file(&[], &[]), run(&[]), &none), "");
            assert_eq!(reasons(&file(&[], &[]), run(&[reason]), &none), name);
            assert_eq!(reasons(&file(&[name], &[]), run(&[]), &none), name);
        }
    }

    #[test]
    fn a_channel_with_a_value_always_stays() {
        let none = IssueFilters::default();
        for (reason, name) in [
            (ScopeReason::Diff, "diff"),
            (ScopeReason::ChangedSince, "changed-since"),
            (ScopeReason::PackageBaselines, "package-baselines"),
            (ScopeReason::Workspace, "workspace"),
            (ScopeReason::ChangedWorkspaces, "changed-workspaces"),
            (ScopeReason::Scope, "scope"),
            (ScopeReason::File, "file"),
        ] {
            assert_eq!(reasons(&file(&[name], &[]), run(&[reason]), &none), name);
        }
    }

    #[test]
    fn the_filter_counts_only_when_the_file_holds_another_category() {
        let exports = filter("--unused-exports");
        let filtered = run(&[ScopeReason::IssueTypeFilter]);
        assert_eq!(
            reasons(&file(&[], &["unused_exports"]), filtered, &exports),
            ""
        );
        let comparison = compare_with_saved_baseline(
            &file(&[], &["unused_exports", "unused_files"]),
            filtered,
            &exports,
        );
        assert_eq!(comparison.reasons.join(), "issue-type-filter");
        assert_eq!(comparison.uncovered_categories, ["unused_files"]);
    }

    #[test]
    fn one_filter_flag_can_cover_several_categories() {
        let deps = filter("--unused-deps");
        let filtered = run(&[ScopeReason::IssueTypeFilter]);
        let content = file(
            &[],
            &[
                "unused_dependencies",
                "unused_dev_dependencies",
                "unused_optional_dependencies",
                "type_only_dependencies",
                "test_only_dependencies",
                "dev_dependencies_in_production",
            ],
        );
        assert_eq!(reasons(&content, filtered, &deps), "");
    }

    /// `prop_drilling_chains` has no filter flag, so a filtered run cannot say
    /// that it covers the field.
    #[test]
    fn a_category_with_no_filter_flag_keeps_the_reason() {
        let exports = filter("--unused-exports");
        let filtered = run(&[ScopeReason::IssueTypeFilter]);
        assert_eq!(
            reasons(
                &file(&[], &["unused_exports", "prop_drilling_chains"]),
                filtered,
                &exports
            ),
            "issue-type-filter"
        );
    }
}
