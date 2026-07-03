import { describe, expect, it, vi } from "vitest";

// The tree-view provider constructs `vscode.TreeItem` / `vscode.ThemeIcon`
// instances and reads `vscode.workspace.workspaceFolders`, so it needs the same
// mock the dedicated tree-view test uses. `countCheckIssues` and the label table
// are pure and need nothing from vscode, but they ride along in the same module
// graph, so one shared mock covers all three surfaces under test.
vi.mock("vscode", async () => {
  const { createTreeViewVscodeMock } = await import("./vscodeTreeMock.js");
  return createTreeViewVscodeMock("/workspace");
});

import { countCheckIssues } from "../src/analysis-utils.js";
import { DIAGNOSTIC_CATEGORIES } from "../src/diagnosticFilter.js";
import { ISSUE_CATEGORY_LABELS, type IssueCategory } from "../src/labels.js";
import { DeadCodeTreeProvider } from "../src/treeView.js";
import type { CheckOutput, FallowCheckResult } from "../src/types.js";
import { emptyCheck } from "./checkFixtures.js";
import type { TestTreeItem } from "./vscodeTreeMock.js";

/**
 * Diagnostic codes in `DIAGNOSTIC_CATEGORIES` that are NOT dead-code findings
 * and therefore have no `CheckOutput` result array, no `countCheckIssues`
 * branch, and no `DeadCodeTreeProvider` category node. They are surfaced through
 * other counters/providers (duplication groups, the security tree, the health
 * view) or other pipelines entirely. This mirrors the way the LSP/CI guard
 * treats these codes separately from the dead-code catalog.
 *
 * Documented reasons:
 * - `code-duplication`: counted by `countDuplicationGroups`, rendered by
 *   `DuplicatesTreeProvider` (clone families), not the dead-code tree. There is
 *   no `CheckOutput` field for it (it lives in `DupesOutput`).
 *
 * Security (`security-sink`, `security-client-server-leak`), coverage-gap, and
 * feature-flag codes are not present in `DIAGNOSTIC_CATEGORIES` (the bundled
 * fallback list) at all, so they need no explicit exclusion entry; the runtime
 * completeness assertion below tolerates their absence by only requiring that
 * every code PRESENT in `DIAGNOSTIC_CATEGORIES` is either mapped or excluded.
 */
const NON_DEAD_CODE_CODES = new Set<string>(["code-duplication"]);

/**
 * One synthetic dead-code finding plus the wiring it must light up:
 * - `field` is the `CheckOutput` result array that carries findings of this
 *   kind. Typing it as `keyof CheckOutput` makes TypeScript compilation FAIL if
 *   a `CheckOutput` field is renamed in the generated contract, so this map can
 *   never silently drift away from the schema (compile-time auto-sync).
 * - `category` is the `IssueCategory` (label/tree key). Typing it as
 *   `IssueCategory` makes compilation FAIL if a label key is renamed.
 * - `finding` is a minimal finding object for `field`; only the fields read by
 *   `countCheckIssues` (none) and `DeadCodeTreeProvider.getChildren` (label /
 *   path / line / col accessors) are populated.
 */
interface KindWiring {
  readonly field: keyof CheckOutput;
  readonly category: IssueCategory;
  readonly diagnosticCode?: string;
  readonly finding: unknown;
}

const loc = { path: "src/x.ts", line: 1, col: 0 };
const pkg = { package_name: "left-pad", path: "package.json", line: 3 };
const member = { parent_name: "Widget", member_name: "render", ...loc };

/**
 * Code -> wiring. Keyed by the dead-code subset of `DIAGNOSTIC_CATEGORIES`
 * codes. The `satisfies` clause keeps each value typed without widening, so the
 * `keyof CheckOutput` / `IssueCategory` guarantees above stay live. A new
 * dead-code code added to `DIAGNOSTIC_CATEGORIES` will trip the runtime
 * completeness assertion (it is neither here nor in `NON_DEAD_CODE_CODES`).
 */
