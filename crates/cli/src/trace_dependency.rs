//! `fallow trace --dependency <PACKAGE>`: how the code uses each imported name
//! of a package.
//!
//! The output is the `DependencyTrace` of `fallow dead-code
//! --trace-dependency` plus the `usage` object. The trace is syntactic and
//! stays off the ranked path. A name that no file imports is an answer with
//! zero counts, not an error. A cursor that belongs to another query exits 2.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use fallow_config::{OutputFormat, ProductionAnalysis};
use fallow_engine::trace::{DependencyTrace, DependencyUsageQuery, SitePageRequest};
use fallow_types::trace_usage::DEFAULT_USAGE_SITE_LIMIT;

use crate::error::{emit_error, emit_error_with_hint};
use crate::report;
use crate::{ConfigLoadOptions, load_config_for_analysis};

/// The `--dependency` target and its usage flags.
pub struct TraceDependencyTarget {
    /// The package to trace.
    pub package_name: String,
    /// `--specifier`: imported names to report.
    pub specifiers: Vec<String>,
    /// `--sites`: list the usage sites.
    pub sites: bool,
    /// `--limit`: the largest number of sites on a page.
    pub limit: Option<u16>,
    /// `--cursor`: the `next_cursor` of the previous page.
    pub cursor: Option<String>,
    /// `--callers`: add the consumer closure.
    pub callers: bool,
    /// `--depth`: the closure depth.
    pub depth: Option<u32>,
}

impl TraceDependencyTarget {
    /// Site mode is on when any of the site flags is given.
    const fn wants_sites(&self) -> bool {
        self.sites || !self.specifiers.is_empty() || self.limit.is_some() || self.cursor.is_some()
    }

    fn query(&self) -> Result<DependencyUsageQuery, fallow_types::trace_usage::UsageQueryError> {
        let sites = self.wants_sites().then(|| SitePageRequest {
            limit: self.limit.unwrap_or(DEFAULT_USAGE_SITE_LIMIT),
            cursor: self.cursor.clone(),
        });
        // `--depth` without `--callers` has no closure to bound, but a value
        // out of range is still an error, as for the symbol target.
        if let Some(depth) = self.depth
            && !(1..=fallow_types::trace_usage::MAX_USAGE_CLOSURE_DEPTH).contains(&depth)
        {
            return Err(fallow_types::trace_usage::UsageQueryError::DepthOutOfRange(
                depth,
            ));
        }
        let closure_depth = self.callers.then(|| {
            self.depth
                .unwrap_or(fallow_types::trace_chain::DEFAULT_TRACE_DEPTH)
        });
        DependencyUsageQuery::new(self.specifiers.clone(), sites, closure_depth)
    }
}

/// Options for `fallow trace --dependency`.
pub struct TraceDependencyOptions<'a> {
    pub root: &'a Path,
    pub config_path: &'a Option<PathBuf>,
    pub output: OutputFormat,
    pub json_style: crate::json_style::JsonStyle,
    pub no_cache: bool,
    pub threads: usize,
    pub quiet: bool,
    pub allow_remote_extends: bool,
    /// The resolved production mode for dead-code analysis.
    pub production: bool,
    /// `--workspace`, validated as in `fallow dead-code`.
    pub workspace: Option<&'a [String]>,
    /// `--changed-workspaces`, validated as in `fallow dead-code`.
    pub changed_workspaces: Option<&'a str>,
    pub target: TraceDependencyTarget,
}

