//! TypeScript alias names of the issue types, for backwards-compatible bare
//! names in the published contracts.

use super::{IssueTsAliasMeta, TsAliasMeta};

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
        code: "deprecated-export-in-use",
        alias: TsAliasMeta {
            name: "DeprecatedExportInUse",
            parent: "DeprecatedExportInUseFinding",
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
        code: "unused-protected-class-member",
        alias: TsAliasMeta {
            name: "UnusedMember",
            parent: "UnusedProtectedClassMemberFinding",
        },
    },
    IssueTsAliasMeta {
        code: "namespace-escape",
        alias: TsAliasMeta {
            name: "NamespaceEscape",
            parent: "NamespaceEscapeFinding",
        },
    },
    IssueTsAliasMeta {
        code: "test-only-export",
        alias: TsAliasMeta {
            name: "TestOnlyExport",
            parent: "TestOnlyExportFinding",
        },
    },
    IssueTsAliasMeta {
        code: "unused-namespace-member",
        alias: TsAliasMeta {
            name: "UnusedMember",
            parent: "UnusedNamespaceMemberFinding",
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
        code: "unlisted-binary",
        alias: TsAliasMeta {
            name: "UnlistedBinary",
            parent: "UnlistedBinaryFinding",
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
        code: "duplicate-export-alias",
        alias: TsAliasMeta {
            name: "DuplicateExportAlias",
            parent: "DuplicateExportAliasFinding",
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
        code: "dev-dependency-in-production",
        alias: TsAliasMeta {
            name: "DevDependencyInProduction",
            parent: "DevDependencyInProductionFinding",
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
        code: "package-cycle",
        alias: TsAliasMeta {
            name: "PackageCycle",
            parent: "PackageCycleFinding",
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
