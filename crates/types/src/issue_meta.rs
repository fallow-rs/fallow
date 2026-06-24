//! Shared issue-type contract metadata.

use crate::suppress::IssueKind;

/// Shared contract facts for issue-like diagnostics.
///
/// Curated prose stays with the surface that owns it. This table is only for
/// stable machine-facing facts that otherwise drift across CLI schema, LSP,
/// MCP, and suppression helpers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IssueKindMeta {
    /// Backing suppression issue kind, when this row maps to one.
    pub kind: Option<IssueKind>,
    /// Canonical issue code used in output and diagnostic codes.
    pub code: &'static str,
    /// Accepted aliases for config, suppression, or migration compatibility.
    pub aliases: &'static [&'static str],
    /// User-facing label for editor and capability surfaces.
    pub label: &'static str,
    /// Canonical `[rules]` key when the issue is configurable.
    pub config_key: Option<&'static str>,
    /// Dead-code CLI filter flag, when one exists.
    pub filter_flag: Option<&'static str>,
    /// MCP `issue_types` selector, when this issue can be selected there.
    pub mcp_issue_type: Option<&'static str>,
    /// Suppression token agents should emit, when suppressible.
    pub suppress_token: Option<&'static str>,
    /// Whether the suppression comment should use `fallow-ignore-file`.
    pub suppress_file_level: bool,
    /// Whether the LSP exposes this row through initialization options and
    /// `fallow/issueTypes`.
    pub lsp: bool,
    /// Broad documentation category for authoring and generated manifests.
    pub docs_category: &'static str,
}

impl IssueKindMeta {
    /// Return the filter flag as an MCP selector pair.
    #[must_use]
    pub const fn mcp_pair(self) -> Option<(&'static str, &'static str)> {
        match (self.mcp_issue_type, self.filter_flag) {
            (Some(issue_type), Some(flag)) => Some((issue_type, flag)),
            _ => None,
        }
    }
}

