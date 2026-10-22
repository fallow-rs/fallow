//! Shared transient extraction types for similar-code candidate generation.
//!
//! These types carry source fragments only between extraction and a provider
//! orchestration layer. They are intentionally not part of `ModuleInfo` or the
//! persisted parse cache.

/// Extraction semantics understood by the first similar-code source contract.
///
/// Increment this value whenever supported function forms, naming, spans, or
/// source payload selection changes.
pub const SIMILAR_CODE_EXTRACTION_SEMANTICS_VERSION: u32 = 1;

/// Full SHA-256 digest of one exact extracted function source fragment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SimilarCodeSourceDigest([u8; 32]);

impl SimilarCodeSourceDigest {
    /// Construct a digest from the complete SHA-256 output bytes.
    #[must_use]
    pub const fn new(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// Return the complete SHA-256 output bytes.
    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

/// Supported source form for an extracted similar-code function.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum SimilarCodeFunctionKind {
    /// A top-level named `function` declaration.
    FunctionDeclaration,
    /// A top-level identifier binding or default export using a function expression.
    FunctionExpression,
    /// A top-level identifier binding or default export using an arrow function.
    ArrowFunction,
    /// A statically named ordinary method on a top-level named class.
    ClassMethod,
    /// A statically named ordinary method on a top-level bound object literal.
    ObjectMethod,
}

/// Conservative syntactic hint about possible function side effects.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum SimilarCodeSideEffectHint {
    /// No explicit effectful construct was present in a reliably parsed body.
    PureLooking,
    /// The body contains a call, write, throw, await, member access, or similar construct.
    MayHaveSideEffects,
    /// Parser recovery made the syntactic classification unreliable.
    Unknown,
}

/// Complete location of one function in UTF-8 source text.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct SimilarCodeFunctionLocation {
    /// UTF-8 project-root-relative path with forward-slash separators.
    pub file: String,
    /// Inclusive zero-based byte offset in the post-BOM source view.
    pub start_byte: u32,
    /// Exclusive zero-based byte offset in the post-BOM source view.
    pub end_byte: u32,
    /// One-based start line.
    pub start_line: u32,
    /// Zero-based start column counted in UTF-8 scalar values, not bytes.
    pub start_column_utf8: u32,
    /// One-based end line.
    pub end_line: u32,
    /// Zero-based end column counted in UTF-8 scalar values, not bytes.
    pub end_column_utf8: u32,
}

/// One named top-level JS or TS function prepared for provider inference.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtractedSimilarCodeFunction {
    /// Stable source-level name selected by the extraction semantics.
    pub name: String,
    /// Supported syntactic function form.
    pub kind: SimilarCodeFunctionKind,
    /// Full UTF-8 source location.
    pub location: SimilarCodeFunctionLocation,
    /// Full SHA-256 digest of `source`.
    pub source_sha256: SimilarCodeSourceDigest,
    /// Exact bounded source fragment, transient and never parse-cache data.
    pub source: String,
    /// Number of declared parameters, excluding TypeScript's `this` parameter.
    pub param_count: u32,
    /// Whether the function has an `async` modifier.
    pub is_async: bool,
    /// Whether the function is a generator.
    pub is_generator: bool,
    /// Whether the direct function body contains an `await` expression.
    pub has_await: bool,
    /// Whether the direct function body contains a `throw` statement.
    pub has_throw: bool,
    /// Conservative closed syntactic side-effect classification.
    pub side_effect_hint: SimilarCodeSideEffectHint,
}

/// Hard source-payload limits for one on-demand extraction call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SimilarCodeExtractionLimits {
    /// Maximum retained functions.
    pub max_functions: usize,
    /// Maximum bytes retained for any one function source fragment.
    pub max_source_bytes_per_function: usize,
    /// Maximum combined source bytes retained by the result.
    pub max_total_source_bytes: usize,
}

impl Default for SimilarCodeExtractionLimits {
    fn default() -> Self {
        const MAX_FUNCTIONS: usize = 10_000;
        const MAX_SOURCE_BYTES_PER_FUNCTION: usize = 64 * 1024;
        const MAX_TOTAL_SOURCE_BYTES: usize = 16 * 1024 * 1024;

        Self {
            max_functions: MAX_FUNCTIONS,
            max_source_bytes_per_function: MAX_SOURCE_BYTES_PER_FUNCTION,
            max_total_source_bytes: MAX_TOTAL_SOURCE_BYTES,
        }
    }
}

/// Stable reason source work was omitted or recovered during extraction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum SimilarCodeExtractionSkipReason {
    /// The input path is not valid UTF-8.
    NonUtf8Path,
    /// The input is not a supported standalone JS or TS source file.
    UnsupportedFileType,
    /// TypeScript declaration files contain no runtime function bodies.
    DeclarationFile,
    /// A closed path or header rule classified the source as generated.
    GeneratedSource,
    /// Oxc recovered from one or more syntax diagnostics.
    SyntaxDiagnostic,
    /// The source nests brackets too deep to parse without a stack overflow.
    NestingLimit,
    /// A declaration or overload had no function body.
    DeclarationWithoutBody,
    /// Nested functions and callbacks are outside the first extraction semantics.
    NestedFunction,
    /// A method, computed binding, or other unsupported function form was omitted.
    UnsupportedFunctionForm,
    /// The AST span could not be sliced safely from the source.
    InvalidSourceSpan,
    /// A function source fragment exceeded its per-function byte limit.
    SourceBytesPerFunctionLimit,
    /// The retained function limit was reached.
    FunctionLimit,
    /// The combined source-payload byte limit was reached.
    TotalSourceBytesLimit,
}

/// Counted omission or recovery evidence for one stable extraction reason.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SimilarCodeExtractionSkip {
    /// Stable reason.
    pub reason: SimilarCodeExtractionSkipReason,
    /// Number of functions or diagnostics represented by the reason.
    pub count: usize,
}

/// Transient result of one bounded source-to-functions extraction call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SimilarCodeExtraction {
    /// Semantics version used to select and name functions.
    pub extraction_semantics_version: u32,
    /// Retained functions in deterministic source order.
    pub functions: Vec<ExtractedSimilarCodeFunction>,
    /// Combined byte size of retained `source` payloads.
    pub source_bytes: usize,
    /// Counted skip and recovery evidence in stable reason order.
    pub skipped: Vec<SimilarCodeExtractionSkip>,
}
