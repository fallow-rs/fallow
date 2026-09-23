//! `fallow trace-error [FILE|-]`: resolve a runtime stack trace's frames
//! against the project graph.
//!
//! Its own surface (`kind: "trace-error"`, `schema_version: "1"`), like the
//! other trace shapes: never folded into the ranked brief and never an input to
//! the focus map. A trace nothing in it resolves is an ANSWER, not an error, so
//! it exits 0 and publishes the counts that say so; only unreadable input or a
//! failed analysis exits 2.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use fallow_config::{OutputFormat, ProductionAnalysis};
use fallow_engine::trace_error::MAX_STACK_TRACE_BYTES;
use fallow_types::trace_error::{ErrorTrace, FrameResolution};

use crate::error::emit_error;
use crate::report;
use crate::report::sink::outln;
use crate::{ConfigLoadOptions, load_config_for_analysis};

/// The stdin sentinel, matching `--diff-file -`.
const STDIN_SENTINEL: &str = "-";

/// Options for `fallow trace-error`.
pub struct TraceErrorOptions<'a> {
    pub root: &'a Path,
    pub config_path: &'a Option<PathBuf>,
    pub output: OutputFormat,
    pub json_style: crate::json_style::JsonStyle,
    pub no_cache: bool,
    pub threads: usize,
    pub quiet: bool,
    pub allow_remote_extends: bool,
    /// The trace file, or `-` / `None` to read stdin.
    pub trace_file: Option<&'a str>,
}