/// All shared issue metadata rows.
pub const ISSUE_KIND_META: &[IssueKindMeta] = &[
    IssueKindMeta {
        kind: Some(IssueKind::CodeDuplication),
        code: "code-duplication",
        aliases: &[],
        label: "Code Duplication",
        config_key: None,
        filter_flag: None,
        mcp_issue_type: None,
        suppress_token: Some("code-duplication"),
        suppress_file_level: false,
        lsp: true,
        docs_category: "dupes",
    },
    IssueKindMeta {
        kind: Some(IssueKind::UnusedFile),
        code: "unused-file",
        aliases: &[],
        label: "Unused Files",
        config_key: Some("unused-files"),
        filter_flag: Some("--unused-files"),
        mcp_issue_type: Some("unused-files"),
        suppress_token: Some("unused-file"),
        suppress_file_level: true,
        lsp: true,
        docs_category: "source",
    },
    IssueKindMeta {
        kind: Some(IssueKind::UnusedExport),
        code: "unused-export",
        aliases: &[],
        label: "Unused Exports",
        config_key: Some("unused-exports"),
        filter_flag: Some("--unused-exports"),
        mcp_issue_type: Some("unused-exports"),
        suppress_token: Some("unused-export"),
        suppress_file_level: false,
        lsp: true,
        docs_category: "source",
    },
    IssueKindMeta {
        kind: Some(IssueKind::UnusedType),
        code: "unused-type",
        aliases: &[],
        label: "Unused Types",
        config_key: Some("unused-types"),
        filter_flag: Some("--unused-types"),
        mcp_issue_type: Some("unused-types"),
        suppress_token: Some("unused-type"),
        suppress_file_level: false,
        lsp: true,
        docs_category: "source",
    },
    IssueKindMeta {
        kind: Some(IssueKind::PrivateTypeLeak),
        code: "private-type-leak",
        aliases: &[],
        label: "Private Type Leaks",
        config_key: Some("private-type-leaks"),
        filter_flag: Some("--private-type-leaks"),
        mcp_issue_type: Some("private-type-leaks"),
        suppress_token: Some("private-type-leak"),
        suppress_file_level: false,
        lsp: true,
        docs_category: "source",
    },
    IssueKindMeta {
        kind: Some(IssueKind::UnusedDependency),
        code: "unused-dependency",
        aliases: &[],
        label: "Unused Dependencies",
        config_key: Some("unused-dependencies"),
        filter_flag: Some("--unused-deps"),
        mcp_issue_type: Some("unused-deps"),
        suppress_token: None,
        suppress_file_level: false,
        lsp: true,
        docs_category: "dependency",
    },
    IssueKindMeta {
        kind: Some(IssueKind::UnusedDevDependency),
        code: "unused-dev-dependency",
        aliases: &[],
        label: "Unused Dev Dependencies",
        config_key: Some("unused-dev-dependencies"),
        filter_flag: Some("--unused-deps"),
        mcp_issue_type: None,
        suppress_token: None,
        suppress_file_level: false,
        lsp: true,
        docs_category: "dependency",
    },
    IssueKindMeta {
        kind: None,
        code: "unused-optional-dependency",
        aliases: &[],
        label: "Unused Optional Dependencies",
        config_key: Some("unused-optional-dependencies"),
        filter_flag: Some("--unused-deps"),
        mcp_issue_type: None,
        suppress_token: None,
        suppress_file_level: false,
        lsp: true,
        docs_category: "dependency",
    },
    IssueKindMeta {
        kind: Some(IssueKind::UnusedEnumMember),
        code: "unused-enum-member",
        aliases: &[],
        label: "Unused Enum Members",
        config_key: Some("unused-enum-members"),
        filter_flag: Some("--unused-enum-members"),
        mcp_issue_type: Some("unused-enum-members"),
        suppress_token: Some("unused-enum-member"),
        suppress_file_level: false,
        lsp: true,
        docs_category: "source",
    },
    IssueKindMeta {
        kind: Some(IssueKind::UnusedClassMember),
        code: "unused-class-member",
        aliases: &[],
        label: "Unused Class Members",
        config_key: Some("unused-class-members"),
        filter_flag: Some("--unused-class-members"),
        mcp_issue_type: Some("unused-class-members"),
        suppress_token: Some("unused-class-member"),
        suppress_file_level: false,
        lsp: true,
        docs_category: "source",
    },
    IssueKindMeta {
        kind: Some(IssueKind::UnusedStoreMember),
        code: "unused-store-member",
        aliases: &["unused-store-members"],
        label: "Unused Store Members",
        config_key: Some("unused-store-members"),
        filter_flag: Some("--unused-store-members"),
        mcp_issue_type: Some("unused-store-members"),
        suppress_token: Some("unused-store-member"),
        suppress_file_level: false,
        lsp: true,
        docs_category: "framework",
    },
    IssueKindMeta {
        kind: Some(IssueKind::UnresolvedImport),
        code: "unresolved-import",
        aliases: &[],
        label: "Unresolved Imports",
        config_key: Some("unresolved-imports"),
        filter_flag: Some("--unresolved-imports"),
        mcp_issue_type: Some("unresolved-imports"),
        suppress_token: Some("unresolved-import"),
        suppress_file_level: false,
        lsp: true,
        docs_category: "dependency",
    },
    IssueKindMeta {
        kind: Some(IssueKind::UnlistedDependency),
        code: "unlisted-dependency",
        aliases: &[],
        label: "Unlisted Dependencies",
        config_key: Some("unlisted-dependencies"),
        filter_flag: Some("--unlisted-deps"),
        mcp_issue_type: Some("unlisted-deps"),
        suppress_token: None,
        suppress_file_level: false,
        lsp: true,
        docs_category: "dependency",
    },
    IssueKindMeta {
        kind: Some(IssueKind::DuplicateExport),
        code: "duplicate-export",
        aliases: &[],
        label: "Duplicate Exports",
        config_key: Some("duplicate-exports"),
        filter_flag: Some("--duplicate-exports"),
        mcp_issue_type: Some("duplicate-exports"),
        suppress_token: Some("duplicate-export"),
        suppress_file_level: true,
        lsp: true,
        docs_category: "source",
    },
    IssueKindMeta {
        kind: Some(IssueKind::TypeOnlyDependency),
        code: "type-only-dependency",
        aliases: &[],
        label: "Type-Only Dependencies",
        config_key: Some("type-only-dependencies"),
        filter_flag: Some("--unused-deps"),
        mcp_issue_type: None,
        suppress_token: None,
        suppress_file_level: false,
        lsp: true,
        docs_category: "dependency",
    },
    IssueKindMeta {
        kind: Some(IssueKind::TestOnlyDependency),
        code: "test-only-dependency",
        aliases: &[],
        label: "Test-Only Dependencies",
        config_key: Some("test-only-dependencies"),
        filter_flag: Some("--unused-deps"),
        mcp_issue_type: None,
        suppress_token: None,
        suppress_file_level: false,
        lsp: true,
        docs_category: "dependency",
    },
    IssueKindMeta {
        kind: Some(IssueKind::CircularDependency),
        code: "circular-dependency",
        aliases: &["circular-dependencies"],
        label: "Circular Dependencies",
        config_key: Some("circular-dependencies"),
        filter_flag: Some("--circular-deps"),
        mcp_issue_type: Some("circular-deps"),
        suppress_token: Some("circular-dependency"),
        suppress_file_level: false,
        lsp: true,
        docs_category: "architecture",
    },
    IssueKindMeta {
        kind: Some(IssueKind::ReExportCycle),
        code: "re-export-cycle",
        aliases: &["re-export-cycles", "reexport-cycle", "reexport-cycles"],
        label: "Re-Export Cycles",
        config_key: Some("re-export-cycle"),
        filter_flag: Some("--re-export-cycles"),
        mcp_issue_type: Some("re-export-cycles"),
        suppress_token: Some("re-export-cycle"),
        suppress_file_level: true,
        lsp: true,
        docs_category: "architecture",
    },
    IssueKindMeta {
        kind: Some(IssueKind::BoundaryViolation),
        code: "boundary-violation",
        aliases: &[],
        label: "Boundary Violations",
        config_key: Some("boundary-violation"),
        filter_flag: Some("--boundary-violations"),
        mcp_issue_type: Some("boundary-violations"),
        suppress_token: Some("boundary-violation"),
        suppress_file_level: false,
        lsp: true,
        docs_category: "architecture",
    },
    IssueKindMeta {
        kind: None,
        code: "boundary-coverage",
        aliases: &["boundary-coverage-violations"],
        label: "Boundary Coverage",
        config_key: Some("boundary-violation"),
        filter_flag: Some("--boundary-violations"),
        mcp_issue_type: None,
        suppress_token: Some("boundary-violation"),
        suppress_file_level: true,
        lsp: false,
        docs_category: "architecture",
    },
    IssueKindMeta {
        kind: Some(IssueKind::BoundaryViolation),
        code: "boundary-call-violation",
        aliases: &["boundary-call-violations"],
        label: "Boundary Call Violations",
        config_key: Some("boundary-violation"),
        filter_flag: Some("--boundary-violations"),
        mcp_issue_type: None,
        suppress_token: Some("boundary-call-violation"),
        suppress_file_level: false,
        lsp: false,
        docs_category: "architecture",
    },
    IssueKindMeta {
        kind: Some(IssueKind::PolicyViolation),
        code: "policy-violation",
        aliases: &["policy-violations"],
        label: "Policy Violations",
        config_key: Some("policy-violation"),
        filter_flag: Some("--policy-violations"),
        mcp_issue_type: Some("policy-violations"),
        suppress_token: Some("policy-violation"),
        suppress_file_level: false,
        lsp: true,
        docs_category: "architecture",
    },
    IssueKindMeta {
        kind: Some(IssueKind::InvalidClientExport),
        code: "invalid-client-export",
        aliases: &["invalid-client-exports"],
        label: "Invalid Client Exports",
        config_key: Some("invalid-client-export"),
        filter_flag: None,
        mcp_issue_type: None,
        suppress_token: Some("invalid-client-export"),
        suppress_file_level: false,
        lsp: true,
        docs_category: "framework",
    },
    IssueKindMeta {
        kind: Some(IssueKind::MixedClientServerBarrel),
        code: "mixed-client-server-barrel",
        aliases: &["mixed-client-server-barrels"],
        label: "Mixed Client/Server Barrels",
        config_key: Some("mixed-client-server-barrel"),
        filter_flag: None,
        mcp_issue_type: None,
        suppress_token: Some("mixed-client-server-barrel"),
        suppress_file_level: false,
        lsp: true,
        docs_category: "framework",
    },
    IssueKindMeta {
        kind: Some(IssueKind::MisplacedDirective),
        code: "misplaced-directive",
        aliases: &["misplaced-directives"],
        label: "Misplaced Directives",
        config_key: Some("misplaced-directive"),
        filter_flag: None,
        mcp_issue_type: None,
        suppress_token: Some("misplaced-directive"),
        suppress_file_level: false,
        lsp: true,
        docs_category: "framework",
    },
    IssueKindMeta {
        kind: Some(IssueKind::UnprovidedInject),
        code: "unprovided-inject",
        aliases: &["unprovided-injects"],
        label: "Unprovided Injects",
        config_key: Some("unprovided-injects"),
        filter_flag: Some("--unprovided-injects"),
        mcp_issue_type: Some("unprovided-injects"),
        suppress_token: Some("unprovided-inject"),
        suppress_file_level: false,
        lsp: true,
        docs_category: "framework",
    },
    IssueKindMeta {
        kind: Some(IssueKind::UnrenderedComponent),
        code: "unrendered-component",
        aliases: &["unrendered-components"],
        label: "Unrendered Components",
        config_key: Some("unrendered-components"),
        filter_flag: Some("--unrendered-components"),
        mcp_issue_type: Some("unrendered-components"),
        suppress_token: Some("unrendered-component"),
        suppress_file_level: false,
        lsp: true,
        docs_category: "framework",
    },
    IssueKindMeta {
        kind: Some(IssueKind::UnusedComponentProp),
        code: "unused-component-prop",
        aliases: &["unused-component-props"],
        label: "Unused Component Props",
        config_key: Some("unused-component-props"),
        filter_flag: Some("--unused-component-props"),
        mcp_issue_type: Some("unused-component-props"),
        suppress_token: Some("unused-component-prop"),
        suppress_file_level: false,
        lsp: true,
        docs_category: "framework",
    },
    IssueKindMeta {
        kind: Some(IssueKind::UnusedComponentEmit),
        code: "unused-component-emit",
        aliases: &["unused-component-emits"],
        label: "Unused Component Emits",
        config_key: Some("unused-component-emits"),
        filter_flag: Some("--unused-component-emits"),
        mcp_issue_type: Some("unused-component-emits"),
        suppress_token: Some("unused-component-emit"),
        suppress_file_level: false,
        lsp: true,
        docs_category: "framework",
    },
    IssueKindMeta {
        kind: Some(IssueKind::UnusedComponentInput),
        code: "unused-component-input",
        aliases: &["unused-component-inputs"],
        label: "Unused Component Inputs",
        config_key: Some("unused-component-inputs"),
        filter_flag: Some("--unused-component-inputs"),
        mcp_issue_type: Some("unused-component-inputs"),
        suppress_token: Some("unused-component-input"),
        suppress_file_level: false,
        lsp: true,
        docs_category: "framework",
    },
    IssueKindMeta {
        kind: Some(IssueKind::UnusedComponentOutput),
        code: "unused-component-output",
        aliases: &["unused-component-outputs"],
        label: "Unused Component Outputs",
        config_key: Some("unused-component-outputs"),
        filter_flag: Some("--unused-component-outputs"),
        mcp_issue_type: Some("unused-component-outputs"),
        suppress_token: Some("unused-component-output"),
        suppress_file_level: false,
        lsp: true,
        docs_category: "framework",
    },
    IssueKindMeta {
        kind: Some(IssueKind::UnusedSvelteEvent),
        code: "unused-svelte-event",
        aliases: &["unused-svelte-events"],
        label: "Unused Svelte Events",
        config_key: Some("unused-svelte-events"),
        filter_flag: Some("--unused-svelte-events"),
        mcp_issue_type: Some("unused-svelte-events"),
        suppress_token: Some("unused-svelte-event"),
        suppress_file_level: false,
        lsp: true,
        docs_category: "framework",
    },
    IssueKindMeta {
        kind: Some(IssueKind::UnusedServerAction),
        code: "unused-server-action",
        aliases: &["unused-server-actions"],
        label: "Unused Server Actions",
        config_key: Some("unused-server-actions"),
        filter_flag: Some("--unused-server-actions"),
        mcp_issue_type: Some("unused-server-actions"),
        suppress_token: Some("unused-server-action"),
        suppress_file_level: false,
        lsp: true,
        docs_category: "framework",
    },
    IssueKindMeta {
        kind: Some(IssueKind::UnusedLoadDataKey),
        code: "unused-load-data-key",
        aliases: &["unused-load-data-keys"],
        label: "Unused Load Data Keys",
        config_key: Some("unused-load-data-keys"),
        filter_flag: Some("--unused-load-data-keys"),
        mcp_issue_type: Some("unused-load-data-keys"),
        suppress_token: Some("unused-load-data-key"),
        suppress_file_level: false,
        lsp: true,
        docs_category: "framework",
    },
    IssueKindMeta {
        kind: Some(IssueKind::RouteCollision),
        code: "route-collision",
        aliases: &["route-collisions"],
        label: "Route Collisions",
        config_key: Some("route-collision"),
        filter_flag: None,
        mcp_issue_type: None,
        suppress_token: Some("route-collision"),
        suppress_file_level: true,
        lsp: true,
        docs_category: "framework",
    },
    IssueKindMeta {
        kind: Some(IssueKind::DynamicSegmentNameConflict),
        code: "dynamic-segment-name-conflict",
        aliases: &["dynamic-segment-name-conflicts"],
        label: "Dynamic Segment Conflicts",
        config_key: Some("dynamic-segment-name-conflict"),
        filter_flag: None,
        mcp_issue_type: None,
        suppress_token: Some("dynamic-segment-name-conflict"),
        suppress_file_level: true,
        lsp: true,
        docs_category: "framework",
    },
    IssueKindMeta {
        kind: Some(IssueKind::StaleSuppression),
        code: "stale-suppression",
        aliases: &[],
        label: "Stale Suppressions",
        config_key: Some("stale-suppressions"),
        filter_flag: Some("--stale-suppressions"),
        mcp_issue_type: Some("stale-suppressions"),
        suppress_token: None,
        suppress_file_level: false,
        lsp: true,
        docs_category: "source",
    },
    IssueKindMeta {
        kind: Some(IssueKind::PnpmCatalogEntry),
        code: "unused-catalog-entry",
        aliases: &["unused-catalog-entries"],
        label: "Unused Catalog Entries",
        config_key: Some("unused-catalog-entries"),
        filter_flag: Some("--unused-catalog-entries"),
        mcp_issue_type: Some("unused-catalog-entries"),
        suppress_token: None,
        suppress_file_level: false,
        lsp: true,
        docs_category: "dependency",
    },
    IssueKindMeta {
        kind: Some(IssueKind::EmptyCatalogGroup),
        code: "empty-catalog-group",
        aliases: &["empty-catalog-groups"],
        label: "Empty Catalog Groups",
        config_key: Some("empty-catalog-groups"),
        filter_flag: Some("--empty-catalog-groups"),
        mcp_issue_type: Some("empty-catalog-groups"),
        suppress_token: None,
        suppress_file_level: false,
        lsp: true,
        docs_category: "dependency",
    },
    IssueKindMeta {
        kind: Some(IssueKind::UnresolvedCatalogReference),
        code: "unresolved-catalog-reference",
        aliases: &["unresolved-catalog-references"],
        label: "Unresolved Catalog References",
        config_key: Some("unresolved-catalog-references"),
        filter_flag: Some("--unresolved-catalog-references"),
        mcp_issue_type: Some("unresolved-catalog-references"),
        suppress_token: None,
        suppress_file_level: false,
        lsp: true,
        docs_category: "dependency",
    },
    IssueKindMeta {
        kind: Some(IssueKind::UnusedDependencyOverride),
        code: "unused-dependency-override",
        aliases: &["unused-dependency-overrides"],
        label: "Unused Dependency Overrides",
        config_key: Some("unused-dependency-overrides"),
        filter_flag: Some("--unused-dependency-overrides"),
        mcp_issue_type: Some("unused-dependency-overrides"),
        suppress_token: None,
        suppress_file_level: false,
        lsp: true,
        docs_category: "dependency",
    },
    IssueKindMeta {
        kind: Some(IssueKind::MisconfiguredDependencyOverride),
        code: "misconfigured-dependency-override",
        aliases: &["misconfigured-dependency-overrides"],
        label: "Misconfigured Dependency Overrides",
        config_key: Some("misconfigured-dependency-overrides"),
        filter_flag: Some("--misconfigured-dependency-overrides"),
        mcp_issue_type: Some("misconfigured-dependency-overrides"),
        suppress_token: None,
        suppress_file_level: false,
        lsp: true,
        docs_category: "dependency",
    },
    IssueKindMeta {
        kind: Some(IssueKind::SecuritySink),
        code: "security-sink",
        aliases: &[],
        label: "Security Sink Candidates",
        config_key: Some("security-sink"),
        filter_flag: None,
        mcp_issue_type: None,
        suppress_token: Some("security-sink"),
        suppress_file_level: false,
        lsp: true,
        docs_category: "security",
    },
    IssueKindMeta {
        kind: Some(IssueKind::SecurityClientServerLeak),
        code: "security-client-server-leak",
        aliases: &[],
        label: "Security Client-Server Leaks",
        config_key: Some("security-client-server-leak"),
        filter_flag: None,
        mcp_issue_type: None,
        suppress_token: Some("security-client-server-leak"),
        suppress_file_level: true,
        lsp: true,
        docs_category: "security",
    },
    IssueKindMeta {
        kind: Some(IssueKind::CoverageGaps),
        code: "coverage-gaps",
        aliases: &[],
        label: "Coverage Gaps",
        config_key: Some("coverage-gaps"),
        filter_flag: None,
        mcp_issue_type: None,
        suppress_token: Some("coverage-gaps"),
        suppress_file_level: true,
        lsp: false,
        docs_category: "health",
    },
    IssueKindMeta {
        kind: Some(IssueKind::FeatureFlag),
        code: "feature-flag",
        aliases: &[],
        label: "Feature Flags",
        config_key: Some("feature-flags"),
        filter_flag: None,
        mcp_issue_type: None,
        suppress_token: Some("feature-flag"),
        suppress_file_level: false,
        lsp: false,
        docs_category: "flags",
    },
    IssueKindMeta {
        kind: Some(IssueKind::Complexity),
        code: "complexity",
        aliases: &[],
        label: "Complexity",
        config_key: None,
        filter_flag: None,
        mcp_issue_type: None,
        suppress_token: Some("complexity"),
        suppress_file_level: false,
        lsp: false,
        docs_category: "health",
    },
    IssueKindMeta {
        kind: Some(IssueKind::PropDrilling),
        code: "prop-drilling",
        aliases: &[],
        label: "Prop Drilling",
        config_key: Some("prop-drilling"),
        filter_flag: None,
        mcp_issue_type: None,
        suppress_token: Some("prop-drilling"),
        suppress_file_level: false,
        lsp: false,
        docs_category: "source",
    },
    IssueKindMeta {
        kind: Some(IssueKind::ThinWrapper),
        code: "thin-wrapper",
        aliases: &["thin-wrappers"],
        label: "Thin Wrappers",
        config_key: Some("thin-wrapper"),
        filter_flag: None,
        mcp_issue_type: None,
        suppress_token: Some("thin-wrapper"),
        suppress_file_level: false,
        lsp: false,
        docs_category: "source",
    },
    IssueKindMeta {
        kind: Some(IssueKind::DuplicatePropShape),
        code: "duplicate-prop-shape",
        aliases: &["duplicate-prop-shapes"],
        label: "Duplicate Prop Shapes",
        config_key: Some("duplicate-prop-shape"),
        filter_flag: None,
        mcp_issue_type: None,
        suppress_token: Some("duplicate-prop-shape"),
        suppress_file_level: false,
        lsp: false,
        docs_category: "source",
    },
];

