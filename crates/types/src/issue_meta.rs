//! Shared issue-type contract metadata.

use std::sync::LazyLock;

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

    /// Whether this row owns a serialized dead-code result contract.
    #[must_use]
    pub fn has_result_contract(self) -> bool {
        issue_result_meta_by_code(self.code).is_some()
    }

    /// SARIF rule ids used by CI formatters for this issue row.
    #[must_use]
    pub fn sarif_rule_ids(self) -> Vec<String> {
        issue_sarif_rule_ids(self.code)
    }

    /// Whether this issue row is eligible for SARIF rule metadata.
    #[must_use]
    pub fn sarif_enabled(self) -> bool {
        self.has_result_contract()
    }

    /// CodeClimate check names used by CI formatters for this issue row.
    #[must_use]
    pub fn codeclimate_check_names(self) -> Vec<String> {
        issue_codeclimate_check_names(self.code)
    }

    /// Whether this issue row is eligible for CodeClimate output.
    #[must_use]
    pub fn codeclimate_enabled(self) -> bool {
        !self.codeclimate_check_names().is_empty()
    }

    /// Documentation anchor under `/explanations/dead-code`.
    #[must_use]
    pub fn docs_anchor(self) -> Option<&'static str> {
        issue_docs_anchor(self.code)
    }

    /// Published TypeScript backwards-compat alias policy.
    #[must_use]
    pub fn ts_alias(self) -> Option<TsAliasMeta> {
        issue_ts_alias(self.code)
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
        aliases: &["unused-dev-deps", "unused-dev-dependencies"],
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
        aliases: &["unused-optional-deps", "unused-optional-dependencies"],
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
        aliases: &["boundary-calls", "boundary-call-violations"],
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
        kind: Some(IssueKind::StaleSuppression),
        code: "missing-suppression-reason",
        aliases: &["missing-suppression-reasons"],
        label: "Missing Suppression Reasons",
        config_key: Some("require-suppression-reason"),
        filter_flag: Some("--stale-suppressions"),
        mcp_issue_type: None,
        suppress_token: None,
        suppress_file_level: false,
        lsp: false,
        docs_category: "source",
    },
    IssueKindMeta {
        kind: Some(IssueKind::PnpmCatalogEntry),
        code: "unused-catalog-entry",
        aliases: &["catalog", "unused-catalog-entries"],
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
        aliases: &["empty-catalog", "empty-catalog-groups"],
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
        aliases: &["unresolved-catalog", "unresolved-catalog-references"],
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
        aliases: &[
            "unused-dependency-overrides",
            "unused-override",
            "unused-overrides",
        ],
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
        aliases: &[
            "misconfigured-dependency-overrides",
            "misconfigured-override",
            "misconfigured-overrides",
        ],
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
    IssueKindMeta {
        kind: Some(IssueKind::CssTokenDrift),
        code: "css-token-drift",
        aliases: &[],
        label: "CSS Token Drift",
        config_key: Some("css-token-drift"),
        filter_flag: None,
        mcp_issue_type: None,
        suppress_token: Some("css-token-drift"),
        suppress_file_level: false,
        lsp: false,
        docs_category: "health",
    },
    IssueKindMeta {
        kind: Some(IssueKind::CssDuplicateBlock),
        code: "css-duplicate-block",
        aliases: &[],
        label: "CSS Duplicate Block",
        config_key: Some("css-duplicate-block"),
        filter_flag: None,
        mcp_issue_type: None,
        suppress_token: Some("css-duplicate-block"),
        suppress_file_level: false,
        lsp: false,
        docs_category: "health",
    },
    IssueKindMeta {
        kind: Some(IssueKind::CssSelectorComplexity),
        code: "css-selector-complexity",
        aliases: &[],
        label: "CSS Selector Complexity",
        config_key: Some("css-selector-complexity"),
        filter_flag: None,
        mcp_issue_type: None,
        suppress_token: Some("css-selector-complexity"),
        suppress_file_level: false,
        lsp: false,
        docs_category: "health",
    },
    IssueKindMeta {
        kind: Some(IssueKind::CssDeadSurface),
        code: "css-dead-surface",
        aliases: &[],
        label: "CSS Dead Surface",
        config_key: Some("css-dead-surface"),
        filter_flag: None,
        mcp_issue_type: None,
        suppress_token: Some("css-dead-surface"),
        suppress_file_level: false,
        lsp: false,
        docs_category: "health",
    },
    IssueKindMeta {
        kind: Some(IssueKind::CssBrokenReference),
        code: "css-broken-reference",
        aliases: &[],
        label: "CSS Broken Reference",
        config_key: Some("css-broken-reference"),
        filter_flag: None,
        mcp_issue_type: None,
        suppress_token: Some("css-broken-reference"),
        suppress_file_level: false,
        lsp: false,
        docs_category: "health",
    },
];

/// Shared contract facts for serialized `AnalysisResults` arrays.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct IssueResultMeta {
    /// Canonical issue code that owns this result array.
    pub code: &'static str,
    /// Short SARIF rule description.
    pub sarif_description: &'static str,
    /// Explanation emitted in dead-code `_meta.rules`.
    pub meta_description: &'static str,
    /// Documentation path emitted in dead-code `_meta.rules`.
    pub meta_docs_path: &'static str,
    /// Human-readable name emitted in dead-code `_meta.rules`.
    pub meta_name: &'static str,
    /// Label used by CI summary tables.
    pub summary_label: &'static str,
    /// Documentation anchor under `/explanations/dead-code`.
    pub docs_anchor: &'static str,
    /// Serialized `AnalysisResults` array key that carries this issue row.
    pub result_key: &'static str,
    /// Whether `result_key` contributes to `AnalysisResults::total_issues()`.
    pub counts_in_total: bool,
}

