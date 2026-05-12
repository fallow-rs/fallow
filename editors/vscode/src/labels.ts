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
  | "unresolved-imports"
  | "unlisted-dependencies"
  | "duplicate-exports"
  | "type-only-dependencies"
  | "test-only-dependencies"
  | "circular-dependencies"
  | "boundary-violation"
  | "stale-suppressions"
  | "unused-catalog-entries";

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
  "unresolved-imports": "Unresolved Imports",
  "unlisted-dependencies": "Unlisted Dependencies",
  "duplicate-exports": "Duplicate Exports",
  "type-only-dependencies": "Type-Only Dependencies",
  "test-only-dependencies": "Test-Only Dependencies",
  "circular-dependencies": "Circular Dependencies",
  "boundary-violation": "Boundary Violations",
  "stale-suppressions": "Stale Suppressions",
  "unused-catalog-entries": "Unused Catalog Entries",
};