const DEAD_CODE_WIRING = {
  "unused-file": {
    field: "unused_files",
    category: "unused-files",
    finding: { ...loc, actions: [] },
  },
  "unused-export": {
    field: "unused_exports",
    category: "unused-exports",
    finding: { ...loc, export_name: "foo", actions: [] },
  },
  "unused-type": {
    field: "unused_types",
    category: "unused-types",
    finding: { ...loc, export_name: "Foo", actions: [] },
  },
  "private-type-leak": {
    field: "private_type_leaks",
    category: "private-type-leaks",
    finding: { ...loc, export_name: "make", type_name: "State", actions: [] },
  },
  "unused-dependency": {
    field: "unused_dependencies",
    category: "unused-dependencies",
    finding: { ...pkg, actions: [] },
  },
  "unused-dev-dependency": {
    field: "unused_dev_dependencies",
    category: "unused-dev-dependencies",
    finding: { ...pkg, actions: [] },
  },
  "unused-optional-dependency": {
    field: "unused_optional_dependencies",
    category: "unused-optional-dependencies",
    finding: { ...pkg, actions: [] },
  },
  "unused-enum-member": {
    field: "unused_enum_members",
    category: "unused-enum-members",
    finding: { ...member, actions: [] },
  },
  "unused-class-member": {
    field: "unused_class_members",
    category: "unused-class-members",
    finding: { ...member, actions: [] },
  },
  "unused-store-member": {
    field: "unused_store_members",
    category: "unused-store-member",
    finding: { ...member, actions: [] },
  },
  "unused-server-action": {
    field: "unused_server_actions",
    category: "unused-server-action",
    finding: { ...loc, action_name: "submit", actions: [] },
  },
  "unused-load-data-key": {
    field: "unused_load_data_keys",
    category: "unused-load-data-keys",
    finding: { path: "src/routes/+page.ts", line: 2, col: 0, key_name: "posts", actions: [] },
  },
  "unused-component-prop": {
    field: "unused_component_props",
    category: "unused-component-prop",
    finding: { ...loc, component_name: "Btn", prop_name: "size", actions: [] },
  },
  "unused-component-emit": {
    field: "unused_component_emits",
    category: "unused-component-emit",
    finding: { ...loc, component_name: "Btn", emit_name: "click", actions: [] },
  },
  "unused-component-input": {
    field: "unused_component_inputs",
    category: "unused-component-input",
    finding: { ...loc, component_name: "Btn", input_name: "size", actions: [] },
  },
  "unused-component-output": {
    field: "unused_component_outputs",
    category: "unused-component-output",
    finding: { ...loc, component_name: "Btn", output_name: "change", actions: [] },
  },
  "unused-svelte-event": {
    field: "unused_svelte_events",
    category: "unused-svelte-event",
    finding: { ...loc, component_name: "Btn", event_name: "change", actions: [] },
  },
  "unrendered-component": {
    field: "unrendered_components",
    category: "unrendered-component",
    finding: { ...loc, component_name: "Btn", actions: [] },
  },
  "unprovided-inject": {
    field: "unprovided_injects",
    category: "unprovided-inject",
    finding: { ...loc, key_name: "theme", actions: [] },
  },
  "invalid-client-export": {
    field: "invalid_client_exports",
    category: "invalid-client-export",
    finding: { ...loc, export_name: "metadata", actions: [] },
  },
  "mixed-client-server-barrel": {
    field: "mixed_client_server_barrels",
    category: "mixed-client-server-barrel",
    finding: { ...loc, client_origin: "a.ts", server_origin: "b.ts", actions: [] },
  },
  "misplaced-directive": {
    field: "misplaced_directives",
    category: "misplaced-directive",
    finding: { ...loc, directive: "use client", actions: [] },
  },
  "route-collision": {
    field: "route_collisions",
    category: "route-collision",
    finding: { ...loc, url: "/dashboard", actions: [] },
  },
  "dynamic-segment-name-conflict": {
    field: "dynamic_segment_name_conflicts",
    category: "dynamic-segment-name-conflict",
    finding: {
      ...loc,
      position: "app/[id]",
      conflicting_segments: ["id", "slug"],
      actions: [],
    },
  },
  "unresolved-import": {
    field: "unresolved_imports",
    category: "unresolved-imports",
    finding: { ...loc, specifier: "./missing", actions: [] },
  },
  "unlisted-dependency": {
    field: "unlisted_dependencies",
    category: "unlisted-dependencies",
    finding: {
      package_name: "axios",
      imported_from: [{ ...loc }],
      actions: [],
    },
  },
  "duplicate-export": {
    field: "duplicate_exports",
    category: "duplicate-exports",
    finding: {
      export_name: "Thing",
      locations: [{ ...loc }],
      actions: [],
    },
  },
  "type-only-dependency": {
    field: "type_only_dependencies",
    category: "type-only-dependencies",
    finding: { ...pkg, actions: [] },
  },
  "test-only-dependency": {
    field: "test_only_dependencies",
    category: "test-only-dependencies",
    finding: { ...pkg, actions: [] },
  },
  "dev-dependency-in-production": {
    field: "dev_dependencies_in_production",
    category: "dev-dependencies-in-production",
    finding: { ...pkg, actions: [] },
  },
  "circular-dependency": {
    field: "circular_dependencies",
    category: "circular-dependencies",
    finding: { length: 2, files: ["a.ts", "b.ts"], actions: [] },
  },
  "re-export-cycle": {
    field: "re_export_cycles",
    category: "re-export-cycles",
    finding: { kind: "cycle", files: ["a.ts", "b.ts"], actions: [] },
  },
  "boundary-violation": {
    field: "boundary_violations",
    category: "boundary-violation",
    finding: {
      from_path: "ui/x.ts",
      to_path: "db/y.ts",
      from_zone: "ui",
      to_zone: "data",
      import_specifier: "../db/y",
      line: 1,
      col: 0,
      actions: [],
    },
  },
  "boundary-coverage": {
    field: "boundary_coverage_violations",
    category: "boundary-violation",
    diagnosticCode: "boundary-violation",
    finding: { ...loc, actions: [] },
  },
  "boundary-call-violation": {
    field: "boundary_call_violations",
    category: "boundary-violation",
    diagnosticCode: "boundary-violation",
    finding: { ...loc, zone: "ui", callee: "cp.exec", pattern: "child_process.*", actions: [] },
  },
  "policy-violation": {
    field: "policy_violations",
    category: "policy-violations",
    finding: {
      path: "src/app.ts",
      line: 7,
      col: 2,
      pack: "team-policy",
      rule_id: "no-moment",
      kind: "banned-import",
      matched: "moment",
      severity: "warn",
      message: "Use date-fns.",
      actions: [],
    },
  },
  "stale-suppression": {
    field: "stale_suppressions",
    category: "stale-suppressions",
    finding: {
      ...loc,
      origin: { type: "comment", issue_kind: "unused-export", is_file_level: false },
    },
  },
  "missing-suppression-reason": {
    field: "stale_suppressions",
    category: "stale-suppressions",
    diagnosticCode: "stale-suppression",
    finding: {
      ...loc,
      origin: { type: "comment", issue_kind: "unused-export", is_file_level: false },
      missing_reason: true,
    },
  },
  "unused-catalog-entry": {
    field: "unused_catalog_entries",
    category: "unused-catalog-entries",
    finding: { path: "pnpm-workspace.yaml", line: 2, catalog_name: "default", entry_name: "react", actions: [] },
  },
  "empty-catalog-group": {
    field: "empty_catalog_groups",
    category: "empty-catalog-groups",
    finding: { path: "pnpm-workspace.yaml", line: 2, catalog_name: "react17", actions: [] },
  },
  "unresolved-catalog-reference": {
    field: "unresolved_catalog_references",
    category: "unresolved-catalog-references",
    finding: { path: "package.json", line: 2, catalog_name: "default", entry_name: "react", actions: [] },
  },
  "unused-dependency-override": {
    field: "unused_dependency_overrides",
    category: "unused-dependency-overrides",
    finding: { path: "package.json", line: 2, raw_key: "react", source: "pnpm.overrides", actions: [] },
  },
  "misconfigured-dependency-override": {
    field: "misconfigured_dependency_overrides",
    category: "misconfigured-dependency-overrides",
    finding: { path: "package.json", line: 2, raw_key: "", source: "pnpm.overrides", actions: [] },
  },
} satisfies Record<string, KindWiring>;

