//! `fallow trace --path <FROM> <TO>`: the shortest import path between two
//! modules.
//!
//! Its own surface (`kind: "trace"`, `schema_version: "1"`), like the other
//! trace shapes: never folded into the ranked brief and never an input to the
//! focus map. An unreachable pair is an ANSWER, not an error, so it exits 0
//! with `reachable: false`; an endpoint that names no module or an ambiguous
//! abbreviation exits 2.

use std::process::ExitCode;

use fallow_config::OutputFormat;
use fallow_engine::trace::{ImportPathEndpoint, ImportPathTrace};

use crate::error::emit_input_error;
use crate::exit_codes::INVALID_INPUT;
use crate::report;
use crate::report::sink::outln;

/// Options for `fallow trace --path`.
pub struct TracePathOptions<'a> {
    pub output: OutputFormat,
    pub json_style: crate::json_style::JsonStyle,
    pub quiet: bool,
    /// Resolves the run of the command line. The command calls it after its
    /// own checks of the command line.
    pub resolve: crate::cli_production::ResolveRun<'a>,
    /// The module the walk starts from.
    pub from: &'a str,
    /// The module the walk is looking for.
    pub to: &'a str,
    /// Follow only static imports that carry a runtime value.
    pub eager_only: bool,
}

/// Resolve the shortest import path and emit it on the trace surface.
pub fn run_trace_path(opts: &TracePathOptions<'_>) -> ExitCode {
    let run = match (opts.resolve)() {
        Ok(run) => run,
        Err(code) => return code,
    };
    let session = match crate::cli_production::dead_code_runnable(&run, opts.output) {
        Ok(runnable) => runnable.open_session(),
        Err(code) => return code,
    };

    let trace = match fallow_engine::trace::trace_import_path_with_session(
        &session,
        opts.from,
        opts.to,
        opts.eager_only,
    ) {
        Ok(Ok(trace)) => trace,
        Ok(Err(endpoint)) => {
            let (label, value) = match endpoint {
                ImportPathEndpoint::From | ImportPathEndpoint::AmbiguousFrom => {
                    (endpoint.label(), opts.from)
                }
                ImportPathEndpoint::To | ImportPathEndpoint::AmbiguousTo => {
                    (endpoint.label(), opts.to)
                }
            };
            let problem = if endpoint.is_ambiguous() {
                "matches multiple modules; use the full project-relative path"
            } else {
                "not found in module graph"
            };
            return crate::error::emit_error_with_hint(
                &format!("--path {label} module '{value}' {problem}"),
                "pass a path relative to the project root; `fallow list --files` prints every \
                     file the run discovered",
                INVALID_INPUT,
                opts.output,
            );
        }
        Err(err) => return emit_input_error(&format!("Analysis error: {err}"), opts.output),
    };

    emit_trace_path(trace, opts)
}

fn emit_trace_path(trace: ImportPathTrace, opts: &TracePathOptions<'_>) -> ExitCode {
    match opts.output {
        OutputFormat::Json => {
            let value = match fallow_output::serialize_trace_json_output(
                trace,
                crate::output_runtime::telemetry_analysis_run_id().as_deref(),
            ) {
                Ok(value) => value,
                Err(err) => {
                    return emit_input_error(
                        &format!("failed to serialize trace output: {err}"),
                        opts.output,
                    );
                }
            };
            report::emit_report_json(&value, "trace", opts.json_style)
        }
        OutputFormat::Human => {
            print_human(&trace, opts.quiet, opts.eager_only);
            ExitCode::SUCCESS
        }
        _ => crate::error::emit_error_with_hint(
            "trace --path supports --format json or human",
            "re-run with `--format json` for a machine-readable answer, or drop `--format`",
            INVALID_INPUT,
            opts.output,
        ),
    }
}

fn print_human(trace: &ImportPathTrace, quiet: bool, eager_only: bool) {
    if eager_only {
        outln!("Shortest eager import path (static value imports only; syntactic)");
    } else {
        outln!("Shortest import path (syntactic; OFF the ranked path)");
    }
    outln!();
    outln!("  from:      {}", trace.from);
    outln!("  to:        {}", trace.to);
    // `hops: 0` means "same module" AND "no route exists", so the hop count
    // cannot carry the answer this command exists to give. The payload has
    // always separated the two on `reachable`; the header printed only the
    // ambiguous half.
    outln!(
        "  reachable: {}",
        if trace.reachable { "yes" } else { "no" }
    );
    outln!("  hops:      {}", trace.hops);
    outln!();
    if trace.path.is_empty() {
        // The reason of an empty path only repeats this line and the header,
        // so the human output prints the answer once.
        outln!(
            "{}",
            if trace.reachable {
                "Same module: no import hop needed."
            } else {
                "No import path."
            }
        );
        return;
    }
    for (index, hop) in trace.path.iter().enumerate() {
        // The line belongs to the IMPORTING file: it anchors the binding
        // that creates the edge, not anything in the imported module.
        let line = hop
            .import_line
            .map_or_else(String::new, |line| format!(":{line}"));
        outln!(
            "  [{}] {}{} -> {}{}",
            index + 1,
            hop.from,
            line,
            hop.to,
            if hop.type_only {
                " [type-only]"
            } else if hop.dynamic {
                " [dynamic]"
            } else {
                ""
            }
        );
    }
    // Prose, like the other trace surfaces: `--quiet` drops the explanation from
    // human output while the JSON payload keeps it either way.
    if !quiet {
        outln!();
        outln!("{}", trace.reason);
    }
}