/// TypeScript backwards-compat alias emitted for a dead-code result row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TsAliasMeta {
    /// Bare alias name kept available from the published `fallow/types` subpath.
    pub name: &'static str,
    /// Generated `*Finding` wrapper type the alias resolves to.
    pub parent: &'static str,
}

/// TypeScript backwards-compat alias row for a dead-code result code.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IssueTsAliasMeta {
    /// Canonical issue code that owns this alias.
    pub code: &'static str,
    /// Published TypeScript alias policy.
    pub alias: TsAliasMeta,
}

/// Published TypeScript backwards-compat aliases.
pub const ISSUE_TS_ALIAS_META: &[IssueTsAliasMeta] = &[
    IssueTsAliasMeta {
        code: "unused-file",
        alias: TsAliasMeta {
            name: "UnusedFile",
            parent: "UnusedFileFinding",
        },
    },
    IssueTsAliasMeta {
        code: "unused-export",
        alias: TsAliasMeta {
            name: "UnusedExport",
            parent: "UnusedExportFinding",
        },
    },
    IssueTsAliasMeta {
        code: "private-type-leak",
        alias: TsAliasMeta {
            name: "PrivateTypeLeak",
            parent: "PrivateTypeLeakFinding",
        },
    },
    IssueTsAliasMeta {
        code: "unused-dependency",
        alias: TsAliasMeta {
            name: "UnusedDependency",
            parent: "UnusedDependencyFinding",
        },
    },
    IssueTsAliasMeta {
        code: "unused-dev-dependency",
        alias: TsAliasMeta {
            name: "UnusedDependency",
            parent: "UnusedDevDependencyFinding",
        },
    },
    IssueTsAliasMeta {
        code: "unused-optional-dependency",
        alias: TsAliasMeta {
            name: "UnusedDependency",
            parent: "UnusedOptionalDependencyFinding",
        },
    },
    IssueTsAliasMeta {
        code: "unused-enum-member",
        alias: TsAliasMeta {
            name: "UnusedMember",
            parent: "UnusedEnumMemberFinding",
        },
    },
    IssueTsAliasMeta {
        code: "unused-class-member",
        alias: TsAliasMeta {
            name: "UnusedMember",
            parent: "UnusedClassMemberFinding",
        },
    },
    IssueTsAliasMeta {
        code: "unused-store-member",
        alias: TsAliasMeta {
            name: "UnusedMember",
            parent: "UnusedStoreMemberFinding",
        },
    },
    IssueTsAliasMeta {
        code: "unresolved-import",
        alias: TsAliasMeta {
            name: "UnresolvedImport",
            parent: "UnresolvedImportFinding",
        },
    },
    IssueTsAliasMeta {
        code: "unlisted-dependency",
        alias: TsAliasMeta {
            name: "UnlistedDependency",
            parent: "UnlistedDependencyFinding",
        },
    },
    IssueTsAliasMeta {
        code: "duplicate-export",
        alias: TsAliasMeta {
            name: "DuplicateExport",
            parent: "DuplicateExportFinding",
        },
    },
    IssueTsAliasMeta {
        code: "type-only-dependency",
        alias: TsAliasMeta {
            name: "TypeOnlyDependency",
            parent: "TypeOnlyDependencyFinding",
        },
    },
    IssueTsAliasMeta {
        code: "test-only-dependency",
        alias: TsAliasMeta {
            name: "TestOnlyDependency",
            parent: "TestOnlyDependencyFinding",
        },
    },
    IssueTsAliasMeta {
        code: "circular-dependency",
        alias: TsAliasMeta {
            name: "CircularDependency",
            parent: "CircularDependencyFinding",
        },
    },
    IssueTsAliasMeta {
        code: "re-export-cycle",
        alias: TsAliasMeta {
            name: "ReExportCycle",
            parent: "ReExportCycleFinding",
        },
    },
    IssueTsAliasMeta {
        code: "boundary-violation",
        alias: TsAliasMeta {
            name: "BoundaryViolation",
            parent: "BoundaryViolationFinding",
        },
    },
    IssueTsAliasMeta {
        code: "unused-catalog-entry",
        alias: TsAliasMeta {
            name: "UnusedCatalogEntry",
            parent: "UnusedCatalogEntryFinding",
        },
    },
    IssueTsAliasMeta {
        code: "empty-catalog-group",
        alias: TsAliasMeta {
            name: "EmptyCatalogGroup",
            parent: "EmptyCatalogGroupFinding",
        },
    },
    IssueTsAliasMeta {
        code: "unresolved-catalog-reference",
        alias: TsAliasMeta {
            name: "UnresolvedCatalogReference",
            parent: "UnresolvedCatalogReferenceFinding",
        },
    },
    IssueTsAliasMeta {
        code: "unused-dependency-override",
        alias: TsAliasMeta {
            name: "UnusedDependencyOverride",
            parent: "UnusedDependencyOverrideFinding",
        },
    },
    IssueTsAliasMeta {
        code: "misconfigured-dependency-override",
        alias: TsAliasMeta {
            name: "MisconfiguredDependencyOverride",
            parent: "MisconfiguredDependencyOverrideFinding",
        },
    },
];