/// Shared contract facts for serialized `AnalysisResults` arrays.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct IssueResultMeta {
    /// Canonical issue code that owns this result array.
    pub code: &'static str,
    /// Serialized `AnalysisResults` array key that carries this issue row.
    pub result_key: &'static str,
    /// Whether `result_key` contributes to `AnalysisResults::total_issues()`.
    pub counts_in_total: bool,
}

/// All shared issue-to-result metadata rows.
pub const ISSUE_RESULT_META: &[IssueResultMeta] = &[
    IssueResultMeta {
        code: "unused-file",
        result_key: "unused_files",
        counts_in_total: true,
    },
    IssueResultMeta {
        code: "unused-export",
        result_key: "unused_exports",
        counts_in_total: true,
    },
    IssueResultMeta {
        code: "unused-type",
        result_key: "unused_types",
        counts_in_total: true,
    },
    IssueResultMeta {
        code: "private-type-leak",
        result_key: "private_type_leaks",
        counts_in_total: true,
    },
    IssueResultMeta {
        code: "unused-dependency",
        result_key: "unused_dependencies",
        counts_in_total: true,
    },
    IssueResultMeta {
        code: "unused-dev-dependency",
        result_key: "unused_dev_dependencies",
        counts_in_total: true,
    },
    IssueResultMeta {
        code: "unused-optional-dependency",
        result_key: "unused_optional_dependencies",
        counts_in_total: true,
    },
    IssueResultMeta {
        code: "unused-enum-member",
        result_key: "unused_enum_members",
        counts_in_total: true,
    },
    IssueResultMeta {
        code: "unused-class-member",
        result_key: "unused_class_members",
        counts_in_total: true,
    },
    IssueResultMeta {
        code: "unused-store-member",
        result_key: "unused_store_members",
        counts_in_total: true,
    },
    IssueResultMeta {
        code: "unresolved-import",
        result_key: "unresolved_imports",
        counts_in_total: true,
    },
    IssueResultMeta {
        code: "unlisted-dependency",
        result_key: "unlisted_dependencies",
        counts_in_total: true,
    },
    IssueResultMeta {
        code: "duplicate-export",
        result_key: "duplicate_exports",
        counts_in_total: true,
    },
    IssueResultMeta {
        code: "type-only-dependency",
        result_key: "type_only_dependencies",
        counts_in_total: true,
    },
    IssueResultMeta {
        code: "test-only-dependency",
        result_key: "test_only_dependencies",
        counts_in_total: true,
    },
    IssueResultMeta {
        code: "circular-dependency",
        result_key: "circular_dependencies",
        counts_in_total: true,
    },
    IssueResultMeta {
        code: "re-export-cycle",
        result_key: "re_export_cycles",
        counts_in_total: true,
    },
    IssueResultMeta {
        code: "boundary-violation",
        result_key: "boundary_violations",
        counts_in_total: true,
    },
    IssueResultMeta {
        code: "boundary-coverage",
        result_key: "boundary_coverage_violations",
        counts_in_total: true,
    },
    IssueResultMeta {
        code: "boundary-call-violation",
        result_key: "boundary_call_violations",
        counts_in_total: true,
    },
    IssueResultMeta {
        code: "policy-violation",
        result_key: "policy_violations",
        counts_in_total: true,
    },
    IssueResultMeta {
        code: "invalid-client-export",
        result_key: "invalid_client_exports",
        counts_in_total: true,
    },
    IssueResultMeta {
        code: "mixed-client-server-barrel",
        result_key: "mixed_client_server_barrels",
        counts_in_total: true,
    },
    IssueResultMeta {
        code: "misplaced-directive",
        result_key: "misplaced_directives",
        counts_in_total: true,
    },
    IssueResultMeta {
        code: "unprovided-inject",
        result_key: "unprovided_injects",
        counts_in_total: true,
    },
    IssueResultMeta {
        code: "unrendered-component",
        result_key: "unrendered_components",
        counts_in_total: true,
    },
    IssueResultMeta {
        code: "unused-component-prop",
        result_key: "unused_component_props",
        counts_in_total: true,
    },
    IssueResultMeta {
        code: "unused-component-emit",
        result_key: "unused_component_emits",
        counts_in_total: true,
    },
    IssueResultMeta {
        code: "unused-component-input",
        result_key: "unused_component_inputs",
        counts_in_total: true,
    },
    IssueResultMeta {
        code: "unused-component-output",
        result_key: "unused_component_outputs",
        counts_in_total: true,
    },
    IssueResultMeta {
        code: "unused-svelte-event",
        result_key: "unused_svelte_events",
        counts_in_total: true,
    },
    IssueResultMeta {
        code: "unused-server-action",
        result_key: "unused_server_actions",
        counts_in_total: true,
    },
    IssueResultMeta {
        code: "unused-load-data-key",
        result_key: "unused_load_data_keys",
        counts_in_total: true,
    },
    IssueResultMeta {
        code: "route-collision",
        result_key: "route_collisions",
        counts_in_total: true,
    },
    IssueResultMeta {
        code: "dynamic-segment-name-conflict",
        result_key: "dynamic_segment_name_conflicts",
        counts_in_total: true,
    },
    IssueResultMeta {
        code: "stale-suppression",
        result_key: "stale_suppressions",
        counts_in_total: true,
    },
    IssueResultMeta {
        code: "unused-catalog-entry",
        result_key: "unused_catalog_entries",
        counts_in_total: true,
    },
    IssueResultMeta {
        code: "empty-catalog-group",
        result_key: "empty_catalog_groups",
        counts_in_total: true,
    },
    IssueResultMeta {
        code: "unresolved-catalog-reference",
        result_key: "unresolved_catalog_references",
        counts_in_total: true,
    },
    IssueResultMeta {
        code: "unused-dependency-override",
        result_key: "unused_dependency_overrides",
        counts_in_total: true,
    },
    IssueResultMeta {
        code: "misconfigured-dependency-override",
        result_key: "misconfigured_dependency_overrides",
        counts_in_total: true,
    },
    IssueResultMeta {
        code: "prop-drilling",
        result_key: "prop_drilling_chains",
        counts_in_total: false,
    },
    IssueResultMeta {
        code: "thin-wrapper",
        result_key: "thin_wrappers",
        counts_in_total: false,
    },
    IssueResultMeta {
        code: "duplicate-prop-shape",
        result_key: "duplicate_prop_shapes",
        counts_in_total: false,
    },
];