/// Read the stack trace, resolve its frames, and emit the result.
pub fn run_trace_error(opts: &TraceErrorOptions<'_>) -> ExitCode {
    let (input, source) = match read_trace(opts.root, opts.trace_file) {
        Ok(pair) => pair,
        Err((message, hint)) => {
            return crate::error::emit_error_with_hint(&message, hint, 2, opts.output);
        }
    };

    let config = match load_config_for_analysis(
        opts.root,
        opts.config_path,
        ConfigLoadOptions {
            output: opts.output,
            no_cache: opts.no_cache,
            threads: opts.threads,
            production_override: None,
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

    let trace = match fallow_engine::trace_error::trace_error_with_session(&session, &input, source)
    {
        Ok(trace) => trace,
        Err(err) => return emit_error(&format!("Analysis error: {err}"), 2, opts.output),
    };

    emit_trace_error(trace, opts)
}

/// Read the trace from a file or from stdin, returning the text and the label
/// the payload reports as its `source`.
///
/// A relative path is resolved against the project root, matching how
/// `--diff-file` resolves its input. The reported `source` keeps the caller's
/// own spelling rather than the resolved absolute path.
///
/// Every failure carries the remedy for it. `Err` is `(message, hint)`, which
/// the caller renders in the shared `Error: ... hint: ...` shape.
fn read_trace(
    root: &Path,
    trace_file: Option<&str>,
) -> Result<(String, String), (String, &'static str)> {
    let path = match trace_file {
        None | Some(STDIN_SENTINEL) => {
            let mut buffer = Vec::new();
            std::io::stdin()
                .take(MAX_STACK_TRACE_BYTES + 1)
                .read_to_end(&mut buffer)
                .map_err(|err| {
                    (
                        format!("failed to read stack trace from stdin: {err}"),
                        PIPE_HINT,
                    )
                })?;
            if buffer.len() as u64 > MAX_STACK_TRACE_BYTES {
                return Err((
                    format!(
                        "stack trace from stdin exceeds the {MAX_STACK_TRACE_BYTES}-byte limit"
                    ),
                    TRIM_HINT,
                ));
            }
            let text = String::from_utf8(buffer).map_err(|_| {
                (
                    "stack trace from stdin is not valid UTF-8".to_string(),
                    "pipe the trace as text; a captured binary log or a mixed encoding cannot be \
                     parsed",
                )
            })?;
            return Ok((text, "stdin".to_string()));
        }
        Some(path) => path,
    };

    let resolved = {
        let candidate = Path::new(path);
        if candidate.is_absolute() {
            candidate.to_path_buf()
        } else {
            root.join(candidate)
        }
    };
    let file = std::fs::File::open(&resolved).map_err(|err| {
        (
            format!("failed to read stack trace from '{path}': {err}"),
            PIPE_HINT,
        )
    })?;
    let mut buffer = Vec::new();
    file.take(MAX_STACK_TRACE_BYTES + 1)
        .read_to_end(&mut buffer)
        .map_err(|err| {
            (
                format!("failed to read stack trace from '{path}': {err}"),
                PIPE_HINT,
            )
        })?;
    if buffer.len() as u64 > MAX_STACK_TRACE_BYTES {
        return Err((
            format!("stack trace '{path}' exceeds the {MAX_STACK_TRACE_BYTES}-byte limit"),
            TRIM_HINT,
        ));
    }
    let text = String::from_utf8(buffer).map_err(|_| {
        (
            format!("stack trace from '{path}' is not valid UTF-8"),
            PIPE_HINT,
        )
    })?;
    Ok((text, path.to_string()))
}

fn emit_trace_error(trace: ErrorTrace, opts: &TraceErrorOptions<'_>) -> ExitCode {
    match opts.output {
        OutputFormat::Json => {
            let value = match fallow_output::serialize_trace_error_json_output(
                trace,
                crate::output_runtime::telemetry_analysis_run_id().as_deref(),
            ) {
                Ok(value) => value,
                Err(err) => {
                    return emit_error(
                        &format!("failed to serialize trace-error output: {err}"),
                        2,
                        opts.output,
                    );
                }
            };
            report::emit_report_json(&value, "trace-error", opts.json_style)
        }
        OutputFormat::Human => {
            print_human(&trace, opts.quiet);
            ExitCode::SUCCESS
        }
        _ => crate::error::emit_error_with_hint(
            "trace-error supports --format json or human",
            "re-run with `--format json` for a machine-readable answer, or drop `--format`",
            2,
            opts.output,
        ),
    }
}

/// The next step for an input that produced no frames, and the one thing the
/// help text does not make obvious at the point of failure.
const PIPE_HINT: &str =
    "pass a stack-trace file, or pipe one: `node app.js 2>&1 | fallow trace-error -`";

/// The remedy for an input over the size ceiling. The frames that matter sit at
/// the top of a stack, so trimming is a real fix rather than a workaround.
const TRIM_HINT: &str =
    "keep the top frames and drop the rest; the innermost frames are the ones this resolves";

fn print_human(trace: &ErrorTrace, quiet: bool) {
    outln!("Stack-trace frames (syntactic; OFF the ranked path)");
    outln!();
    outln!("  source: {}", trace.source);
    if let Some(header) = &trace.header {
        outln!("  error:  {header}");
    }
    outln!();
    if trace.frames.is_empty() {
        print_empty_human(trace);
        return;
    }
    // A reason explains a CLASS of frame, and a stack is usually one class
    // repeated. Printing it per frame turned a 60-frame node_modules stack into
    // 60 byte-identical lines between the reader and the counts. Each frame
    // still carries its own `[origin/resolution]` labels, so a suppressed
    // repeat loses nothing; a reason that CHANGES prints again.
    let mut last_reason: Option<&str> = None;
    for frame in &trace.frames {
        let location = match (&frame.file, frame.line) {
            (Some(file), Some(line)) => format!("{file}:{line}"),
            (Some(file), None) => file.clone(),
            (None, _) => "<no location>".to_string(),
        };
        outln!(
            "  [{}] {} {} [{}/{}]",
            frame.index,
            frame.function.as_deref().unwrap_or("<anonymous>"),
            location,
            frame.origin.label(),
            frame.resolution.label()
        );
        for candidate in &frame.candidates {
            // An ambiguous frame prints every candidate. Printing only the
            // first would restate the exact overclaim the payload refuses.
            let member = candidate
                .member
                .as_ref()
                .map_or_else(String::new, |member| format!(".{member}"));
            let line = candidate
                .line
                .map_or_else(String::new, |line| format!(":{line}"));
            outln!(
                "        -> {}{} {}{} ({})",
                candidate.file,
                line,
                candidate.symbol,
                member,
                candidate.kind
            );
        }
        if frame.candidates_omitted > 0 {
            outln!(
                "        -> {} further matches omitted",
                frame.candidates_omitted
            );
        }
        // A resolved frame normally needs no explanation, but one whose own
        // line disagrees with the definition it matched does: the note is the
        // only place that disagreement is written out.
        if (frame.resolution != FrameResolution::Resolved || frame.line_mismatch)
            && last_reason != Some(frame.reason.as_str())
        {
            outln!("        {}", frame.reason);
            last_reason = Some(&frame.reason);
        }
    }
    outln!();
    outln!("{}", counts_line(&trace.counts));
    // Prose, like the other trace surfaces: `--quiet` drops the explanation from
    // human output while the JSON payload keeps it either way.
    if !quiet {
        outln!();
        outln!("{}", trace.reason);
    }
}

/// The empty state: one statement of the fact, then what to do about it.
///
/// It used to state the same thing three times (a literal `No stack frames
/// recognised.`, a counts line that was all zeroes, and the prose `reason`) and
/// then stop, with no next step and nothing saying the input can be piped. The
/// unparsed-line count is folded into the sentence rather than left on a counts
/// line so it survives `--quiet`, which drops prose.
fn print_empty_human(trace: &ErrorTrace) {
    let unparsed = trace.counts.unparsed_lines;
    if unparsed > 0 {
        outln!(
            "No stack frames recognised ({unparsed} input line{} did not parse as a frame).",
            if unparsed == 1 { "" } else { "s" }
        );
    } else {
        outln!("No stack frames recognised.");
    }
    outln!("  hint: {PIPE_HINT}");
}

/// The always-printed counts line.
///
/// `frames_omitted` and `unparsed_lines` are MEASUREMENTS, not progress, so
/// they belong here rather than in the prose `--quiet` drops. Without them a
/// capped trace looks complete and an input in which nothing was recognised
/// looks like an empty trace, which is exactly the overclaim this payload is
/// built to refuse. They print only when non-zero: zero omissions is the
/// ordinary case and a permanent column of zeroes would bury the run where the
/// number is not zero.
fn counts_line(counts: &fallow_types::trace_error::ErrorTraceCounts) -> String {
    use std::fmt::Write as _;

    let mut line = format!(
        "  frames {} | resolved {} | ambiguous {} | not found {} | not attempted {}",
        counts.frames, counts.resolved, counts.ambiguous, counts.not_found, counts.not_attempted
    );
    if counts.frames_omitted > 0 {
        let _ = write!(line, " | frames omitted {}", counts.frames_omitted);
    }
    if counts.unparsed_lines > 0 {
        let _ = write!(line, " | unparsed lines {}", counts.unparsed_lines);
    }
    line
}
