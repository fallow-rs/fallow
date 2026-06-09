import type { FallowCheckResult } from "../src/types.js";

const issueCollectionKeys = [
  "unused_files",
  "unused_exports",
  "unused_types",
  "private_type_leaks",
  "unused_dependencies",
  "unused_dev_dependencies",
  "unused_optional_dependencies",
  "unused_enum_members",
  "unused_class_members",
  "unresolved_imports",
  "unlisted_dependencies",
  "duplicate_exports",
  "type_only_dependencies",
  "test_only_dependencies",
  "circular_dependencies",
  "boundary_violations",
  "stale_suppressions",
] as const;

const summaryKeys = [
  "total_issues",
  "unused_files",
  "unused_exports",
  "unused_types",
  "private_type_leaks",
  "unused_dependencies",
  "unused_enum_members",
  "unused_class_members",
  "unresolved_imports",
  "unlisted_dependencies",
  "duplicate_exports",
  "type_only_dependencies",
  "test_only_dependencies",
  "circular_dependencies",
  "boundary_violations",
  "stale_suppressions",
  "unused_catalog_entries",
  "empty_catalog_groups",
  "unresolved_catalog_references",
  "unused_dependency_overrides",
  "misconfigured_dependency_overrides",
] as const satisfies ReadonlyArray<keyof FallowCheckResult["summary"]>;

type IssueCollectionKey = (typeof issueCollectionKeys)[number];

const emptyIssueCollections = (): Pick<FallowCheckResult, IssueCollectionKey> =>
  Object.fromEntries(issueCollectionKeys.map((key) => [key, []])) as unknown as Pick<
    FallowCheckResult,
    IssueCollectionKey
  >;

const emptySummary = (): FallowCheckResult["summary"] =>
  Object.fromEntries(summaryKeys.map((key) => [key, 0])) as unknown as FallowCheckResult["summary"];

export const emptyCheck = (): FallowCheckResult => ({
  schema_version: 7,
  version: "0.0.0-test",
  elapsed_ms: 0,
  total_issues: 0,
  ...emptyIssueCollections(),
  summary: emptySummary(),
});