/// Canonical names and aliases accepted by `IssueKind::parse`.
pub const KNOWN_ISSUE_KIND_NAMES: &[&str] = &[
    "unused-file",
    "unused-export",
    "unused-type",
    "private-type-leak",
    "unused-dependency",
    "unused-dev-dependency",
    "unused-enum-member",
    "unused-class-member",
    "unresolved-import",
    "unlisted-dependency",
    "duplicate-export",
    "code-duplication",
    "circular-dependency",
    "circular-dependencies",
    "re-export-cycle",
    "re-export-cycles",
    "reexport-cycle",
    "reexport-cycles",
    "type-only-dependency",
    "test-only-dependency",
    "boundary-violation",
    "boundary-call-violation",
    "boundary-call-violations",
    "coverage-gaps",
    "feature-flag",
    "complexity",
    "stale-suppression",
    "unused-catalog-entry",
    "unused-catalog-entries",
    "empty-catalog-group",
    "empty-catalog-groups",
    "unresolved-catalog-reference",
    "unresolved-catalog-references",
    "unused-dependency-override",
    "unused-dependency-overrides",
    "misconfigured-dependency-override",
    "misconfigured-dependency-overrides",
    "security-client-server-leak",
    "security-sink",
    "policy-violation",
    "policy-violations",
    "invalid-client-export",
    "invalid-client-exports",
    "mixed-client-server-barrel",
    "mixed-client-server-barrels",
    "misplaced-directive",
    "misplaced-directives",
    "unused-store-member",
    "unused-store-members",
    "unprovided-inject",
    "unprovided-injects",
    "route-collision",
    "route-collisions",
    "dynamic-segment-name-conflict",
    "dynamic-segment-name-conflicts",
    "unrendered-component",
    "unrendered-components",
    "unused-component-prop",
    "unused-component-props",
    "unused-component-emit",
    "unused-component-emits",
    "unused-component-input",
    "unused-component-inputs",
    "unused-component-output",
    "unused-component-outputs",
    "unused-server-action",
    "unused-server-actions",
    "unused-load-data-key",
    "unused-load-data-keys",
    "prop-drilling",
    "thin-wrapper",
    "thin-wrappers",
    "duplicate-prop-shape",
    "duplicate-prop-shapes",
    "unused-svelte-event",
    "unused-svelte-events",
];

