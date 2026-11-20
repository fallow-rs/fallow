//! Stack-trace frame resolution contract for `fallow trace-error`.
//!
//! The verb answers one narrow question: for each frame of a runtime stack
//! trace, which definitions in the analysed project does that frame's
//! identifier name? It is deliberately easy to overclaim here, so the wire
//! shape is built so that it cannot:
//!
//! - a frame that matches several definitions reports `ambiguous` and lists
//!   every candidate, instead of picking one and calling it the answer;
//! - a frame that matches nothing reports `not_found` instead of being dropped;
//! - a frame that matches one definition but whose own line sits at a
//!   different declaration carries `line_mismatch`, because the look-up asks
//!   about the identifier and not about the line;
//! - a frame the project graph was never asked about (a dependency frame, a
//!   runtime-internal frame, a generated bundle, or a frame carrying no
//!   identifier to look up) reports `not_attempted` rather than borrowing
//!   `not_found`'s meaning;
//! - every frame read from the input appears in `frames`, in input order, and
//!   [`ErrorTraceCounts`](crate::trace_error::ErrorTraceCounts) publishes the
//!   per-outcome totals, so a caller can see exactly how much of its trace
//!   went unanswered.
//!
//! No source-map resolution is performed. A frame pointing into a build
//! artifact is reported as such, because a stale map rebinds silently to the
//! wrong line and a wrong line is worse than an honest refusal.

use serde::Serialize;

/// Wire-version discriminator for [`ErrorTrace`]. Independent from the global
/// `SchemaVersion` and from the other trace payloads, like
/// [`crate::trace::ImportPathTraceSchemaVersion`]. Serializes as a string
/// `const` so JSON consumers can switch on it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub enum ErrorTraceSchemaVersion {
    /// First release of the `fallow trace-error` shape.
    #[serde(rename = "1")]
    V1,
}

/// Where a frame's source location sits relative to the analysed project.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum FrameOrigin {
    /// The frame's file resolved to at least one module in the project graph.
    /// Only these frames are looked up against the graph's definitions.
    InProject,
    /// The frame's file lives under an installed dependency tree.
    NodeModules,
    /// Everything else: a runtime-internal frame, a generated bundle, a file
    /// outside the analysed corpus, or a frame carrying no source location.
    OutOfCorpus,
}

impl FrameOrigin {
    /// Stable kebab-case token for human output and diagnostics.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::InProject => "in-project",
            Self::NodeModules => "node-modules",
            Self::OutOfCorpus => "out-of-corpus",
        }
    }
}

/// What the project graph could say about a frame's identifier.
///
/// `not_attempted` is not a softer `not_found`: it records that the graph was
/// never consulted, because the frame does not point at project source. Keeping
/// them apart is what lets `resolved + ambiguous + not_found + not_attempted`
/// equal the frame count without any of the four lying about what it measured.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum FrameResolution {
    /// Exactly one definition matched. `candidates` holds that one entry.
    Resolved,
    /// More than one definition matched. Every match is listed and none is
    /// preferred; the caller decides, or narrows the question.
    Ambiguous,
    /// The graph was asked and knows no definition under this identifier. A
    /// module-local function is not in the graph's definition set, so this is
    /// also the answer for a frame naming one.
    NotFound,
    /// The graph was not asked. Either the frame does not point at project
    /// source, or it points at project source but carries nothing addressable
    /// to ask about: no printed function name, or a placeholder such as
    /// `Object.<anonymous>`. `reason` names which case applies, so this is
    /// never a silent shrug.
    NotAttempted,
}

impl FrameResolution {
    /// Stable kebab-case token for human output and diagnostics.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Resolved => "resolved",
            Self::Ambiguous => "ambiguous",
            Self::NotFound => "not-found",
            Self::NotAttempted => "not-attempted",
        }
    }
}

/// One definition a frame's identifier could name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct ErrorTraceCandidate {
    /// Root-relative file declaring the definition.
    pub file: String,
    /// The exported name. For a member match this is the owning export.
    pub symbol: String,
    /// The member name, when the frame's identifier named a member of
    /// `symbol` rather than `symbol` itself. Absent for a direct export match.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub member: Option<String>,
    /// What kind of definition this is: `export`, or the member kind
    /// (`class-method`, `class-property`, `enum-member`, `store-member`,
    /// `namespace-member`).
    pub kind: String,
    /// 1-based declaration line of the definition's identifier. Absent when the
    /// source file could not be read; never guessed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line: Option<u32>,
}

