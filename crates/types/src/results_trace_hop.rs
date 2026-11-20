//! The hops of the structural trace of a security finding.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::serde_path;

/// The role a hop plays in a security finding's structural import trace.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum TraceHopRole {
    /// The `"use client"` boundary file the finding is anchored on.
    ClientBoundary,
    /// A module that reads an untrusted input source such as request data,
    /// where the candidate's sink argument actually traces back to that read in
    /// the same statement (arg-level, the strong intra-module association).
    UntrustedSource,
    /// A module that merely contains an untrusted-input source somewhere and is
    /// import-reachable to the sink module (module-level, issue #885). This is a
    /// reachability signal, NOT a proven value path: the specific source value
    /// is not shown to reach the sink argument. Labeled distinctly from
    /// `UntrustedSource` so a consumer never reads a module-level hop as a
    /// value-flow proof.
    ModuleSource,
    /// An intermediate module on the transitive import path.
    Intermediate,
    /// The module that reads the secret.
    SecretSource,
    /// The syntactic sink site of a catalogue-driven `tainted-sink` candidate
    /// (the single hop the `tainted_sink` detector emits). Distinct from
    /// `SecretSource`, which is specific to the `client-server-leak` rule.
    Sink,
}

/// One hop in a security finding's structural trace. Stored as an absolute path
/// internally; JSON serialization strips the project root via
/// `serde_path::serialize`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct TraceHop {
    /// File on this hop of the import chain.
    #[serde(serialize_with = "serde_path::serialize")]
    pub path: PathBuf,
    /// 1-based line number. Import-chain hops point at the import site. A
    /// `secret-source` hop points at the first secret read of the module, and
    /// a `client-server-leak` sink hop at its first server-only import.
    pub line: u32,
    /// 0-based byte column offset.
    pub col: u32,
    /// Role of this hop in the chain.
    pub role: TraceHopRole,
    /// Every non-public env read that makes this module a secret source, in
    /// source order. Present only on a `secret-source` hop; the hop `line` and
    /// `col` equal the first entry.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub secret_reads: Vec<TraceSecretRead>,
}

/// One non-public env read on a `secret-source` trace hop.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct TraceSecretRead {
    /// The read path, for example `process.env.SESSION_SECRET`.
    pub name: String,
    /// 1-based line number of the read.
    pub line: u32,
    /// 0-based byte column offset of the read.
    pub col: u32,
}

impl TraceHop {
    /// A hop with no secret reads.
    #[must_use]
    pub const fn new(path: PathBuf, line: u32, col: u32, role: TraceHopRole) -> Self {
        Self {
            path,
            line,
            col,
            role,
            secret_reads: Vec::new(),
        }
    }
}