type MappedCode = keyof typeof DEAD_CODE_WIRING;

/** Build a `CheckOutput` with exactly one finding in `field`. */
const checkWith = (field: keyof CheckOutput, finding: unknown): FallowCheckResult =>
  ({
    ...emptyCheck(),
    [field]: [finding],
  }) as FallowCheckResult;

/** The DIAGNOSTIC_CATEGORIES label for a code (the canonical human label). */
const canonicalLabel = (code: string): string => {
  const category = DIAGNOSTIC_CATEGORIES.find((c) => c.code === code);
  return category?.label ?? "";
};

describe("dead-code IssueKind drift guard", () => {
  it("every DIAGNOSTIC_CATEGORIES code is either dead-code-mapped or documented non-dead-code", () => {
    // Chains to the existing `diagnosticFilter` drift test ("includes every
    // diagnostic code"): that test pins DIAGNOSTIC_CATEGORIES to the LSP's
    // emitted codes, this one pins each of those codes to the sidebar's three
    // surfaces. A new kind that lands in DIAGNOSTIC_CATEGORIES but not here (and
    // not in NON_DEAD_CODE_CODES) trips this assertion.
    const unaccounted = DIAGNOSTIC_CATEGORIES.map((c) => c.code).filter(
      (code) => !(code in DEAD_CODE_WIRING) && !NON_DEAD_CODE_CODES.has(code),
    );
    expect(unaccounted).toEqual([]);
  });

  it("excludes only genuine non-dead-code codes (no dead-code kind hidden in the skip set)", () => {
    // Guard against an over-broad exclusion silently dropping a real dead-code
    // kind from the sidebar contract. Anything in NON_DEAD_CODE_CODES must NOT
    // also be a mapped dead-code kind.
    for (const code of NON_DEAD_CODE_CODES) {
      expect(code in DEAD_CODE_WIRING).toBe(false);
    }
  });

  const cases = Object.keys(DEAD_CODE_WIRING) as MappedCode[];

  it.each(cases)("%s is counted, rendered as a category, and labeled", (code) => {
    const wiring: KindWiring = DEAD_CODE_WIRING[code];
    const { field, category, diagnosticCode, finding } = wiring;
    const check = checkWith(field, finding);

    // (a) countCheckIssues counts it.
    expect(countCheckIssues(check)).toBeGreaterThanOrEqual(1);

    // (b) DeadCodeTreeProvider renders a category node for it.
    const provider = new DeadCodeTreeProvider();
    provider.update(check);
    const categories = provider.getChildren() as TestTreeItem[];
    expect(categories).toHaveLength(1);
    const node = categories[0] as TestTreeItem;

    // (c) ISSUE_CATEGORY_LABELS has a real label (not the generic "warning"
    // fallback the tree falls back to for an unmapped icon), and the rendered
    // category node uses it. The node label is `${label} (count)`.
    const label = ISSUE_CATEGORY_LABELS[category];
    expect(label).toBeTruthy();
    expect(node.label).toBe(`${label} (1)`);

    // The tree's category label must agree with the LSP's canonical label for
    // the code, so the sidebar and the squiggle catalog never disagree.
    expect(label).toBe(canonicalLabel(diagnosticCode ?? code));
    provider.dispose();
  });
});