/// One frame read from the input stack trace.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct ErrorTraceFrame {
    /// 0-based position in the input trace, so a caller can quote a frame back
    /// even after filtering the array.
    pub index: usize,
    /// The input line this frame was read from, trimmed of surrounding
    /// whitespace and otherwise verbatim.
    pub raw: String,
    /// The frame's function identifier as written by the runtime, with the
    /// `async` and `new` markers stripped and recorded separately. Absent for a
    /// frame the runtime emitted without one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub function: Option<String>,
    /// Whether the runtime marked this frame as a constructor call (`new X`).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub is_constructor: bool,
    /// Whether the runtime marked this frame as an async call.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub is_async: bool,
    /// The frame's file as read from the trace, with any `file://` or
    /// `http(s)://` wrapper removed and separators forward-slashed. Reported as
    /// read: it is NOT rewritten to the module path it matched, so a caller can
    /// see what its runtime actually said. Absent for a frame with no location.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
    /// 1-based line from the frame's location, when the runtime supplied one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line: Option<u32>,
    /// 1-based column from the frame's location, when the runtime supplied one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub column: Option<u32>,
    /// Where the frame's file sits relative to the analysed project.
    pub origin: FrameOrigin,
    /// What the project graph could say about this frame's identifier.
    pub resolution: FrameResolution,
    /// Every definition the identifier could name, in deterministic order.
    /// Exactly one entry when `resolution` is `resolved`, more than one when it
    /// is `ambiguous`, and empty otherwise.
    pub candidates: Vec<ErrorTraceCandidate>,
    /// How many further candidates a presentation cap withheld.
    /// `candidates.len() + candidates_omitted` is the true match count, so an
    /// `ambiguous` frame never understates how ambiguous it is.
    pub candidates_omitted: usize,
    /// Set when this frame's own line disagrees with the definition its
    /// identifier matched: some other definition in the same file is declared
    /// closer above the line the runtime reported.
    ///
    /// The look-up matches on the identifier alone, so a `resolved` frame is
    /// resolved however far its line sits from the match. That is honest about
    /// the question asked and silent about a question a reader would ask next,
    /// which is why the disagreement is published instead of left to be
    /// noticed. The frame is NOT reclassified: the graph does know a
    /// definition under this identifier, and only the caller can say whether
    /// the runtime ran that one or a same-named definition elsewhere.
    /// `reason` names the declaration that sits closer. Only set on a
    /// `resolved` frame that carried a line and matched a definition whose own
    /// line could be read.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub line_mismatch: bool,
    /// Human-readable statement of what happened to this frame.
    pub reason: String,
}

/// Per-outcome totals for an [`ErrorTrace`].
///
/// `resolved + ambiguous + not_found + not_attempted == frames`, and
/// `in_project + node_modules + out_of_corpus == frames`. Both identities hold
/// on every run, so a caller can verify that nothing was dropped.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct ErrorTraceCounts {
    /// Frames reported in `frames`.
    pub frames: usize,
    /// Frames a cap withheld from `frames`. Their outcomes are NOT counted in
    /// the fields below, which describe the reported frames only.
    pub frames_omitted: usize,
    /// Frames whose file resolved to project source.
    pub in_project: usize,
    /// Frames whose file lives under an installed dependency tree.
    pub node_modules: usize,
    /// Frames outside the analysed corpus, including frames with no location.
    pub out_of_corpus: usize,
    /// Frames that matched exactly one definition.
    pub resolved: usize,
    /// Frames that matched more than one definition.
    pub ambiguous: usize,
    /// Frames the graph was asked about and could not name.
    pub not_found: usize,
    /// Frames the graph was never asked about.
    pub not_attempted: usize,
    /// Non-blank input lines that were neither recognised as a frame nor taken
    /// as `header`. A trace that is entirely unrecognised reports zero frames
    /// and a non-zero count here, rather than looking like an empty trace.
    pub unparsed_lines: usize,
}

/// Result of resolving a runtime stack trace against the project graph.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "schema", schemars(title = "fallow trace-error"))]
pub struct ErrorTrace {
    /// Wire-shape version of this payload.
    pub schema_version: ErrorTraceSchemaVersion,
    /// Where the trace was read from: `stdin`, or the path as the caller wrote
    /// it.
    pub source: String,
    /// The first non-blank input line that preceded any recognised frame,
    /// verbatim. Conventionally the error type and message, but it is reported
    /// as read and NOT parsed into parts. Absent when the input began with a
    /// frame or was empty.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub header: Option<String>,
    /// Every recognised frame, in input order. Nothing is filtered out: a
    /// dependency or runtime-internal frame stays in the array with its origin
    /// recorded, so hop numbering matches the trace the caller pasted.
    pub frames: Vec<ErrorTraceFrame>,
    /// Per-outcome totals.
    pub counts: ErrorTraceCounts,
    /// Human-readable summary of the outcome.
    pub reason: String,
}
