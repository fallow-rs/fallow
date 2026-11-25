//! Security control sites, sanitizers, tainted bindings, skipped callees, and
//! sink sites.

use super::*;

/// Family of a control pattern observed in a file on an import trace.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    serde::Serialize,
    serde::Deserialize,
    bitcode::Encode,
    bitcode::Decode,
)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum SecurityControlKind {
    /// Sanitization or escaping before a sink.
    Sanitization,
    /// Input validation or schema parsing.
    Validation,
    /// Authentication check or middleware.
    Authentication,
    /// Authorization or permission check.
    Authorization,
}

/// An observed control call or guard pattern, without proof that it protects a sink.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, bitcode::Encode, bitcode::Decode)]
pub struct SecurityControlSite {
    /// Control family.
    pub kind: SecurityControlKind,
    /// Flattened callee path or a stable synthetic name for guard-derived
    /// controls.
    pub callee_path: String,
    /// Byte offset of the control span start.
    pub span_start: u32,
    /// Byte offset of the control span end.
    pub span_end: u32,
}

/// Sanitizer output domain. Kept intentionally narrow so a sanitizer for one
/// domain cannot suppress a different sink family.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    serde::Serialize,
    serde::Deserialize,
    bitcode::Encode,
    bitcode::Decode,
)]
pub enum SanitizerScope {
    /// HTML markup sanitized by DOMPurify-compatible APIs.
    Html,
    /// URL or redirect target checked against a literal-backed allowlist.
    Url,
    /// Path value checked against a high-confidence containment guard.
    Path,
    /// SQL identifier quoted with a helper that doubles embedded identifier quotes.
    SqlIdentifier,
}

/// A captured sink argument that is itself a recognized sanitizer call.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, bitcode::Encode, bitcode::Decode)]
pub struct SanitizedSinkArg {
    /// Byte offset of the owning sink span start.
    pub span_start: u32,
    /// The positional argument index on the owning sink.
    pub arg_index: u32,
    /// The sanitizer output domain for this argument.
    pub scope: SanitizerScope,
}

/// A local binding tied to the flattened member-access path it was initialized
/// from. The analyze layer matches `source_path` against the data-driven source
/// catalogue; when it matches, `local` is treated as carrying untrusted input.
///
/// Captured for two shapes: a direct assignment (`const id = req.query.id` ->
/// `{ local: "id", source_path: "req.query" }`, the literal-key tail dropped so
/// the path matches a catalogue prefix) and an object destructure
/// (`const { id } = req.query` -> `{ local: "id", source_path: "req.query" }`).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, bitcode::Encode, bitcode::Decode)]
pub struct TaintedBinding {
    /// The local binding name introduced by the declarator.
    pub local: String,
    /// The flattened object member-access path the binding was sourced from.
    pub source_path: String,
    /// Byte offset of the source read (the member-access expression the binding
    /// was sourced from), so the analyze layer can anchor a taint trace's source
    /// node at the real read line instead of the module import line. Stored as a
    /// `u32` (not `Span`) to stay bitcode-encodable for the cache. `0` when no
    /// concrete read expression is available (synthetic framework-param /
    /// helper-return bindings), in which case the analyze layer falls back to the
    /// sink site rather than claiming a spurious line.
    pub source_span_start: u32,
}

/// Why a sink-shaped callee could not be flattened into a static catalogue
/// path.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    serde::Serialize,
    serde::Deserialize,
    bitcode::Encode,
    bitcode::Decode,
)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum SkippedSecurityCalleeReason {
    /// A computed member access such as `client[method](input)`.
    ComputedMember,
    /// A dynamic non-member callee such as `(factory())(input)`.
    DynamicDispatch,
    /// An assignment target whose object could not be flattened.
    UnsupportedAssignmentObject,
}

/// Syntactic expression shape for a skipped security callee.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    serde::Serialize,
    serde::Deserialize,
    bitcode::Encode,
    bitcode::Decode,
)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum SkippedSecurityCalleeExpressionKind {
    /// `obj.prop(...)`.
    StaticMemberExpression,
    /// `obj[prop](...)`.
    ComputedMemberExpression,
    /// A bare identifier or private identifier callee.
    Identifier,
    /// Any other call-like expression that cannot be represented compactly.
    Other,
}

/// Span-only diagnostic for a skipped security callee inside one module.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, bitcode::Encode, bitcode::Decode)]
pub struct SkippedSecurityCalleeSite {
    /// Why the callee was skipped.
    pub reason: SkippedSecurityCalleeReason,
    /// Compact expression shape of the skipped callee.
    pub expression_kind: SkippedSecurityCalleeExpressionKind,
    /// Start byte offset of the skipped callee expression.
    pub span_start: u32,
    /// End byte offset of the skipped callee expression.
    pub span_end: u32,
}

/// The syntactic shape of a captured security sink site. Category-blind: the
/// extractor records the shape and the dotted/bare callee path; the analyze
/// layer matches it against the data-driven catalogue. See
/// `crates/security/data/security_matchers.toml`.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    serde::Serialize,
    serde::Deserialize,
    bitcode::Encode,
    bitcode::Decode,
)]
pub enum SinkShape {
    /// A call to a bare identifier (e.g. `eval(x)`).
    Call,
    /// A call to a dotted member path (e.g. `child_process.exec(x)`).
    MemberCall,
    /// An assignment to a member target (e.g. `el.innerHTML = x`).
    MemberAssign,
    /// A tagged template expression (e.g. ``sql`...${x}...` ``).
    TaggedTemplate,
    /// A JSX attribute value (e.g. `dangerouslySetInnerHTML={x}`).
    JsxAttr,
    /// A constructor call (e.g. `new Function("return x")`).
    NewExpression,
    /// A static string literal assigned to a secret-shaped identifier or known
    /// provider credential prefix.
    SecretLiteral,
}