/// All shared issue-to-result metadata rows.
pub const ISSUE_RESULT_META: &[IssueResultMeta] = &[
    IssueResultMeta {
        code: "unused-file",
        sarif_description: "File is not reachable from any entry point",
        meta_description: "Source files that are not imported by any other module and are not entry points. Detection uses graph reachability from configured entry points.",
        meta_docs_path: "explanations/dead-code#unused-files",
        meta_name: "Unused Files",
        summary_label: "Unused files",
        docs_anchor: "unused-files",
        result_key: "unused_files",
        counts_in_total: true,
    },
    IssueResultMeta {
        code: "unused-export",
        sarif_description: "Export is never imported",
        meta_description: "Named exports that are never imported by any other module in the project, including direct exports and re-exports through barrel files.",
        meta_docs_path: "explanations/dead-code#unused-exports",
        meta_name: "Unused Exports",
        summary_label: "Unused exports",
        docs_anchor: "unused-exports",
        result_key: "unused_exports",
        counts_in_total: true,
    },
    IssueResultMeta {
        code: "unused-type",
        sarif_description: "Type export is never imported",
        meta_description: "Type-only exports that are never imported. These do not generate runtime code but add maintenance burden.",
        meta_docs_path: "explanations/dead-code#unused-types",
        meta_name: "Unused Type Exports",
        summary_label: "Unused types",
        docs_anchor: "unused-types",
        result_key: "unused_types",
        counts_in_total: true,
    },
    IssueResultMeta {
        code: "private-type-leak",
        sarif_description: "Exported signature references a private type",
        meta_description: "Exported values or types whose public TypeScript signature references a same-file type declaration that is not exported.",
        meta_docs_path: "explanations/dead-code#private-type-leaks",
        meta_name: "Private Type Leaks",
        summary_label: "Private type leaks",
        docs_anchor: "private-type-leaks",
        result_key: "private_type_leaks",
        counts_in_total: true,
    },
    IssueResultMeta {
        code: "unused-dependency",
        sarif_description: "Dependency listed but never imported",
        meta_description: "Packages listed in dependencies that are never imported or required by any source file.",
        meta_docs_path: "explanations/dead-code#unused-dependencies",
        meta_name: "Unused Dependencies",
        summary_label: "Unused dependencies",
        docs_anchor: "unused-dependencies",
        result_key: "unused_dependencies",
        counts_in_total: true,
    },
    IssueResultMeta {
        code: "unused-dev-dependency",
        sarif_description: "Dev dependency listed but never imported",
        meta_description: "Packages listed in devDependencies that are never imported by test files, config files, or scripts.",
        meta_docs_path: "explanations/dead-code#unused-devdependencies",
        meta_name: "Unused Dev Dependencies",
        summary_label: "Unused devDependencies",
        docs_anchor: "unused-dependencies",
        result_key: "unused_dev_dependencies",
        counts_in_total: true,
    },
    IssueResultMeta {
        code: "unused-optional-dependency",
        sarif_description: "Optional dependency listed but never imported",
        meta_description: "Packages listed in optionalDependencies that are never imported.",
        meta_docs_path: "explanations/dead-code#unused-optionaldependencies",
        meta_name: "Unused Optional Dependencies",
        summary_label: "Unused optionalDependencies",
        docs_anchor: "unused-dependencies",
        result_key: "unused_optional_dependencies",
        counts_in_total: true,
    },
    IssueResultMeta {
        code: "unused-enum-member",
        sarif_description: "Enum member is never referenced",
        meta_description: "Enum members that are never referenced in the codebase.",
        meta_docs_path: "explanations/dead-code#unused-enum-members",
        meta_name: "Unused Enum Members",
        summary_label: "Unused enum members",
        docs_anchor: "unused-enum-members",
        result_key: "unused_enum_members",
        counts_in_total: true,
    },
    IssueResultMeta {
        code: "unused-class-member",
        sarif_description: "Class member is never referenced",
        meta_description: "Class methods and properties that are never referenced outside the class.",
        meta_docs_path: "explanations/dead-code#unused-class-members",
        meta_name: "Unused Class Members",
        summary_label: "Unused class members",
        docs_anchor: "unused-class-members",
        result_key: "unused_class_members",
        counts_in_total: true,
    },
    IssueResultMeta {
        code: "unused-store-member",
        sarif_description: "Store member is never accessed by any consumer",
        meta_description: "Pinia store members declared but never accessed by any consumer project-wide.",
        meta_docs_path: "explanations/dead-code#unused-store-members",
        meta_name: "Unused Store Members",
        summary_label: "Unused store members",
        docs_anchor: "unused-store-members",
        result_key: "unused_store_members",
        counts_in_total: true,
    },
    IssueResultMeta {
        code: "unresolved-import",
        sarif_description: "Import could not be resolved",
        meta_description: "Import specifiers that could not be resolved to a file on disk.",
        meta_docs_path: "explanations/dead-code#unresolved-imports",
        meta_name: "Unresolved Imports",
        summary_label: "Unresolved imports",
        docs_anchor: "unresolved-imports",
        result_key: "unresolved_imports",
        counts_in_total: true,
    },
    IssueResultMeta {
        code: "unlisted-dependency",
        sarif_description: "Dependency used but not in package.json",
        meta_description: "Packages imported in source code but not listed in package.json.",
        meta_docs_path: "explanations/dead-code#unlisted-dependencies",
        meta_name: "Unlisted Dependencies",
        summary_label: "Unlisted dependencies",
        docs_anchor: "unlisted-dependencies",
        result_key: "unlisted_dependencies",
        counts_in_total: true,
    },
    IssueResultMeta {
        code: "duplicate-export",
        sarif_description: "Export name appears in multiple modules",
        meta_description: "The same export name is defined in multiple modules.",
        meta_docs_path: "explanations/dead-code#duplicate-exports",
        meta_name: "Duplicate Exports",
        summary_label: "Duplicate exports",
        docs_anchor: "duplicate-exports",
        result_key: "duplicate_exports",
        counts_in_total: true,
    },
    IssueResultMeta {
        code: "type-only-dependency",
        sarif_description: "Production dependency only used via type-only imports",
        meta_description: "Production dependencies that are only imported via type-only imports.",
        meta_docs_path: "explanations/dead-code#type-only-dependencies",
        meta_name: "Type-only Dependencies",
        summary_label: "Type-only dependencies",
        docs_anchor: "type-only-dependencies",
        result_key: "type_only_dependencies",
        counts_in_total: true,
    },
    IssueResultMeta {
        code: "test-only-dependency",
        sarif_description: "Production dependency only imported by test files",
        meta_description: "Production dependencies that are only imported from test files.",
        meta_docs_path: "explanations/dead-code#test-only-dependencies",
        meta_name: "Test-only Dependencies",
        summary_label: "Test-only dependencies",
        docs_anchor: "test-only-dependencies",
        result_key: "test_only_dependencies",
        counts_in_total: true,
    },
    IssueResultMeta {
        code: "circular-dependency",
        sarif_description: "Circular dependency chain detected",
        meta_description: "A cycle in the module import graph.",
        meta_docs_path: "explanations/dead-code#circular-dependencies",
        meta_name: "Circular Dependencies",
        summary_label: "Circular dependencies",
        docs_anchor: "circular-dependencies",
        result_key: "circular_dependencies",
        counts_in_total: true,
    },
    IssueResultMeta {
        code: "re-export-cycle",
        sarif_description: "Two or more barrel files re-export from each other in a loop",
        meta_description: "A barrel file re-exports from another barrel that ultimately re-exports back.",
        meta_docs_path: "explanations/dead-code#re-export-cycles",
        meta_name: "Re-Export Cycles",
        summary_label: "Re-export cycles",
        docs_anchor: "re-export-cycles",
        result_key: "re_export_cycles",
        counts_in_total: true,
    },
    IssueResultMeta {
        code: "boundary-violation",
        sarif_description: "Import crosses a configured architecture boundary",
        meta_description: "A module imports from a zone that its configured boundary rules do not allow.",
        meta_docs_path: "explanations/dead-code#boundary-violations",
        meta_name: "Boundary Violations",
        summary_label: "Boundary violations",
        docs_anchor: "boundary-violations",
        result_key: "boundary_violations",
        counts_in_total: true,
    },
    IssueResultMeta {
        code: "boundary-coverage",
        sarif_description: "Source file matches no configured architecture boundary zone",
        meta_description: "A reachable source file is not assigned to any configured boundary zone while boundary coverage is required.",
        meta_docs_path: "explanations/dead-code#boundary-violations",
        meta_name: "Boundary Coverage",
        summary_label: "Boundary coverage",
        docs_anchor: "boundary-violations",
        result_key: "boundary_coverage_violations",
        counts_in_total: true,
    },
    IssueResultMeta {
        code: "boundary-call-violation",
        sarif_description: "Zoned file calls a callee its zone forbids",
        meta_description: "A file classified into a boundary zone calls a callee matching one of the zone's forbidden call patterns.",
        meta_docs_path: "explanations/dead-code#boundary-violations",
        meta_name: "Boundary Call Violation",
        summary_label: "Boundary calls",
        docs_anchor: "boundary-violations",
        result_key: "boundary_call_violations",
        counts_in_total: true,
    },
    IssueResultMeta {
        code: "policy-violation",
        sarif_description: "Banned usage matched a rule-pack rule",
        meta_description: "A call site, import, or catalogue-derived effect matched a configured rule pack rule.",
        meta_docs_path: "explanations/dead-code#policy-violations",
        meta_name: "Policy Violation",
        summary_label: "Policy violations",
        docs_anchor: "policy-violations",
        result_key: "policy_violations",
        counts_in_total: true,
    },
    IssueResultMeta {
        code: "invalid-client-export",
        sarif_description: "\"use client\" file exports a server-only / route-config name",
        meta_description: "A file carrying the use client directive also exports a Next.js server-only or route-segment config name.",
        meta_docs_path: "explanations/dead-code#invalid-client-exports",
        meta_name: "Invalid client export",
        summary_label: "Invalid client exports",
        docs_anchor: "invalid-client-exports",
        result_key: "invalid_client_exports",
        counts_in_total: true,
    },
    IssueResultMeta {
        code: "mixed-client-server-barrel",
        sarif_description: "Barrel re-exports both a \"use client\" module and a server-only module",
        meta_description: "A barrel file forwards a name from a use client module alongside a name from a server-only module.",
        meta_docs_path: "explanations/dead-code#mixed-client-server-barrels",
        meta_name: "Mixed client/server barrel",
        summary_label: "Mixed client/server barrels",
        docs_anchor: "mixed-client-server-barrels",
        result_key: "mixed_client_server_barrels",
        counts_in_total: true,
    },
    IssueResultMeta {
        code: "misplaced-directive",
        sarif_description: "\"use client\" / \"use server\" directive is not in the leading position and is ignored",
        meta_description: "A use client or use server directive string appears after a non-directive statement and is ignored.",
        meta_docs_path: "explanations/dead-code#misplaced-directives",
        meta_name: "Misplaced directive",
        summary_label: "Misplaced directives",
        docs_anchor: "misplaced-directives",
        result_key: "misplaced_directives",
        counts_in_total: true,
    },
    IssueResultMeta {
        code: "unprovided-inject",
        sarif_description: "inject() / getContext() reads a key that no provide() / setContext() supplies",
        meta_description: "A Vue inject or Svelte getContext reads a dependency-injection key that no matching provider supplies.",
        meta_docs_path: "explanations/dead-code#unprovided-injects",
        meta_name: "Unprovided injects",
        summary_label: "Unprovided injects",
        docs_anchor: "unprovided-inject",
        result_key: "unprovided_injects",
        counts_in_total: true,
    },
    IssueResultMeta {
        code: "unrendered-component",
        sarif_description: "A Vue / Svelte component is reachable through a barrel but rendered nowhere",
        meta_description: "A Vue or Svelte single-file component is reachable through the graph but rendered nowhere in the project.",
        meta_docs_path: "explanations/dead-code#unrendered-components",
        meta_name: "Unrendered components",
        summary_label: "Unrendered components",
        docs_anchor: "unrendered-component",
        result_key: "unrendered_components",
        counts_in_total: true,
    },
    IssueResultMeta {
        code: "unused-component-prop",
        sarif_description: "A Vue, Svelte, or React component prop is referenced nowhere in its own component",
        meta_description: "A declared Vue, Svelte, React, or Preact component prop is referenced nowhere inside its own component.",
        meta_docs_path: "explanations/dead-code#unused-component-props",
        meta_name: "Unused component props",
        summary_label: "Unused component props",
        docs_anchor: "unused-component-prop",
        result_key: "unused_component_props",
        counts_in_total: true,
    },
    IssueResultMeta {
        code: "unused-component-emit",
        sarif_description: "A Vue <script setup> defineEmits event is emitted nowhere in its own component",
        meta_description: "A Vue script setup defineEmits event is emitted nowhere in its own component.",
        meta_docs_path: "explanations/dead-code#unused-component-emits",
        meta_name: "Unused component emits",
        summary_label: "Unused component emits",
        docs_anchor: "unused-component-emit",
        result_key: "unused_component_emits",
        counts_in_total: true,
    },
    IssueResultMeta {
        code: "unused-component-input",
        sarif_description: "An Angular @Input() / signal input() / model() input is read nowhere in its own component",
        meta_description: "An Angular input is read nowhere in its own component.",
        meta_docs_path: "explanations/dead-code#unused-component-inputs",
        meta_name: "Unused component inputs",
        summary_label: "Unused component inputs",
        docs_anchor: "unused-component-input",
        result_key: "unused_component_inputs",
        counts_in_total: true,
    },
    IssueResultMeta {
        code: "unused-component-output",
        sarif_description: "An Angular @Output() / signal output() output is emitted nowhere in its own component",
        meta_description: "An Angular output is emitted nowhere in its own component.",
        meta_docs_path: "explanations/dead-code#unused-component-outputs",
        meta_name: "Unused component outputs",
        summary_label: "Unused component outputs",
        docs_anchor: "unused-component-output",
        result_key: "unused_component_outputs",
        counts_in_total: true,
    },
    IssueResultMeta {
        code: "unused-svelte-event",
        sarif_description: "A Svelte component dispatches a createEventDispatcher event whose name is listened to nowhere in the project",
        meta_description: "A Svelte component dispatches a custom event whose name is listened to nowhere in the analyzed project.",
        meta_docs_path: "explanations/dead-code#unused-svelte-events",
        meta_name: "Unused Svelte events",
        summary_label: "Unused Svelte events",
        docs_anchor: "unused-svelte-event",
        result_key: "unused_svelte_events",
        counts_in_total: true,
    },
    IssueResultMeta {
        code: "unused-server-action",
        sarif_description: "A Next.js Server Action exported from a \"use server\" file is referenced by no code in the project",
        meta_description: "A Next.js Server Action exported from a use server file is referenced by no code in the project.",
        meta_docs_path: "explanations/dead-code#unused-server-actions",
        meta_name: "Unused server actions",
        summary_label: "Unused server actions",
        docs_anchor: "unused-server-action",
        result_key: "unused_server_actions",
        counts_in_total: true,
    },
    IssueResultMeta {
        code: "unused-load-data-key",
        sarif_description: "A SvelteKit load() return-object key is read by no consumer",
        meta_description: "A SvelteKit load return-object key is read by no route or project-wide consumer.",
        meta_docs_path: "explanations/dead-code#unused-load-data-keys",
        meta_name: "Unused load data keys",
        summary_label: "Unused load data keys",
        docs_anchor: "unused-load-data-key",
        result_key: "unused_load_data_keys",
        counts_in_total: true,
    },
    IssueResultMeta {
        code: "route-collision",
        sarif_description: "Two or more Next.js App Router route files resolve to the same URL",
        meta_description: "Two or more Next.js App Router route files resolve to the same URL within one app root.",
        meta_docs_path: "explanations/dead-code#route-collisions",
        meta_name: "Route collision",
        summary_label: "Route collisions",
        docs_anchor: "route-collisions",
        result_key: "route_collisions",
        counts_in_total: true,
    },
    IssueResultMeta {
        code: "dynamic-segment-name-conflict",
        sarif_description: "Sibling Next.js dynamic route segments use different slug names at the same position",
        meta_description: "Sibling Next.js dynamic route segments use different slug names at the same position.",
        meta_docs_path: "explanations/dead-code#dynamic-segment-name-conflicts",
        meta_name: "Dynamic segment name conflict",
        summary_label: "Dynamic segment conflicts",
        docs_anchor: "dynamic-segment-name-conflicts",
        result_key: "dynamic_segment_name_conflicts",
        counts_in_total: true,
    },
    IssueResultMeta {
        code: "stale-suppression",
        sarif_description: "Suppression comment or tag no longer matches any issue",
        meta_description: "A fallow suppression comment or tag no longer matches any active issue.",
        meta_docs_path: "explanations/dead-code#stale-suppressions",
        meta_name: "Stale Suppressions",
        summary_label: "Stale suppressions",
        docs_anchor: "stale-suppressions",
        result_key: "stale_suppressions",
        counts_in_total: true,
    },
    IssueResultMeta {
        code: "unused-catalog-entry",
        sarif_description: "Catalog entry not referenced by any workspace package",
        meta_description: "A package manager catalog entry is not referenced by any workspace package.json.",
        meta_docs_path: "explanations/dead-code#unused-catalog-entries",
        meta_name: "Unused catalog entry",
        summary_label: "Unused catalog entries",
        docs_anchor: "unused-catalog-entries",
        result_key: "unused_catalog_entries",
        counts_in_total: true,
    },
    IssueResultMeta {
        code: "empty-catalog-group",
        sarif_description: "Named catalog group has no entries",
        meta_description: "A named package manager catalog group has no package entries.",
        meta_docs_path: "explanations/dead-code#empty-catalog-groups",
        meta_name: "Empty catalog group",
        summary_label: "Empty catalog groups",
        docs_anchor: "empty-catalog-groups",
        result_key: "empty_catalog_groups",
        counts_in_total: true,
    },
    IssueResultMeta {
        code: "unresolved-catalog-reference",
        sarif_description: "package.json references a catalog that does not declare the package",
        meta_description: "A workspace package.json uses a catalog protocol reference that no catalog declares.",
        meta_docs_path: "explanations/dead-code#unresolved-catalog-references",
        meta_name: "Unresolved catalog reference",
        summary_label: "Unresolved catalog references",
        docs_anchor: "unresolved-catalog-references",
        result_key: "unresolved_catalog_references",
        counts_in_total: true,
    },
    IssueResultMeta {
        code: "unused-dependency-override",
        sarif_description: "pnpm.overrides entry targets a package not declared or resolved",
        meta_description: "A pnpm dependency override targets a package not declared by any workspace package and not present in the lockfile.",
        meta_docs_path: "explanations/dead-code#unused-dependency-overrides",
        meta_name: "Unused pnpm dependency override",
        summary_label: "Unused dependency overrides",
        docs_anchor: "unused-dependency-overrides",
        result_key: "unused_dependency_overrides",
        counts_in_total: true,
    },
    IssueResultMeta {
        code: "misconfigured-dependency-override",
        sarif_description: "pnpm.overrides entry has an unparsable key or value",
        meta_description: "A pnpm dependency override key or value does not parse as a valid override spec.",
        meta_docs_path: "explanations/dead-code#misconfigured-dependency-overrides",
        meta_name: "Misconfigured pnpm dependency override",
        summary_label: "Misconfigured dependency overrides",
        docs_anchor: "misconfigured-dependency-overrides",
        result_key: "misconfigured_dependency_overrides",
        counts_in_total: true,
    },
    IssueResultMeta {
        code: "prop-drilling",
        sarif_description: "A React/Preact prop is forwarded unchanged through 3+ pass-through components to a distant consumer",
        meta_description: "A React or Preact prop is forwarded unchanged through multiple pass-through components to a distant consumer.",
        meta_docs_path: "explanations/dead-code#prop-drilling",
        meta_name: "Prop drilling",
        summary_label: "Prop drilling",
        docs_anchor: "prop-drilling",
        result_key: "prop_drilling_chains",
        counts_in_total: false,
    },
    IssueResultMeta {
        code: "thin-wrapper",
        sarif_description: "A React/Preact component whose whole body is a single spread-forwarded child render (a candidate for inlining)",
        meta_description: "A React or Preact component is structural indirection around a single spread-forwarded child render.",
        meta_docs_path: "explanations/dead-code#thin-wrapper",
        meta_name: "Thin wrapper",
        summary_label: "Thin wrappers",
        docs_anchor: "thin-wrapper",
        result_key: "thin_wrappers",
        counts_in_total: false,
    },
    IssueResultMeta {
        code: "duplicate-prop-shape",
        sarif_description: "Three or more React/Preact components across two or more files declare an identical prop-name set (a missing shared Props type)",
        meta_description: "Multiple React or Preact components declare an identical significant prop-name set.",
        meta_docs_path: "explanations/dead-code#duplicate-prop-shape",
        meta_name: "Duplicate prop shape",
        summary_label: "Duplicate prop shapes",
        docs_anchor: "duplicate-prop-shape",
        result_key: "duplicate_prop_shapes",
        counts_in_total: false,
    },
];

