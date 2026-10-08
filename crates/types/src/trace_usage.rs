//! Per-specifier usage of a dependency: the `usage` object of
//! `fallow trace --dependency <package>`.
//!
//! The object counts how the code uses each imported name of a package. It
//! follows one hop through a project wrapper, for example
//! `export const useAppSelector = useSelector.withTypes<RootState>()`, and it
//! counts each use that it cannot resolve. The answer is syntactic: it reads
//! import bindings and call sites, not types.

use serde::Serialize;

/// Wire-shape version of the [`DependencyUsage`] object.
///
/// The object has its own version, because it is an optional part of the
/// `DependencyTrace` envelope.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub enum DependencyUsageSchemaVersion {
    /// First release of the dependency usage shape.
    #[serde(rename = "1")]
    V1,
}

/// How the usage was found.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum UsageConfidence {
    /// Read from import bindings and call sites, without type information.
    Syntactic,
}

/// How the code uses each imported name of a dependency.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct DependencyUsage {
    /// Wire-shape version of this object.
    pub schema_version: DependencyUsageSchemaVersion,
    /// How the usage was found.
    pub confidence: UsageConfidence,
    /// One entry per imported name, sorted by `name` in byte order. With a
    /// specifier filter, only the selected names, each present even when no
    /// file imports it.
    pub specifiers: Vec<SpecifierUsage>,
    /// Uses that hide which names a file reads, counted per file-level form.
    /// Present with a specifier filter too, because a `require` or a dynamic
    /// import can hide any name.
    pub unresolved: FileLevelUnresolved,
    /// One page of usage sites. Absent unless sites were requested.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sites: Option<UsageSitePage>,
    /// The files that import the users of the dependency. Absent unless a
    /// closure depth was requested.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub closure: Option<ConsumerClosure>,
}

/// The usage of one imported name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct SpecifierUsage {
    /// The imported name: `default` for a default import, the first member for
    /// a namespace member, and `*` for a bare namespace use.
    pub name: String,
    /// Files with a static import binding of the name, a namespace member use
    /// of it, or a re-export of it from the package.
    pub file_count: usize,
    /// The files in `file_count` where every binding of the name is type-only:
    /// an `import type`, or a value import that the file reads only in type
    /// positions.
    pub type_only_file_count: usize,
    /// Direct calls of the name, not through a project wrapper.
    pub call_site_count: usize,
    /// Uses of the name that the trace cannot resolve to a call.
    pub unresolved: SpecifierUnresolved,
    /// Project wrappers of the name, sorted by `file`, then `export`.
    pub wrappers: Vec<UsageWrapper>,
}

/// Uses of one imported name that the trace cannot resolve to a call. The key
/// set is closed in schema version 1.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct SpecifierUnresolved {
    /// The name is the whole initializer of a declarator that is not a
    /// wrapper: `const s = useSelector`.
    pub value_alias: usize,
    /// Any other value use that is not a call: an argument, an array
    /// element, a property value or an optional call.
    pub non_call_reference: usize,
    /// The name is a JSX element: `<Provider>`.
    pub jsx_element: usize,
    /// The file re-exports the name from the package. The trace does not
    /// follow the consumers of the re-export.
    pub re_export: usize,
    /// A wrapper consumer that exports the wrapper again. The trace does not
    /// follow the second hop.
    pub nested_wrapper: usize,
    /// Files with a runtime binding of the name and no usage site, for
    /// example a use in a Vue, Svelte or Astro template.
    pub binding_without_site: usize,
}

/// Uses that hide which names a file reads. The key set is closed in schema
/// version 1.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct FileLevelUnresolved {
    /// `import("package")` expressions.
    pub dynamic_import: usize,
    /// `require("package")` calls.
    pub require: usize,
    /// `import "package"` statements.
    pub side_effect_import: usize,
    /// `export * from "package"` statements.
    pub star_re_export: usize,
    /// Files that import the package through a specifier that does not name
    /// the package, for example a path alias.
    pub unattributed_file: usize,
}