/// CLI filter flags on `fallow dead-code` that scope output to one issue family.
pub const DEAD_CODE_FILTER_FLAGS: &[&str] = &[
    "--unused-files",
    "--unused-exports",
    "--unused-types",
    "--private-type-leaks",
    "--unused-deps",
    "--unused-enum-members",
    "--unused-class-members",
    "--unused-store-members",
    "--unprovided-injects",
    "--unrendered-components",
    "--unused-component-props",
    "--unused-component-emits",
    "--unused-component-inputs",
    "--unused-component-outputs",
    "--unused-svelte-events",
    "--unused-server-actions",
    "--unused-load-data-keys",
    "--unresolved-imports",
    "--unlisted-deps",
    "--duplicate-exports",
    "--circular-deps",
    "--re-export-cycles",
    "--boundary-violations",
    "--policy-violations",
    "--stale-suppressions",
    "--unused-catalog-entries",
    "--empty-catalog-groups",
    "--unresolved-catalog-references",
    "--unused-dependency-overrides",
    "--misconfigured-dependency-overrides",
];

/// MCP issue selector names mapped to dead-code CLI flags.
pub const MCP_ISSUE_TYPE_FLAGS: &[(&str, &str)] = &[
    ("unused-files", "--unused-files"),
    ("unused-exports", "--unused-exports"),
    ("unused-types", "--unused-types"),
    ("private-type-leaks", "--private-type-leaks"),
    ("unused-deps", "--unused-deps"),
    ("unused-enum-members", "--unused-enum-members"),
    ("unused-class-members", "--unused-class-members"),
    ("unused-store-members", "--unused-store-members"),
    ("unprovided-injects", "--unprovided-injects"),
    ("unrendered-components", "--unrendered-components"),
    ("unused-component-props", "--unused-component-props"),
    ("unused-component-emits", "--unused-component-emits"),
    ("unused-component-inputs", "--unused-component-inputs"),
    ("unused-component-outputs", "--unused-component-outputs"),
    ("unused-svelte-events", "--unused-svelte-events"),
    ("unused-server-actions", "--unused-server-actions"),
    ("unused-load-data-keys", "--unused-load-data-keys"),
    ("unresolved-imports", "--unresolved-imports"),
    ("unlisted-deps", "--unlisted-deps"),
    ("duplicate-exports", "--duplicate-exports"),
    ("circular-deps", "--circular-deps"),
    ("re-export-cycles", "--re-export-cycles"),
    ("boundary-violations", "--boundary-violations"),
    ("policy-violations", "--policy-violations"),
    ("stale-suppressions", "--stale-suppressions"),
    ("unused-catalog-entries", "--unused-catalog-entries"),
    ("empty-catalog-groups", "--empty-catalog-groups"),
    (
        "unresolved-catalog-references",
        "--unresolved-catalog-references",
    ),
    (
        "unused-dependency-overrides",
        "--unused-dependency-overrides",
    ),
    (
        "misconfigured-dependency-overrides",
        "--misconfigured-dependency-overrides",
    ),
];