/// Canonical names and aliases accepted by `IssueKind::parse`.
pub static KNOWN_ISSUE_KIND_NAMES: LazyLock<Vec<&'static str>> =
    LazyLock::new(known_issue_kind_names_from_meta);

/// CLI filter flags on `fallow dead-code` that scope output to one issue family.
pub static DEAD_CODE_FILTER_FLAGS: LazyLock<Vec<&'static str>> =
    LazyLock::new(dead_code_filter_flags_from_meta);

/// MCP issue selector names mapped to dead-code CLI flags.
pub static MCP_ISSUE_TYPE_FLAGS: LazyLock<Vec<(&'static str, &'static str)>> =
    LazyLock::new(mcp_issue_type_flags_from_meta);

/// Result issue codes emitted by the dead-code CodeClimate formatter.
pub static CODECLIMATE_RESULT_CODES: LazyLock<Vec<&'static str>> =
    LazyLock::new(codeclimate_result_codes_from_meta);

fn known_issue_kind_names_from_meta() -> Vec<&'static str> {
    let mut names = Vec::new();
    for meta in ISSUE_KIND_META.iter().filter(|meta| meta.kind.is_some()) {
        push_unique(&mut names, meta.code);
        for alias in meta.aliases {
            push_unique(&mut names, *alias);
        }
    }
    names
}

