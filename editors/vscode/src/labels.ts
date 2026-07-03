/**
 * Tree-view category keys. The mapping groups output arrays under the
 * category names the existing tree view expects, while labels are read from
 * the generated IssueKindMeta registry surface.
 */

import { DIAGNOSTIC_CATEGORIES } from "./generated/issue-types.js";

const CATEGORY_TO_REGISTRY_CODE = {
  "unused-files": "unused-file",
  "unused-exports": "unused-export",
  "unused-types": "unused-type",
  "private-type-leaks": "private-type-leak",
  "unused-dependencies": "unused-dependency",
  "unused-dev-dependencies": "unused-dev-dependency",
  "unused-optional-dependencies": "unused-optional-dependency",
  "unused-enum-members": "unused-enum-member",
  "unused-class-members": "unused-class-member",
  "unused-store-member": "unused-store-member",
  "unused-server-action": "unused-server-action",
  "unused-load-data-keys": "unused-load-data-key",
  "unused-component-prop": "unused-component-prop",
  "unused-component-emit": "unused-component-emit",
  "unused-component-input": "unused-component-input",
  "unused-component-output": "unused-component-output",
  "unused-svelte-event": "unused-svelte-event",
  "unrendered-component": "unrendered-component",
  "unprovided-inject": "unprovided-inject",
  "invalid-client-export": "invalid-client-export",
  "mixed-client-server-barrel": "mixed-client-server-barrel",
  "misplaced-directive": "misplaced-directive",
  "route-collision": "route-collision",
  "dynamic-segment-name-conflict": "dynamic-segment-name-conflict",
  "unresolved-imports": "unresolved-import",
  "unlisted-dependencies": "unlisted-dependency",
  "duplicate-exports": "duplicate-export",
  "type-only-dependencies": "type-only-dependency",
  "test-only-dependencies": "test-only-dependency",
  "dev-dependencies-in-production": "dev-dependency-in-production",
  "circular-dependencies": "circular-dependency",
  "re-export-cycles": "re-export-cycle",
  "boundary-violation": "boundary-violation",
  "policy-violations": "policy-violation",
  "stale-suppressions": "stale-suppression",
  "unused-catalog-entries": "unused-catalog-entry",
  "empty-catalog-groups": "empty-catalog-group",
  "unresolved-catalog-references": "unresolved-catalog-reference",
  "unused-dependency-overrides": "unused-dependency-override",
  "misconfigured-dependency-overrides": "misconfigured-dependency-override",
} as const;

export type IssueCategory = keyof typeof CATEGORY_TO_REGISTRY_CODE;

const LABEL_BY_REGISTRY_CODE = new Map(
  DIAGNOSTIC_CATEGORIES.map((category) => [category.code, category.label] as const),
);

const labelForRegistryCode = (code: string): string => {
  const label = LABEL_BY_REGISTRY_CODE.get(code);
  if (label === undefined) {
    throw new Error(`Missing diagnostic category label for ${code}`);
  }
  return label;
};

export const ISSUE_CATEGORY_LABELS = Object.fromEntries(
  Object.entries(CATEGORY_TO_REGISTRY_CODE).map(([category, code]) => [
    category,
    labelForRegistryCode(code),
  ]),
) as Record<IssueCategory, string>;
