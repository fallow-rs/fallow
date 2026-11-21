//! The inputs that the base pass of `fallow audit` and its cache key share.

use std::path::{Path, PathBuf};

use fallow_config::AuditGate;
use rustc_hash::FxHashSet;

use super::cache::BaseKeyRequest;
use super::{AuditOptions, CliAuditBackend};

/// The inputs of the base pass that do not depend on the base worktree. The
/// cache key is computed before the worktree exists, so the base pass and the
/// key both read this value.
pub(super) struct BaseRunInputs {
    /// The config file of the head run. The base pass loads the same file.
    pub(super) config_path: Option<PathBuf>,
    /// The coverage inputs of the base pass.
    pub(super) coverage: fallow_api::audit_run::BaseCoverageInputs,
}

impl BaseRunInputs {
    pub(super) fn resolve(opts: &AuditOptions<'_>) -> Self {
        Self {
            config_path: opts
                .config_path
                .clone()
                .or_else(|| fallow_engine::project_config::find_config_file(opts.root)),
            coverage: fallow_api::audit_run::base_coverage_inputs(
                opts.root,
                opts.coverage,
                opts.coverage_root,
            ),
        }
    }

    /// Build the `AuditOptions` of a base pass at `base_root`.
    #[expect(
        clippy::ref_option,
        reason = "AuditOptions.config_path is &Option<PathBuf>; the borrow is stored into the returned struct"
    )]
    pub(super) fn options<'a>(
        &'a self,
        opts: &AuditOptions<'a>,
        base_root: &'a Path,
        config_path: &'a Option<PathBuf>,
        base_cache_dir: &'a Path,
    ) -> AuditOptions<'a> {
        AuditOptions {
            root: base_root,
            config_path,
            cache_dir: base_cache_dir,
            output: opts.output,
            json_style: opts.json_style,
            no_cache: opts.no_cache,
            threads: opts.threads,
            quiet: true,
            allow_remote_extends: opts.allow_remote_extends,
            changed_since: None,
            production: opts.production,
            production_dead_code: opts.production_dead_code,
            production_health: opts.production_health,
            production_dupes: opts.production_dupes,
            workspace: opts.workspace,
            changed_workspaces: None,
            explain: false,
            explain_skipped: false,
            performance: false,
            group_by: opts.group_by,
            dead_code_baseline: None,
            health_baseline: None,
            dupes_baseline: None,
            health_baseline_mode: fallow_engine::baseline::HealthBaselineMode::default(),
            fail_on_stale_baseline: false,
            max_crap: opts.max_crap,
            file_lines_check: opts.file_lines_check,
            coverage: self.coverage.coverage.as_deref(),
            coverage_root: self.coverage.coverage_root.as_deref(),
            gate: AuditGate::All,
            worsened: fallow_config::AuditWorsened::Warn,
            include_entry_exports: opts.include_entry_exports,
            show_cascade: opts.show_cascade,
            fail_on_parse_error: false,
            analyses: opts.analyses,
            // Base styling keys keep opt-in `rules.css-* = error` gated on
            // introduced findings only; the base snapshot is cached.
            css: opts.css,
            css_deep: opts.css_deep,
            runtime_coverage: None,
            min_invocations_hot: opts.min_invocations_hot,
            mutation_report: None,
            brief: false,
            max_decisions: 4,
            walkthrough_guide: false,
            walkthrough: false,
            mark_viewed: &[],
            show_cleared: false,
            walkthrough_file: None,
            show_deprioritized: false,
            // Deliberately unscoped: the base pass runs in another worktree whose
            // path spelling differs, so a head-root scope would narrow the base
            // focus to empty and misattribute everything as introduced. The head
            // pass is already scope-narrowed; a full base snapshot joins correctly
            // against it.
            scope: None,
        }
    }
}

impl CliAuditBackend<'_> {
    /// The cache key inputs of the base pass for `base_ref` and the changed
    /// files `focus`.
    pub(super) fn base_key_request<'b>(
        &'b self,
        base_ref: &'b str,
        focus: &'b FxHashSet<PathBuf>,
    ) -> BaseKeyRequest<'b> {
        BaseKeyRequest {
            opts: self.opts,
            base_run: self.base_run(),
            base_ref,
            changed_files: focus,
            workspace_patterns: self.workspace_patterns(),
            head: self.head,
            type_aware: self.type_aware,
        }
    }
}

#[cfg(test)]
#[path = "audit_base_run_tests.rs"]
mod tests;