fn dead_code_filter_flags_from_meta() -> Vec<&'static str> {
    let mut flags = Vec::new();
    for meta in ISSUE_KIND_META {
        if let Some(flag) = meta.filter_flag {
            push_unique(&mut flags, flag);
        }
    }
    flags
}

fn mcp_issue_type_flags_from_meta() -> Vec<(&'static str, &'static str)> {
    ISSUE_KIND_META
        .iter()
        .filter_map(|meta| meta.mcp_pair())
        .collect()
}

fn codeclimate_result_codes_from_meta() -> Vec<&'static str> {
    ISSUE_RESULT_META
        .iter()
        .filter(|meta| meta.counts_in_total)
        .map(|meta| meta.code)
        .collect()
}

fn push_unique<T: Copy + PartialEq>(items: &mut Vec<T>, item: T) {
    if !items.contains(&item) {
        items.push(item);
    }
}

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

/// Lookup metadata by any shared contract token: canonical code, alias,
/// config key, MCP selector, suppression token, or CLI filter flag.
#[must_use]
pub fn issue_meta_for_contract_token(token: &str) -> Option<&'static IssueKindMeta> {
    let normalized = token.trim().trim_start_matches("--").replace('_', "-");
    ISSUE_KIND_META
        .iter()
        .find(|meta| issue_meta_matches_contract_token(meta, &normalized))
}