/// The shape of a project wrapper.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum WrapperShape {
    /// The wrapper is the result of a call of the name:
    /// `useSelector.withTypes<RootState>()`.
    Call,
    /// The wrapper is the name itself: `export const useAppDispatch = useDispatch`.
    Alias,
}

/// A top-level exported declarator that wraps an imported name. The trace
/// follows one hop from the wrapper to its consumers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct UsageWrapper {
    /// The file that declares the wrapper, root-relative.
    pub file: String,
    /// The exported name of the wrapper.
    pub export: String,
    /// The shape of the wrapper.
    pub shape: WrapperShape,
    /// 1-based line of the wrapper initializer.
    pub line: u32,
    /// Distinct files with a call of the wrapper.
    pub consumer_file_count: usize,
    /// Calls of the wrapper.
    pub call_site_count: usize,
}

/// One page of usage sites.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct UsageSitePage {
    /// The sites on this page, sorted by `file`, `line`, `col`, kind,
    /// `specifier`, then `via`.
    pub items: Vec<UsageSite>,
    /// The number of sites on all pages.
    pub total: usize,
    /// The largest number of items on a page.
    pub limit: u16,
    /// An opaque token for the next page. Absent on the last page.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
}

/// One use of the dependency in the code.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct UsageSite {
    /// The file, root-relative.
    pub file: String,
    /// 1-based line.
    pub line: u32,
    /// 0-based byte column.
    pub col: u32,
    /// The imported name. Absent on file-level kinds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub specifier: Option<String>,
    /// The local binding that the code uses. Absent on file-level kinds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub local_name: Option<String>,
    /// The static member after the imported name, for example `withTypes`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub member: Option<String>,
    /// How the code uses the dependency at this site.
    pub kind: UsageSiteKind,
    /// The wrapper that the site goes through, as `FILE:EXPORT`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub via: Option<String>,
}

/// How the code uses the dependency at a site.
///
/// The set is open: read an unknown kind as an unresolved site. The order of
/// the variants is the sort order of sites at the same position.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum UsageSiteKind {
    /// A call of the name, directly or through a wrapper.
    Call,
    /// The initializer of a project wrapper.
    WrapperDefinition,
    /// The whole initializer of a declarator that is not a wrapper.
    ValueAlias,
    /// A value use that is not a call.
    NonCallReference,
    /// A JSX element.
    JsxElement,
    /// A named re-export from the package.
    ReExport,
    /// A wrapper consumer that exports the wrapper again.
    NestedWrapper,
    /// An `import("package")` expression.
    DynamicImport,
    /// A `require("package")` call.
    Require,
    /// An `import "package"` statement.
    SideEffectImport,
    /// An `export * from "package"` statement.
    StarReExport,
}

impl UsageSiteKind {
    /// The `snake_case` wire name of the kind.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Call => "call",
            Self::WrapperDefinition => "wrapper_definition",
            Self::ValueAlias => "value_alias",
            Self::NonCallReference => "non_call_reference",
            Self::JsxElement => "jsx_element",
            Self::ReExport => "re_export",
            Self::NestedWrapper => "nested_wrapper",
            Self::DynamicImport => "dynamic_import",
            Self::Require => "require",
            Self::SideEffectImport => "side_effect_import",
            Self::StarReExport => "star_re_export",
        }
    }
}

/// The files that import the users of the dependency, found by a reverse walk
/// of the import graph.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct ConsumerClosure {
    /// The largest depth of the walk.
    pub depth: u32,
    /// The number of files in `files`.
    pub file_count: usize,
    /// The files, sorted by `depth`, then `file`. The seed files are not
    /// listed.
    pub files: Vec<ClosureFile>,
    /// Whether a file at the largest depth has an importer that the walk did
    /// not visit.
    pub truncated: bool,
}

/// One file of a [`ConsumerClosure`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct ClosureFile {
    /// The file, root-relative.
    pub file: String,
    /// The number of import edges from the nearest seed file.
    pub depth: u32,
}

/// The default number of sites on a page.
pub const DEFAULT_USAGE_SITE_LIMIT: u16 = 50;
/// The largest number of sites on a page.
pub const MAX_USAGE_SITE_LIMIT: u16 = 500;
/// The largest closure depth.
pub const MAX_USAGE_CLOSURE_DEPTH: u32 = 10;

