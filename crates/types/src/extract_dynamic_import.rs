//! Dynamic `import()` call facts.

use oxc_span::Span;

/// A dynamic `import()` call.
#[derive(Debug, Clone)]
pub struct DynamicImportInfo {
    /// The import specifier.
    pub source: String,
    /// Source span of the `import()` expression.
    pub span: Span,
    /// Names destructured from the dynamic import result.
    /// Non-empty means `const { a, b } = await import(...)` -> Named imports.
    /// Empty means simple `import(...)` or `const x = await import(...)` -> Namespace.
    pub destructured_names: Vec<String>,
    /// The local variable name for `const x = await import(...)`.
    /// Used for namespace import narrowing via member access tracking.
    pub local_name: Option<String>,
    /// True when this dynamic import was synthesised by fallow rather than appearing in user source.
    pub is_speculative: bool,
    /// True when the value of an `import()` without a binding escapes, for
    /// example as a return value, a call argument, or an array element.
    /// The consumer can then read any export, so the edge credits the whole
    /// module namespace. False for a discarded `import()` statement, which
    /// only loads the module.
    pub value_escapes: bool,
}
