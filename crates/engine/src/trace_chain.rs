//! Symbol trace types exposed through the engine boundary.

use fallow_config::ResolvedConfig;

use crate::{EngineError, EngineResult, core_backend, session::AnalysisSession};

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
    let session = AnalysisSession::from_resolved_config(config.clone());
    trace_symbol_chain_with_session(&session, query)
}

/// Run symbol-level call-chain tracing through an existing analysis session.
///
/// # Errors
///
/// Returns an error if parsing, graph construction, or retained module
/// analysis fails.
pub fn trace_symbol_chain_with_session(
    session: &AnalysisSession,
    query: SymbolChainQuery<'_>,
) -> EngineResult<Option<SymbolChainTrace>> {
    let output = session.analyze_dead_code_with_artifacts(true, true)?;
    let graph = output
        .graph
        .as_ref()
        .ok_or_else(|| EngineError::new("trace requires a retained module graph"))?;
    let modules = output.modules.as_deref().unwrap_or(&[]);
    Ok(core_backend::trace_symbol_chain(
        graph.as_graph(),
        modules,
        session.root(),
        query,
    ))
}
