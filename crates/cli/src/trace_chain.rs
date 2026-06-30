//! `fallow trace <symbol> --callers --callees [--depth N]`: symbol-level
//! call chains.
//!
//! Best-effort, syntactic (ADR-001), EXPLICITLY OFF the ranked path. The result
//! is its OWN surface (`kind: "trace"`); it is NEVER folded into the ranked
//! brief and is NEVER an input to the focus map / ranking. Resolved-vs-
//! unresolved callees are reported honestly: an unresolved callee is surfaced
//! in `unresolved_callees`, never silently dropped.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use fallow_config::{OutputFormat, ProductionAnalysis};
use fallow_types::trace_chain::{SymbolChainQuery, SymbolChainTrace, TraceDirections};

use crate::error::emit_error;
use crate::report;
use crate::report::sink::outln;
use crate::{ConfigLoadOptions, load_config_for_analysis};

/// Options for `fallow trace`.
pub struct TraceChainOptions<'a> {
    pub root: &'a Path,
    pub config_path: &'a Option<PathBuf>,
    pub output: OutputFormat,
    pub no_cache: bool,
    pub threads: usize,
    pub quiet: bool,
    /// `FILE:SYMBOL` target.
    pub target: String,
    /// Walk UP to callers.
    pub callers: bool,
    /// Walk DOWN to callees.
    pub callees: bool,
    /// Chain depth bound (already resolved to a concrete value by the caller).
    pub depth: u32,
}

/// Run the symbol-level call-chain trace and emit its own output surface.
pub fn run_trace(opts: &TraceChainOptions<'_>) -> ExitCode {
    let Some((file, symbol)) = parse_target(&opts.target) else {
        return emit_error(
            "trace requires a FILE:SYMBOL target (e.g., src/utils.ts:formatDate)",
            2,
            opts.output,
        );
    };

    // Default to walking BOTH directions when neither flag is set, so a bare
    // `fallow trace <symbol>` is useful without remembering the flags.
    let directions = if !opts.callers && !opts.callees {
        TraceDirections {
            callers: true,
            callees: true,
        }
    } else {
        TraceDirections {
            callers: opts.callers,
            callees: opts.callees,
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
        },
        ProductionAnalysis::DeadCode,
    ) {
        Ok(config) => config,
        Err(code) => return code,
    };

    let trace = match fallow_engine::trace_symbol_chain(
        &config,
        SymbolChainQuery {
            file: &file,
            symbol: &symbol,
            depth: opts.depth,
            directions,
        },
    ) {
        Ok(Some(trace)) => trace,
        Ok(None) => {
            return emit_error(
                &format!("file '{file}' not found in module graph"),
                2,
                opts.output,
            );
        }
        Err(err) => return emit_error(&format!("Analysis error: {err}"), 2, opts.output),
    };

    emit_trace(trace, opts)
}

/// Split a `FILE:SYMBOL` target. The symbol is everything after the LAST `:` so
/// Windows drive-letter paths and nested colons survive.
fn parse_target(target: &str) -> Option<(String, String)> {
    let (file, symbol) = target.rsplit_once(':')?;
    if file.trim().is_empty() || symbol.trim().is_empty() {
        return None;
    }
    Some((file.to_string(), symbol.to_string()))
}

fn emit_trace(trace: SymbolChainTrace, opts: &TraceChainOptions<'_>) -> ExitCode {
    match opts.output {
        OutputFormat::Json => {
            let value = match fallow_output::serialize_trace_json_output(
                trace,
                crate::output_runtime::current_root_envelope_mode(),
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
            report::emit_json(&value, "trace")
        }
        OutputFormat::Human => {
            print_human(&trace, opts.quiet);
            ExitCode::SUCCESS
        }
        _ => emit_error("trace supports --format json or human", 2, opts.output),
    }
}

fn print_human(trace: &SymbolChainTrace, quiet: bool) {
    outln!("Symbol-level call chain (best-effort, syntactic; OFF the ranked path)");
    outln!();
    outln!("  symbol: {}:{}", trace.file.display(), trace.symbol);
    outln!("  found:  {}", trace.symbol_found);
    outln!("  depth:  {}", trace.depth);
    outln!();
    if let Some(callers) = trace.callers.as_ref() {
        outln!("Callers (up): {}", callers.len());
        for hop in callers {
            outln!(
                "  [{}] {} (imported as {} -> local {}){}",
                hop.depth,
                hop.file.display(),
                hop.imported_as,
                hop.local_name,
                if hop.type_only { " [type-only]" } else { "" }
            );
        }
        outln!();
    }
    if let Some(callees) = trace.callees.as_ref() {
        outln!("Resolved callees (down): {}", callees.len());
        for hop in callees {
            outln!(
                "  [{}] {} (imported as {} -> local {}){}",
                hop.depth,
                hop.file.display(),
                hop.imported_as,
                hop.local_name,
                if hop.type_only { " [type-only]" } else { "" }
            );
        }
        outln!();
    }
    if let Some(unresolved) = trace.unresolved_callees.as_ref() {
        outln!(
            "Unresolved callees (reported, not dropped): {}",
            unresolved.len()
        );
        for u in unresolved {
            outln!("  {} ({:?})", u.callee, u.reason);
        }
        outln!();
    }
    if !quiet {
        outln!("  {}", trace.reason);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_target_splits_on_last_colon() {
        assert_eq!(
            parse_target("src/utils.ts:formatDate"),
            Some(("src/utils.ts".to_string(), "formatDate".to_string()))
        );
        // Windows-style drive colon survives (symbol is after the LAST colon).
        assert_eq!(
            parse_target("C:/proj/src/a.ts:foo"),
            Some(("C:/proj/src/a.ts".to_string(), "foo".to_string()))
        );
    }

    #[test]
    fn parse_target_rejects_empty_halves() {
        assert!(parse_target("src/utils.ts:").is_none());
        assert!(parse_target(":foo").is_none());
        assert!(parse_target("no-colon").is_none());
    }
}