/// Trace the dependency with its usage and emit it on the trace surface.
pub fn run_trace_dependency(opts: &TraceDependencyOptions<'_>) -> ExitCode {
    if !matches!(opts.output, OutputFormat::Json | OutputFormat::Human) {
        return unsupported_format(opts.output);
    }
    let query = match opts.target.query() {
        Ok(query) => query,
        Err(err) => return emit_error(&err.to_string(), 2, opts.output),
    };
    // `fallow dead-code --trace-dependency` validates the workspace scope the
    // same way. The scope does not narrow the trace itself.
    if let Err(code) = crate::check::resolve_workspace_scope(
        opts.root,
        opts.workspace,
        opts.changed_workspaces,
        opts.output,
    ) {
        return code;
    }
    let config = match load_config_for_analysis(
        opts.root,
        opts.config_path,
        ConfigLoadOptions {
            output: opts.output,
            no_cache: opts.no_cache,
            threads: opts.threads,
            production_override:
                fallow_engine::project_config::ProductionFlags::single_analysis_override(
                    opts.production,
                    Some(opts.production),
                ),
            quiet: opts.quiet,
            allow_remote_extends: opts.allow_remote_extends,
        },
        ProductionAnalysis::DeadCode,
    ) {
        Ok(config) => config,
        Err(code) => return code,
    };
    let session = match fallow_engine::session::AnalysisSession::from_resolved_config(config) {
        Ok(session) => session,
        Err(err) => return emit_error(&format!("Analysis error: {err}"), 2, opts.output),
    };
    let trace = match fallow_engine::trace::trace_dependency_with_session(
        &session,
        &opts.target.package_name,
        &query,
    ) {
        Ok(Ok(trace)) => trace,
        Ok(Err(err)) => {
            return emit_error_with_hint(
                &format!("--cursor: {err}"),
                "pass the `next_cursor` of a page from the same package and --specifier list",
                2,
                opts.output,
            );
        }
        Err(err) => return emit_error(&format!("Analysis error: {err}"), 2, opts.output),
    };
    emit(&trace, opts)
}

fn unsupported_format(output: OutputFormat) -> ExitCode {
    emit_error_with_hint(
        "trace --dependency supports --format json or human",
        "re-run with `--format json` for a machine-readable answer, or drop `--format`",
        2,
        output,
    )
}

fn emit(trace: &DependencyTrace, opts: &TraceDependencyOptions<'_>) -> ExitCode {
    match opts.output {
        OutputFormat::Json => {
            let value = match fallow_output::serialize_trace_json_output(
                trace,
                crate::output_runtime::telemetry_analysis_run_id().as_deref(),
            ) {
                Ok(value) => value,
                Err(err) => {
                    return emit_error(
                        &format!("failed to serialize trace output: {err}"),
                        2,
                        opts.output,
                    );
                }
            };
            report::emit_report_json(&value, "trace", opts.json_style)
        }
        OutputFormat::Human => {
            report::print_dependency_usage_trace_human(trace, opts.quiet);
            ExitCode::SUCCESS
        }
        other => unsupported_format(other),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn target() -> TraceDependencyTarget {
        TraceDependencyTarget {
            package_name: "react-redux".to_owned(),
            specifiers: Vec::new(),
            sites: false,
            limit: None,
            cursor: None,
            callers: false,
            depth: None,
        }
    }

    #[test]
    fn counts_only_without_site_flags() {
        let query = target().query().expect("valid query");
        assert!(query.sites.is_none());
        assert!(query.closure_depth.is_none());
    }

    #[test]
    fn each_site_flag_turns_on_the_site_page() {
        let with = |edit: fn(&mut TraceDependencyTarget)| {
            let mut target = target();
            edit(&mut target);
            target.query().expect("valid query").sites
        };
        assert!(with(|t| t.sites = true).is_some());
        assert!(with(|t| t.specifiers = vec!["useSelector".to_owned()]).is_some());
        assert!(with(|t| t.cursor = Some("v1.00".to_owned())).is_some());
        let page = with(|t| t.limit = Some(5)).expect("site page");
        assert_eq!(page.limit, 5);
        assert_eq!(
            with(|t| t.sites = true).map(|page| page.limit),
            Some(DEFAULT_USAGE_SITE_LIMIT)
        );
    }

    #[test]
    fn callers_use_the_default_depth_and_reject_a_depth_out_of_range() {
        let mut callers = target();
        callers.callers = true;
        assert_eq!(
            callers.query().expect("valid query").closure_depth,
            Some(fallow_types::trace_chain::DEFAULT_TRACE_DEPTH)
        );
        callers.depth = Some(11);
        assert!(callers.query().is_err());
        callers.depth = Some(0);
        assert!(callers.query().is_err());
    }
}
