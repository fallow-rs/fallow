//! Symbol-level call-chain output contracts.

use std::path::PathBuf;

use serde::Serialize;

use crate::serde_path;

/// Default chain depth when `--depth` is unset.
pub const DEFAULT_TRACE_DEPTH: u32 = 2;

/// Which directions to walk.
#[derive(Debug, Clone, Copy)]
pub struct TraceDirections {
    /// Walk up to callers.
    pub callers: bool,
    /// Walk down to callees.
    pub callees: bool,
}

/// The result of a symbol-level call-chain trace. Its own surface (`kind:
/// "trace"`), NOT folded into the ranked brief.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct SymbolChainTrace {
    /// The file containing the traced symbol (project-root-relative).
    #[serde(serialize_with = "serde_path::serialize")]
    pub file: PathBuf,
    /// The traced symbol name.
    pub symbol: String,
    /// Whether the symbol's defining export was found in the graph. When
    /// `false`, the chains are empty and `reason` explains why.
    pub symbol_found: bool,
    /// The chain depth applied to both directions.
    pub depth: u32,
    /// Whether this trace is best-effort (always `true`: symbol-level chains are
    /// labeled best-effort, syntactic per ADR-001).
    pub best_effort: bool,
    /// Caller chain hops (UP). Present only when `--callers` was requested.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub callers: Option<Vec<ChainHop>>,
    /// Callee chain hops (DOWN) resolved to an import-symbol edge. Present only
    /// when `--callees` was requested.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub callees: Option<Vec<ChainHop>>,
    /// Callees referenced at a call site in the symbol's module that the
    /// syntactic walk could NOT resolve to an import-symbol edge (locals,
    /// globals, dynamic dispatch, re-bound callees). Reported, never dropped.
    /// Present only when `--callees` was requested.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unresolved_callees: Option<Vec<UnresolvedCallee>>,
    /// A human-readable summary of the trace outcome.
    pub reason: String,
}

/// One hop in a caller / callee chain.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct ChainHop {
    /// The file at this hop (project-root-relative). For a caller hop this is
    /// the importing module; for a callee hop the imported module.
    #[serde(serialize_with = "serde_path::serialize")]
    pub file: PathBuf,
    /// The symbol name as imported across the edge (`default`, `*` for namespace,
    /// the imported name otherwise).
    pub imported_as: String,
    /// The local binding name in the file at this hop.
    pub local_name: String,
    /// Whether the import edge is type-only (`import type { ... }`).
    pub type_only: bool,
    /// The hop's depth (1 = direct caller/callee of the symbol).
    pub depth: u32,
}

/// A callee referenced at a call site that did not resolve to an import-symbol
/// edge. Surfaced so a missing callee is never silently dropped.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct UnresolvedCallee {
    /// The callee path as written at the call site (e.g. `helper`,
    /// `obj.method`).
    pub callee: String,
    /// Why it is unresolved (best-effort classification).
    pub reason: UnresolvedReason,
}

/// Best-effort classification of why a callee did not resolve to an edge.
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum UnresolvedReason {
    /// A bare identifier call with no matching import binding (a same-module
    /// local function, a global, or a re-bound callee).
    LocalOrGlobal,
    /// A computed / member-expression callee (`obj.method`, dynamic dispatch).
    MemberOrDynamic,
}

/// Target and traversal parameters for a symbol-chain trace.
#[derive(Debug, Clone, Copy)]
pub struct SymbolChainQuery<'a> {
    /// File path of the target symbol, root-relative or absolute.
    pub file: &'a str,
    /// Exported symbol name to trace.
    pub symbol: &'a str,
    /// Maximum traversal depth in each direction.
    pub depth: u32,
    /// Which directions to walk.
    pub directions: TraceDirections,
}
