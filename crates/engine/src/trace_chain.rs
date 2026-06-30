//! Symbol trace types exposed through the engine boundary.

use fallow_config::ResolvedConfig;

use crate::{
    EngineError, EngineResult, core_backend, session::analyze_dead_code_with_artifacts_from_config,
};

use fallow_types::trace_chain::{SymbolChainQuery, SymbolChainTrace};

/// Run symbol-level call-chain tracing through the engine boundary.
///
/// # Errors
///
/// Returns an error if parsing, graph construction, or retained module
/// analysis fails.
pub fn trace_symbol_chain(
    config: &ResolvedConfig,
    query: SymbolChainQuery<'_>,
) -> EngineResult<Option<SymbolChainTrace>> {
    let output = analyze_dead_code_with_artifacts_from_config(config, true, true)?;
    let graph = output
        .graph
        .as_ref()
        .ok_or_else(|| EngineError::new("trace requires a retained module graph"))?;
    let modules = output.modules.as_deref().unwrap_or(&[]);
    Ok(core_backend::trace_symbol_chain(
        graph.as_graph(),
        modules,
        &config.root,
        query,
    ))
}