/// The shape of the argument captured at a sink site. Category-blind like
/// [`SinkShape`], but finer-grained: it lets the catalogue matcher require or
/// exclude specific argument shapes. The discriminator is what distinguishes an
/// unsafe SQL string concatenation or template-into-`.execute()` from a
/// safely-parameterized `` sql`${x}` `` tagged template, an object-literal
/// `.execute({ sql, args })` argument, or a literal-aware sink argument.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    serde::Serialize,
    serde::Deserialize,
    bitcode::Encode,
    bitcode::Decode,
)]
pub enum SinkArgKind {
    /// A template literal with at least one `${...}` substitution (e.g.
    /// `` `SELECT ${x}` ``). On a `tagged-template` shape this is the tag's
    /// quasi; on a `call`/`member-call` shape it is the positional argument.
    TemplateWithSubst,
    /// A binary `+` string concatenation (e.g. `"SELECT " + x`).
    Concat,
    /// An object literal (e.g. `.execute({ sql, args })`, the parameterized form).
    Object,
    /// A call expression argument (e.g. `query(buildSql())`).
    Call,
    /// A literal argument admitted by a literal-aware security matcher.
    Literal,
    /// A zero-argument sink captured because the callee itself is the signal.
    NoArg,
    /// Any other non-literal expression (bare identifier, member access, etc.).
    Other,
}

/// A captured sink site. The visitor records every existing non-literal call /
/// member-assign / member-call / tagged-template / jsx-attr sink site, and a
/// small allowlist of literal-aware sites where the literal value is the signal.
/// It knows nothing about CWE categories.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, bitcode::Encode, bitcode::Decode)]
pub struct SinkSite {
    /// The syntactic shape of the sink site.
    pub sink_shape: SinkShape,
    /// The flattened dotted/bare callee or member path.
    pub callee_path: String,
    /// The positional argument index. For zero-argument captures this is 0.
    pub arg_index: u32,
    /// Whether the relevant argument is non-literal. Existing non-literal
    /// catalogue rows require this to remain true.
    pub arg_is_non_literal: bool,
    /// The finer-grained shape of the captured argument. Lets the catalogue
    /// require unsafe shapes (concat / template-with-substitution / literal /
    /// no-arg) and exclude safe ones (object literal, the parameterized form).
    /// See [`SinkArgKind`].
    pub arg_kind: SinkArgKind,
    /// Literal argument value for literal-aware rows.
    pub arg_literal: Option<SinkLiteralValue>,
    /// Risky regex fragment for structural ReDoS candidates.
    pub regex_pattern: Option<String>,
    /// Static object-literal properties for option-object rows.
    pub object_properties: Vec<SinkObjectProperty>,
    /// Static top-level object-literal keys, including keys whose values are not
    /// literal. Used by missing-option rows that only need key presence.
    pub object_property_keys: Vec<String>,
    /// Whether [`object_property_keys`](Self::object_property_keys) is complete.
    /// False for non-object arguments and object literals with spread or
    /// non-static keys, where a missing-key claim would be speculative.
    pub object_property_keys_complete: bool,
    /// Identifier names referenced anywhere inside the captured non-literal sink
    /// argument, or contextual names for zero-argument captures such as a
    /// token-like `Math.random()` assignment target. Deduped in source order.
    /// Used by the analyze layer to back-trace the sink argument to a known
    /// untrusted source or to apply narrow context gates. Intra-module,
    /// name-based, conservative; it is never a taint proof.
    pub arg_idents: Vec<String>,
    /// Flattened static member paths referenced inside the captured non-literal
    /// sink argument. Includes both the full path and source-object path for
    /// leaf reads (`process.env.SECRET` records `process.env.SECRET` and
    /// `process.env`) so direct source expressions can be matched without an
    /// intermediate local binding.
    pub arg_source_paths: Vec<String>,
    /// Byte offset of the sink span start. Stored as `u32` (not `Span`) so the
    /// struct is bitcode-encodable and can be persisted directly in the cache.
    pub span_start: u32,
    /// Byte offset of the sink span end.
    pub span_end: u32,
    /// The arg-0 URL string literal of a network-shaped call (`fetch`, `axios.*`,
    /// `got`, ...), captured so the `secret-to-network` category (#890) can carry
    /// a destination-host signal on its candidate: `Some(literal)` when the
    /// destination is a static string literal (almost always intended auth, e.g.
    /// the credential's own provider), `None` when it is dynamic (the suspicious
    /// case). `None` for non-call sinks and calls with no arg 0.
    pub url_arg_literal: Option<String>,
    /// URL construction shape for URL-like sink arguments when the extractor can
    /// classify it syntactically. `None` for non-URL sinks and URL expressions
    /// whose shape is not visible at the sink.
    pub url_shape: Option<SecurityUrlShape>,
    /// Options-object metadata of the call, on the command argument and the
    /// argument array of a call whose options position the extractor knows. `None` for every other
    /// sink, which means that the options are unknown.
    pub call_options: Option<Box<SinkCallOptions>>,
}

impl SinkSite {
    /// Reconstruct the source span from the stored byte offsets.
    #[must_use]
    pub fn span(&self) -> Span {
        Span::new(self.span_start, self.span_end)
    }
}