/// Whether a metadata row owns the provided shared contract token.
#[must_use]
pub fn issue_meta_matches_contract_token(meta: &IssueKindMeta, token: &str) -> bool {
    let normalized = token.trim().trim_start_matches("--").replace('_', "-");
    meta.code == normalized
        || meta.aliases.contains(&normalized.as_str())
        || meta.config_key == Some(normalized.as_str())
        || meta.mcp_issue_type == Some(normalized.as_str())
        || meta.suppress_token == Some(normalized.as_str())
        || meta.filter_flag.map(|flag| flag.trim_start_matches("--")) == Some(normalized.as_str())
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

/// SARIF rule ids used by CI formatters for a canonical issue code.
#[must_use]
pub fn issue_sarif_rule_ids(code: &str) -> Vec<String> {
    let mut ids = vec![format!("fallow/{code}")];
    if code == "stale-suppression" {
        ids.push("fallow/missing-suppression-reason".to_string());
    }
    ids
}

/// Short SARIF rule description for a rule id emitted by dead-code output.
#[must_use]
pub fn issue_sarif_rule_description(rule_id: &str) -> Option<&'static str> {
    let code = rule_id.strip_prefix("fallow/")?;
    if code == "missing-suppression-reason" {
        return Some("Suppression comment omits a required reason");
    }
    issue_result_meta_by_code(code).map(|meta| meta.sarif_description)
}

