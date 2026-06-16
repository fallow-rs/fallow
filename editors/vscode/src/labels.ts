/**
 * Tree-view category labels. The kebab-case `IssueCategory` keys mirror
 * fallow's rule names and the VS Code setting `fallow.issueTypes.*` keys.
 * These are UI strings, not part of the JSON output contract.
 */

export type IssueCategory =
  | "unused-files"
  | "unused-exports"
  | "unused-types"
  | "private-type-leaks"
  | "unused-dependencies"
  | "unused-dev-dependencies"
  | "unused-optional-dependencies"
  | "unused-enum-members"
  | "unused-class-members"
  | "unused-store-member"
  | "unused-server-action"
  | "unused-load-data-keys"
  | "unused-component-prop"
  | "unused-component-emit"
  | "unused-component-input"
  | "unused-component-output"
  | "unrendered-component"
  | "unprovided-inject"
  | "invalid-client-export"
  | "mixed-client-server-barrel"
  | "misplaced-directive"
  | "route-collision"
  | "dynamic-segment-name-conflict"
  | "unresolved-imports"
  | "unlisted-dependencies"
  | "duplicate-exports"
  | "type-only-dependencies"
  | "test-only-dependencies"
  | "circular-dependencies"
  | "re-export-cycles"
  | "boundary-violation"
  | "policy-violations"
  | "stale-suppressions"
  | "unused-catalog-entries"
  | "empty-catalog-groups"
  | "unresolved-catalog-references"
  | "unused-dependency-overrides"
  | "misconfigured-dependency-overrides";

export const ISSUE_CATEGORY_LABELS: Record<IssueCategory, string> = {
  "unused-files": "Unused Files",
  "unused-exports": "Unused Exports",
  "unused-types": "Unused Types",
  "private-type-leaks": "Private Type Leaks",
  "unused-dependencies": "Unused Dependencies",
  "unused-dev-dependencies": "Unused Dev Dependencies",
  "unused-optional-dependencies": "Unused Optional Dependencies",
  "unused-enum-members": "Unused Enum Members",
  "unused-class-members": "Unused Class Members",
  "unused-store-member": "Unused Store Members",
  "unused-server-action": "Unused Server Actions",
  "unused-load-data-keys": "Unused Load Data Keys",
  "unused-component-prop": "Unused Component Props",
  "unused-component-emit": "Unused Component Emits",
  "unused-component-input": "Unused Component Inputs",
  "unused-component-output": "Unused Component Outputs",
  "unrendered-component": "Unrendered Components",
  "unprovided-inject": "Unprovided Injects",
  "invalid-client-export": "Invalid Client Exports",
  "mixed-client-server-barrel": "Mixed Client/Server Barrels",
  "misplaced-directive": "Misplaced Directives",
  "route-collision": "Route Collisions",
  "dynamic-segment-name-conflict": "Dynamic Segment Name Conflicts",
  "unresolved-imports": "Unresolved Imports",
  "unlisted-dependencies": "Unlisted Dependencies",
  "duplicate-exports": "Duplicate Exports",
  "type-only-dependencies": "Type-Only Dependencies",
  "test-only-dependencies": "Test-Only Dependencies",
  "circular-dependencies": "Circular Dependencies",
  "re-export-cycles": "Re-Export Cycles",
  "boundary-violation": "Boundary Violations",
  "policy-violations": "Policy Violations",
  "stale-suppressions": "Stale Suppressions",
  "unused-catalog-entries": "Unused Catalog Entries",
  "empty-catalog-groups": "Empty Catalog Groups",
  "unresolved-catalog-references": "Unresolved Catalog References",
  "unused-dependency-overrides": "Unused Dependency Overrides",
  "misconfigured-dependency-overrides": "Misconfigured Dependency Overrides",
};
