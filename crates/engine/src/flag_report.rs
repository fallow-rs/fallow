//! One entry point that builds the `fallow flags --retirement` report.
//!
//! The CLI and the programmatic API (and through it the MCP server) call this
//! function, so the two surfaces run the same steps in the same order: group
//! the sites into rows, measure the age, apply the vendor export, compute the
//! age gate, then filter, sort and limit the rows.

use std::path::Path;

use fallow_config::WorkspaceInfo;
use fallow_types::flag_retirement::{FlagAgeMode, FlagRetirementReport};
use fallow_types::workspace::WorkspaceDiagnosticKind;
use rustc_hash::FxHashSet;

use crate::clock::AnalysisClock;
use crate::flag_age::{FlagAgeRequest, PickaxeProgress, apply_flag_ages};
use crate::flag_retirement::{
    RetirementOptions, RetirementSiteInput, aggregate_flags, finish_report, max_age_gate,
};
use crate::flag_vendor::{VendorExport, VendorMatch, apply_vendor_state};

/// Inputs of one retirement report.
pub struct RetirementRequest<'a> {
    /// Project root. Paths in the report are relative to it.
    pub root: &'a Path,
    /// Workspaces of the project, for the flag identity.
    pub workspaces: &'a [WorkspaceInfo],
    /// Every flag site of the project, also outside the scope of the run.
    pub sites: Vec<RetirementSiteInput>,
    /// Whether a file is in the scope of the run.
    pub in_scope: &'a dyn Fn(&Path) -> bool,
    /// Whether the run covers the whole project. Only such a run adds
    /// `vendor-only` rows.
    pub whole_project: bool,
    /// How to measure flag age.
    pub age_mode: FlagAgeMode,
    /// Directory for the age cache, or `None` to run without the cache.
    pub cache_dir: Option<&'a Path>,
    /// Receives pickaxe progress.
    pub progress: Option<&'a (dyn Fn(PickaxeProgress) + Sync)>,
    /// The `--flag-state` export, if any.
    pub vendor_export: Option<&'a VendorExport>,
    /// `flags.vendorKeyPrefix`.
    pub vendor_key_prefix: Option<&'a str>,
    /// `--max-flag-age`, in days.
    pub max_flag_age: Option<u64>,
    /// Filters, order and limit of the rows.
    pub options: RetirementOptions,
}

/// A retirement report and the reasons that ages are missing.
pub struct RetirementBuild {
    /// The report.
    pub report: FlagRetirementReport,
    /// Why no age was measured, when git history was not available.
    pub diagnostics: Vec<WorkspaceDiagnosticKind>,
}

/// Build the retirement report.
#[must_use]
pub fn build_retirement_report(request: RetirementRequest<'_>) -> RetirementBuild {
    let root = request.root;
    let code_flag_names: FxHashSet<String> = request
        .sites
        .iter()
        .map(|site| site.flag_name.clone())
        .collect();
    let mut rows = aggregate_flags(request.sites, root, request.workspaces, request.in_scope);
    let age = apply_flag_ages(
        &mut rows,
        &FlagAgeRequest {
            root,
            mode: request.age_mode,
            cache_dir: request.cache_dir,
            progress: request.progress,
        },
    );
    let vendor_state = request.vendor_export.map(|export| {
        apply_vendor_state(
            &mut rows,
            &VendorMatch {
                export,
                key_prefix: request.vendor_key_prefix,
                code_flag_names: &code_flag_names,
                add_vendor_only: request.whole_project,
                clock_epoch_secs: AnalysisClock::for_repo(root).epoch_secs(),
            },
        )
    });
    let max_flag_age = request.max_flag_age.map(|days| max_age_gate(&rows, days));
    let mut report = finish_report(
        rows,
        request.age_mode,
        age.generated_at_clock,
        &request.options,
    );
    report.vendor_state = vendor_state;
    report.max_flag_age = max_flag_age;
    RetirementBuild {
        report,
        diagnostics: age.diagnostics,
    }
}