/// A request for one page of usage sites.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SitePageRequest {
    /// The largest number of sites on the page, 1 to 500.
    pub limit: u16,
    /// The `next_cursor` of the previous page.
    pub cursor: Option<String>,
}

/// What a dependency usage trace reports.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DependencyUsageQuery {
    /// Imported names to report. Empty reports all names.
    pub specifiers: Vec<String>,
    /// The page of sites to report, or `None` for counts only.
    pub sites: Option<SitePageRequest>,
    /// The depth of the consumer closure, or `None` for no closure.
    pub closure_depth: Option<u32>,
}

/// An invalid [`DependencyUsageQuery`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UsageQueryError {
    /// The site limit is not in 1..=500.
    LimitOutOfRange(u16),
    /// The closure depth is not in 1..=10.
    DepthOutOfRange(u32),
}

impl std::fmt::Display for UsageQueryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::LimitOutOfRange(limit) => write!(
                f,
                "the site limit must be between 1 and {MAX_USAGE_SITE_LIMIT}, got {limit}"
            ),
            Self::DepthOutOfRange(depth) => write!(
                f,
                "the closure depth must be between 1 and {MAX_USAGE_CLOSURE_DEPTH}, got {depth}"
            ),
        }
    }
}

impl std::error::Error for UsageQueryError {}

impl DependencyUsageQuery {
    /// Build a validated query.
    ///
    /// # Errors
    ///
    /// Returns an error when the site limit is not in 1..=500 or the closure
    /// depth is not in 1..=10.
    pub fn new(
        specifiers: Vec<String>,
        sites: Option<SitePageRequest>,
        closure_depth: Option<u32>,
    ) -> Result<Self, UsageQueryError> {
        if let Some(page) = &sites
            && !(1..=MAX_USAGE_SITE_LIMIT).contains(&page.limit)
        {
            return Err(UsageQueryError::LimitOutOfRange(page.limit));
        }
        if let Some(depth) = closure_depth
            && !(1..=MAX_USAGE_CLOSURE_DEPTH).contains(&depth)
        {
            return Err(UsageQueryError::DepthOutOfRange(depth));
        }
        Ok(Self {
            specifiers,
            sites,
            closure_depth,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_rejects_out_of_range_limit_and_depth() {
        let page = |limit| {
            Some(SitePageRequest {
                limit,
                cursor: None,
            })
        };
        assert_eq!(
            DependencyUsageQuery::new(Vec::new(), page(0), None),
            Err(UsageQueryError::LimitOutOfRange(0))
        );
        assert_eq!(
            DependencyUsageQuery::new(Vec::new(), page(501), None),
            Err(UsageQueryError::LimitOutOfRange(501))
        );
        assert_eq!(
            DependencyUsageQuery::new(Vec::new(), None, Some(0)),
            Err(UsageQueryError::DepthOutOfRange(0))
        );
        assert_eq!(
            DependencyUsageQuery::new(Vec::new(), None, Some(11)),
            Err(UsageQueryError::DepthOutOfRange(11))
        );
        assert!(DependencyUsageQuery::new(Vec::new(), page(500), Some(10)).is_ok());
        assert!(DependencyUsageQuery::new(Vec::new(), page(1), Some(1)).is_ok());
    }

    #[test]
    fn site_kind_wire_names_match_serde() {
        for kind in [
            UsageSiteKind::Call,
            UsageSiteKind::WrapperDefinition,
            UsageSiteKind::ValueAlias,
            UsageSiteKind::NonCallReference,
            UsageSiteKind::JsxElement,
            UsageSiteKind::ReExport,
            UsageSiteKind::NestedWrapper,
            UsageSiteKind::DynamicImport,
            UsageSiteKind::Require,
            UsageSiteKind::SideEffectImport,
            UsageSiteKind::StarReExport,
        ] {
            assert_eq!(
                serde_json::to_value(kind).expect("a site kind serializes"),
                serde_json::Value::String(kind.as_str().to_owned())
            );
        }
    }
}