/// Lookup metadata by canonical code.
#[must_use]
pub fn issue_meta_by_code(code: &str) -> Option<&'static IssueKindMeta> {
    ISSUE_KIND_META.iter().find(|meta| meta.code == code)
}

/// Lookup metadata by canonical code or alias.
#[must_use]
pub fn issue_meta_for_token(token: &str) -> Option<&'static IssueKindMeta> {
    ISSUE_KIND_META
        .iter()
        .find(|meta| meta.code == token || meta.aliases.contains(&token))
}

/// Lookup metadata by backing issue kind.
#[must_use]
pub fn issue_meta_by_kind(kind: IssueKind) -> Option<&'static IssueKindMeta> {
    ISSUE_KIND_META.iter().find(|meta| meta.kind == Some(kind))
}

/// Lookup serialized result metadata by canonical issue code.
#[must_use]
pub fn issue_result_meta_by_code(code: &str) -> Option<&'static IssueResultMeta> {
    ISSUE_RESULT_META.iter().find(|meta| meta.code == code)
}

/// Rows exposed by the LSP issue-type capability.
pub fn diagnostic_issue_metas() -> impl Iterator<Item = &'static IssueKindMeta> {
    ISSUE_KIND_META.iter().filter(|meta| meta.lsp)
}

/// Rows that map to a serialized `AnalysisResults` array.
pub fn result_issue_metas() -> impl Iterator<Item = &'static IssueResultMeta> {
    ISSUE_RESULT_META.iter()
}