/// CodeClimate check names used by CI formatters for a canonical issue code.
#[must_use]
pub fn issue_codeclimate_check_names(code: &str) -> Vec<String> {
    if !CODECLIMATE_RESULT_CODES.contains(&code) {
        return Vec::new();
    }
    issue_sarif_rule_ids(code)
}

/// Documentation anchor under `/explanations/dead-code` for a canonical issue
/// code.
#[must_use]
pub fn issue_docs_anchor(code: &str) -> Option<&'static str> {
    issue_result_meta_by_code(code).map(|meta| meta.docs_anchor)
}

/// Published TypeScript alias policy for backwards-compatible bare names.
#[must_use]
pub fn issue_ts_alias(code: &str) -> Option<TsAliasMeta> {
    ISSUE_TS_ALIAS_META
        .iter()
        .find(|meta| meta.code == code)
        .map(|meta| meta.alias)
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
        for name in KNOWN_ISSUE_KIND_NAMES.iter() {
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
        for &kind in IssueKind::ALL {
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
    fn contract_tokens_resolve_through_metadata() {
        for meta in ISSUE_KIND_META {
            assert_eq!(
                issue_meta_for_contract_token(meta.code).map(|resolved| resolved.code),
                Some(meta.code),
                "canonical issue code {} should resolve through contract token lookup",
                meta.code
            );
            for token in meta.aliases {
                assert!(
                    issue_meta_matches_contract_token(meta, token),
                    "alias {token} should match {}",
                    meta.code
                );
            }
            for token in [
                meta.config_key,
                meta.mcp_issue_type,
                meta.suppress_token,
                meta.filter_flag,
            ]
            .into_iter()
            .flatten()
            {
                assert!(
                    issue_meta_for_contract_token(token).is_some(),
                    "contract token {token} should resolve through metadata"
                );
                assert!(
                    issue_meta_matches_contract_token(meta, token),
                    "contract token {token} should match {}",
                    meta.code
                );
            }
        }
    }

    #[test]
    fn sarif_rule_descriptions_cover_result_metadata() {
        for meta in result_issue_metas() {
            for rule_id in issue_sarif_rule_ids(meta.code) {
                assert!(
                    issue_sarif_rule_description(&rule_id).is_some(),
                    "SARIF rule {rule_id} for {} needs a central description",
                    meta.code
                );
            }
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
    fn result_meta_codes_have_docs_anchors() {
        for meta in ISSUE_RESULT_META {
            let issue = issue_meta_by_code(meta.code)
                .unwrap_or_else(|| panic!("result metadata code {} has no issue row", meta.code));
            assert_eq!(
                issue.docs_anchor(),
                Some(meta.docs_anchor),
                "result metadata code {} has mismatched docs anchor",
                meta.code
            );
        }
    }

    #[test]
    fn result_meta_codes_have_summary_labels() {
        for meta in ISSUE_RESULT_META {
            assert!(
                !meta.summary_label.is_empty(),
                "result metadata code {} has no summary label",
                meta.code
            );
        }
    }

    #[test]
    fn result_meta_codes_have_meta_names() {
        for meta in ISSUE_RESULT_META {
            assert!(
                !meta.meta_name.is_empty(),
                "result metadata code {} has no meta name",
                meta.code
            );
        }
    }

    #[test]
    fn result_meta_codes_have_meta_docs_paths() {
        for meta in ISSUE_RESULT_META {
            assert!(
                meta.meta_docs_path.starts_with("explanations/dead-code#"),
                "result metadata code {} has invalid meta docs path",
                meta.code
            );
        }
    }

    #[test]
    fn result_meta_codes_have_meta_descriptions() {
        for meta in ISSUE_RESULT_META {
            assert!(
                !meta.meta_description.is_empty(),
                "result metadata code {} has no meta description",
                meta.code
            );
        }
    }

    #[test]
    fn ci_format_ids_are_prefixed_and_known() {
        let result_codes: BTreeSet<&str> = result_issue_metas().map(|meta| meta.code).collect();
        let codeclimate_codes: BTreeSet<&str> = CODECLIMATE_RESULT_CODES.iter().copied().collect();
        assert!(codeclimate_codes.is_subset(&result_codes));

        for meta in result_issue_metas() {
            let issue = issue_meta_by_code(meta.code)
                .unwrap_or_else(|| panic!("result metadata code {} has no issue row", meta.code));
            assert!(issue.sarif_enabled());
            let sarif_ids = issue.sarif_rule_ids();
            assert!(sarif_ids.contains(&format!("fallow/{}", meta.code)));
            for rule_id in sarif_ids {
                assert!(
                    rule_id.starts_with("fallow/"),
                    "result metadata code {} has unprefixed SARIF rule id {rule_id}",
                    meta.code
                );
            }
            for check_name in issue.codeclimate_check_names() {
                assert!(
                    check_name.starts_with("fallow/"),
                    "result metadata code {} has unprefixed CodeClimate check name {check_name}",
                    meta.code
                );
            }
        }
    }

    #[test]
    fn ci_summary_check_tables_match_result_metadata() {
        assert_summary_check_table_matches_result_metadata(
            include_str!("../../../action/jq/summary-check.jq"),
            "action/jq/summary-check.jq",
        );
        assert_summary_check_table_matches_result_metadata(
            include_str!("../../../ci/jq/summary-check.jq"),
            "ci/jq/summary-check.jq",
        );
    }

    #[test]
    fn ci_summary_combined_tables_match_result_metadata() {
        assert_summary_combined_table_matches_result_metadata(
            include_str!("../../../action/jq/summary-combined.jq"),
            "action/jq/summary-combined.jq",
        );
        assert_summary_combined_table_matches_result_metadata(
            include_str!("../../../ci/jq/summary-combined.jq"),
            "ci/jq/summary-combined.jq",
        );
    }

    fn assert_summary_check_table_matches_result_metadata(source: &str, path: &str) {
        for meta in counted_result_issue_metas() {
            let expected = format!(
                r#"table_row("{}"; "{}"; "{}")"#,
                meta.summary_label, meta.result_key, meta.docs_anchor
            );
            assert!(
                source.contains(&expected),
                "{path} must include registry row for {} as `{expected}`",
                meta.code
            );
        }
    }

    fn assert_summary_combined_table_matches_result_metadata(source: &str, path: &str) {
        for meta in counted_result_issue_metas() {
            let row = source
                .lines()
                .find(|line| line.contains(&format!(".check.{}", meta.result_key)))
                .unwrap_or_else(|| {
                    panic!(
                        "{path} must include combined summary row for {} ({})",
                        meta.code, meta.result_key
                    )
                });
            assert!(
                row.contains(&format!("[{}]", meta.summary_label)),
                "{path} row for {} must use registry label `{}`: {row}",
                meta.code,
                meta.summary_label
            );
            assert!(
                row.contains(&format!(r#"docs("{}")"#, meta.docs_anchor)),
                "{path} row for {} must use registry docs anchor `{}`: {row}",
                meta.code,
                meta.docs_anchor
            );
        }
    }

    #[test]
    fn ts_alias_policy_is_explicit() {
        let aliases: BTreeSet<(&str, &str)> = ISSUE_TS_ALIAS_META
            .iter()
            .map(|meta| (meta.alias.name, meta.alias.parent))
            .collect();

        assert_eq!(
            BTreeSet::from([
                ("BoundaryViolation", "BoundaryViolationFinding"),
                ("CircularDependency", "CircularDependencyFinding"),
                ("DuplicateExport", "DuplicateExportFinding"),
                ("EmptyCatalogGroup", "EmptyCatalogGroupFinding"),
                (
                    "MisconfiguredDependencyOverride",
                    "MisconfiguredDependencyOverrideFinding",
                ),
                ("PrivateTypeLeak", "PrivateTypeLeakFinding"),
                ("ReExportCycle", "ReExportCycleFinding"),
                ("TestOnlyDependency", "TestOnlyDependencyFinding"),
                ("TypeOnlyDependency", "TypeOnlyDependencyFinding"),
                (
                    "UnresolvedCatalogReference",
                    "UnresolvedCatalogReferenceFinding",
                ),
                ("UnresolvedImport", "UnresolvedImportFinding"),
                ("UnlistedDependency", "UnlistedDependencyFinding"),
                ("UnusedCatalogEntry", "UnusedCatalogEntryFinding"),
                ("UnusedDependency", "UnusedDependencyFinding"),
                ("UnusedDependency", "UnusedDevDependencyFinding"),
                ("UnusedDependency", "UnusedOptionalDependencyFinding"),
                (
                    "UnusedDependencyOverride",
                    "UnusedDependencyOverrideFinding",
                ),
                ("UnusedExport", "UnusedExportFinding"),
                ("UnusedFile", "UnusedFileFinding"),
                ("UnusedMember", "UnusedClassMemberFinding"),
                ("UnusedMember", "UnusedEnumMemberFinding"),
                ("UnusedMember", "UnusedStoreMemberFinding"),
            ]),
            aliases
        );
    }

    #[test]
    fn ts_alias_registry_rows_match_result_metadata() {
        let result_codes: BTreeSet<&str> = result_issue_metas().map(|meta| meta.code).collect();
        let mut seen_codes = BTreeSet::new();
        for meta in ISSUE_TS_ALIAS_META {
            assert!(
                seen_codes.insert(meta.code),
                "duplicate TypeScript alias row for {}",
                meta.code
            );
            assert!(
                result_codes.contains(meta.code),
                "TypeScript alias row {} has no result metadata",
                meta.code
            );
            assert!(
                meta.alias.parent.ends_with("Finding"),
                "TypeScript alias row {} must point at a generated Finding wrapper",
                meta.code
            );
            assert_eq!(
                Some(meta.alias),
                issue_meta_by_code(meta.code).and_then(|issue| issue.ts_alias()),
                "IssueKindMeta helper must resolve TypeScript alias row {} through the registry",
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