/// Rows whose serialized `AnalysisResults` array contributes to `total_issues`.
pub fn counted_result_issue_metas() -> impl Iterator<Item = &'static IssueResultMeta> {
    result_issue_metas().filter(|meta| meta.counts_in_total)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use crate::results::TOTAL_ISSUE_RESULT_KEYS;

    use super::*;

    #[test]
    fn known_names_round_trip_through_metadata() {
        for name in KNOWN_ISSUE_KIND_NAMES {
            let meta = issue_meta_for_token(name)
                .unwrap_or_else(|| panic!("known issue name {name} missing metadata row"));
            assert!(
                meta.kind.is_some(),
                "known issue name {name} maps to non-IssueKind metadata"
            );
        }
    }

    #[test]
    fn issue_kind_variants_have_metadata() {
        for discriminant in 1..=47 {
            let kind = IssueKind::from_discriminant(discriminant).unwrap();
            assert!(
                issue_meta_by_kind(kind).is_some(),
                "IssueKind {kind:?} has no metadata row"
            );
        }
    }

    #[test]
    fn dead_code_filter_flags_match_metadata() {
        let from_constants: BTreeSet<&str> = DEAD_CODE_FILTER_FLAGS.iter().copied().collect();
        let from_meta: BTreeSet<&str> = ISSUE_KIND_META
            .iter()
            .filter_map(|meta| meta.filter_flag)
            .collect();
        assert_eq!(from_constants, from_meta);
    }

    #[test]
    fn mcp_issue_type_flags_match_metadata() {
        let from_constants: BTreeSet<(&str, &str)> = MCP_ISSUE_TYPE_FLAGS.iter().copied().collect();
        let from_meta: BTreeSet<(&str, &str)> = ISSUE_KIND_META
            .iter()
            .filter_map(|meta| meta.mcp_pair())
            .collect();
        assert_eq!(from_constants, from_meta);
    }

    #[test]
    fn lsp_exposes_only_actual_diagnostic_codes() {
        let codes: BTreeSet<&str> = diagnostic_issue_metas().map(|meta| meta.code).collect();
        assert!(codes.contains("boundary-violation"));
        assert!(!codes.contains("boundary-coverage"));
        assert!(!codes.contains("boundary-call-violation"));
    }

    #[test]
    fn issue_codes_are_unique() {
        let mut seen = BTreeSet::new();
        for meta in ISSUE_KIND_META {
            assert!(seen.insert(meta.code), "duplicate issue code {}", meta.code);
        }
    }

    #[test]
    fn result_meta_codes_have_issue_metadata() {
        for meta in ISSUE_RESULT_META {
            assert!(
                issue_meta_by_code(meta.code).is_some(),
                "result metadata code {} has no issue metadata row",
                meta.code
            );
        }
    }

    #[test]
    fn result_keys_are_unique() {
        let mut seen = BTreeSet::new();
        for meta in ISSUE_RESULT_META {
            assert!(
                seen.insert(meta.result_key),
                "duplicate result key {}",
                meta.result_key
            );
        }
    }

    #[test]
    fn counted_result_keys_match_total_issue_fields() {
        let from_total: BTreeSet<&str> = TOTAL_ISSUE_RESULT_KEYS.iter().copied().collect();
        let from_meta: BTreeSet<&str> = counted_result_issue_metas()
            .map(|meta| meta.result_key)
            .collect();
        assert_eq!(from_total, from_meta);
    }

    #[test]
    fn advisory_result_keys_are_explicitly_excluded_from_total() {
        let expected = BTreeSet::from([
            "duplicate_prop_shapes",
            "prop_drilling_chains",
            "thin_wrappers",
        ]);
        let from_meta: BTreeSet<&str> = result_issue_metas()
            .filter(|meta| !meta.counts_in_total)
            .map(|meta| meta.result_key)
            .collect();
        assert_eq!(expected, from_meta);
    }
}
