import { describe, expect, it } from "vitest";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { COUNTED_FINDING_KEYS } from "../src/analysis-utils.js";
import {
  alignWithSidebarCount,
  buildAnalysisSummaryLines,
  buildParamsFromCli,
  formatChangedSinceRefForStatusBar,
  getDuplicationPercentage,
  renderStatusBarText,
} from "../src/statusBar-utils.js";
import type { AnalysisCompleteParams } from "../src/statusBar-utils.js";
import type { FallowCheckResult, FallowDupesResult } from "../src/types.js";
import { ISSUE_TYPE_DEFAULTS } from "../src/generated/issue-types.js";

const baseParams = (
  overrides: Partial<AnalysisCompleteParams> = {}
): AnalysisCompleteParams => Object.assign({
  totalIssues: 0,
  unusedFiles: 0,
  unusedExports: 0,
  unusedTypes: 0,
  privateTypeLeaks: 0,
  unusedDependencies: 0,
  unusedDevDependencies: 0,
  unusedOptionalDependencies: 0,
  unusedEnumMembers: 0,
  unusedClassMembers: 0,
  unresolvedImports: 0,
  unlistedDependencies: 0,
  unlistedBinaries: 0,
  duplicateExports: 0,
  typeOnlyDependencies: 0,
  testOnlyDependencies: 0,
  devDependenciesInProduction: 0,
  circularDependencies: 0,
  reExportCycles: 0,
  packageCycles: 0,
  boundaryViolations: 0,
  staleSuppressions: 0,
  unusedCatalogEntries: 0,
  unresolvedCatalogReferences: 0,
  unusedDependencyOverrides: 0,
  misconfiguredDependencyOverrides: 0,
  duplicationPercentage: 0,
  cloneGroups: 0,
}, overrides);

describe("getDuplicationPercentage", () => {
  it("clamps non-finite values to zero", () => {
    expect(getDuplicationPercentage(Number.NaN)).toBe(0);
    expect(getDuplicationPercentage(Number.POSITIVE_INFINITY)).toBe(0);
  });

  it("keeps finite values unchanged", () => {
    expect(getDuplicationPercentage(4.25)).toBe(4.25);
  });
});

const summary = (params: AnalysisCompleteParams, changedSinceRef?: string): string =>
  buildAnalysisSummaryLines(params, changedSinceRef).join("\n\n");

describe("buildAnalysisSummaryLines", () => {
  it("includes only present issue categories", () => {
    const markdown = summary(
      baseParams({
        totalIssues: 4,
        unusedFiles: 1,
        unresolvedImports: 2,
        cloneGroups: 1,
        duplicationPercentage: 3.25,
      })
    );

    expect(markdown).toContain("$(error) 2 unresolved imports");
    expect(markdown).toContain("$(warning) 1 unused files");
    expect(markdown).not.toContain("unused exports");
  });

  it("shows a success message when no issues exist", () => {
    const markdown = summary(baseParams());

    expect(markdown).toContain("$(check) No issues found");
  });

  it("surfaces the changedSince ref when scoped", () => {
    const markdown = summary(baseParams(), "fallow-baseline");
    expect(markdown).toContain("Scoped to changes since fallow\\-baseline");
  });

  it("lists package refs and explains the remaining full scope", () => {
    const markdown = summary(
      baseParams({
        packageBaselines: [
          { workspace_root: "packages/web", reference: "main" },
          { workspace_root: "packages/legacy", reference: "release/2024.10" },
        ],
      }),
    );
    expect(markdown).toContain("Code diagnostics and clone groups use package baselines:");
    expect(markdown).toContain("packages/web: main");
    expect(markdown).toContain("packages/legacy: release/2024\\.10");
    expect(markdown).toContain("Unlisted packages and root files remain in full scope.");
    expect(markdown).toContain("Health and security reports use their own scopes.");
  });

  it("prefers a completed package scope over a setting changed after the run", () => {
    const params = baseParams({
      packageBaselines: [{ workspace_root: "packages/web", reference: "main" }],
    });
    expect(summary(params, "new-global-ref"))
      .toContain("packages/web: main");
    expect(renderStatusBarText("$(search) Fallow", "new-global-ref", undefined, params.packageBaselines))
      .toBe("$(search) Fallow (package baselines)");
  });

  it("uses the server-applied changedSince scope when present", () => {
    const markdown = summary(
      baseParams({
        changedSinceScope: {
          requestedRef: "origin/main",
          state: "applied",
        },
      }),
      "configured-ref",
    );

    expect(markdown).toContain("Scoped to changes since origin/main");
    expect(markdown).not.toContain("configured\\-ref");
  });

  it("warns that dropped changedSince results are full-scope", () => {
    const markdown = summary(
      baseParams({
        changedSinceScope: {
          requestedRef: "missing-ref",
          state: "dropped",
          reason: "git ref was not found",
        },
      }),
      "missing-ref",
    );

    expect(markdown).toContain("Scope since missing\\-ref was dropped");
    expect(markdown).toContain("results are full-scope");
    expect(markdown).toContain("git ref was not found");
    expect(markdown).not.toContain("Scoped to changes since");
  });

  it("escapes changedSince markdown in trusted tooltip text", () => {
    const markdown = summary(
      baseParams(),
      "base` [open](command:workbench.action.openSettings)"
    );
    expect(markdown).toContain(
      "base\\` \\[open\\]\\(command:workbench\\.action\\.openSettings\\)"
    );
  });

  it("omits the scope line when no changedSince ref is given", () => {
    const markdown = summary(baseParams());
    expect(markdown).not.toContain("Scoped to changes since");
  });
});

describe("formatChangedSinceRefForStatusBar", () => {
  it("normalizes whitespace for the compact status bar label", () => {
    expect(formatChangedSinceRefForStatusBar(" feature\nbranch\tname ")).toBe(
      "feature branch name"
    );
  });

  it("truncates long refs for the compact status bar label", () => {
    const formatted = formatChangedSinceRefForStatusBar(
      "feature/some-extremely-long-baseline-branch-name-that-would-crowd-the-status-bar"
    );
    expect(formatted.length).toBeLessThanOrEqual(48);
    expect(formatted).toMatch(/\.\.\.$/);
  });
});

describe("renderStatusBarText", () => {
  it("returns the base label unchanged when no changedSince is set", () => {
    expect(renderStatusBarText("$(search) Fallow", null)).toBe(
      "$(search) Fallow"
    );
    expect(renderStatusBarText("$(search) Fallow: 5 issues", null)).toBe(
      "$(search) Fallow: 5 issues"
    );
  });

  it("treats empty-string ref as 'no filter' so the suffix never renders blank", () => {
    // Defensive: callers should pre-coerce "" to null via getChangedSince()
    // (or the liveChangedSince() wrapper in statusBar.ts). If the empty
    // string ever leaks through directly, the helper must still fall
    // back to the base label rather than rendering "Fallow (since )".
    expect(renderStatusBarText("$(search) Fallow", "")).toBe(
      "$(search) Fallow"
    );
  });

  it("appends `(since <ref>)` to every state when changedSince is active", () => {
    const ref = "fallow-baseline";
    expect(renderStatusBarText("$(search) Fallow", ref)).toBe(
      "$(search) Fallow (since fallow-baseline)"
    );
    expect(
      renderStatusBarText("$(loading~spin) Fallow: Analyzing...", ref)
    ).toBe("$(loading~spin) Fallow: Analyzing... (since fallow-baseline)");
    expect(renderStatusBarText("$(error) Fallow: Error", ref)).toBe(
      "$(error) Fallow: Error (since fallow-baseline)"
    );
    expect(renderStatusBarText("$(search) Fallow: 3 issues", ref)).toBe(
      "$(search) Fallow: 3 issues (since fallow-baseline)"
    );
  });

  it("preserves the applied suffix from the completion notification", () => {
    expect(
      renderStatusBarText("$(search) Fallow: 3 issues", "configured-ref", {
        requestedRef: "origin/main",
        state: "applied",
      }),
    ).toBe("$(search) Fallow: 3 issues (since origin/main)");
  });

  it("shows reported package baselines until a reported global scope overrides them", () => {
    const packages = [{ workspace_root: "packages/web", reference: "main" }];
    expect(renderStatusBarText("$(search) Fallow", null, undefined, packages)).toBe(
      "$(search) Fallow (package baselines)",
    );
    expect(renderStatusBarText("$(search) Fallow", "HEAD", undefined, packages)).toBe(
      "$(search) Fallow (package baselines)",
    );
    expect(renderStatusBarText("$(search) Fallow", "HEAD", {
      requestedRef: "HEAD",
      state: "applied",
    }, packages)).toBe(
      "$(search) Fallow (since HEAD)",
    );
  });

  it("marks dropped scope without claiming the filter is active", () => {
    expect(
      renderStatusBarText("$(search) Fallow: 3 issues", "missing-ref", {
        requestedRef: "missing-ref",
        state: "dropped",
        reason: "git ref was not found",
      }),
    ).toBe("$(search) Fallow: 3 issues (since missing-ref: scope dropped)");
  });

  it("delegates to formatChangedSinceRefForStatusBar for truncation", () => {
    const longRef =
      "feature/some-extremely-long-baseline-branch-name-that-would-crowd-the-status-bar";
    const rendered = renderStatusBarText("$(search) Fallow", longRef);
    expect(rendered.startsWith("$(search) Fallow (since ")).toBe(true);
    expect(rendered.endsWith("...)")).toBe(true);
  });
});

describe("buildParamsFromCli", () => {
  const emptyCheck = (): FallowCheckResult => ({
    schema_version: 11,
    version: "0.0.0-test",
    elapsed_ms: 0,
    total_issues: 0,
    unused_files: [],
    unused_exports: [],
    unused_types: [],
    private_type_leaks: [],
    unused_dependencies: [],
    unused_dev_dependencies: [],
    unused_optional_dependencies: [],
    unused_enum_members: [],
    unused_class_members: [],
    unresolved_imports: [],
    unlisted_dependencies: [],
    duplicate_exports: [],
    type_only_dependencies: [],
    test_only_dependencies: [],
    circular_dependencies: [],
    boundary_violations: [],
    stale_suppressions: [],
    summary: {
      total_issues: 0,
      unused_files: 0,
      unused_exports: 0,
      unused_types: 0,
      private_type_leaks: 0,
      deprecated_exports_in_use: 0,
      unused_dependencies: 0,
      unused_enum_members: 0,
      unused_class_members: 0,
      unresolved_imports: 0,
      unlisted_dependencies: 0,
      duplicate_exports: 0,
      type_only_dependencies: 0,
      test_only_dependencies: 0,
      dev_dependencies_in_production: 0,
      circular_dependencies: 0,
      boundary_violations: 0,
      stale_suppressions: 0,
      unused_catalog_entries: 0,
      empty_catalog_groups: 0,
      unresolved_catalog_references: 0,
      unused_dependency_overrides: 0,
      misconfigured_dependency_overrides: 0,
    },
  });

  it("returns zero counts when both inputs are null", () => {
    const params = buildParamsFromCli(null, null, null);
    expect(params.totalIssues).toBe(0);
    expect(params.duplicationPercentage).toBe(0);
    expect(params.cloneGroups).toBe(0);
  });

  it("uses CLI scope provenance for the displayed analysis", () => {
    const params = buildParamsFromCli(emptyCheck(), null, {
      package_baselines: [{ workspace_root: "packages/web", reference: "main" }],
    });
    expect(
      renderStatusBarText("$(search) Fallow", null, params.changedSinceScope, params.packageBaselines),
    ).toBe("$(search) Fallow (package baselines)");
    expect(summary(params)).toContain("packages/web: main");

    const dropped = buildParamsFromCli(emptyCheck(), null, {
      request_outcomes: {
        "changed-since": {
          status: "not-applied",
          affects: "scope",
          requested: "missing-ref",
          reason: "git-missing",
          message: "The Git ref was not found.",
        },
      },
    });
    expect(renderStatusBarText("$(search) Fallow", "missing-ref", dropped.changedSinceScope))
      .toBe("$(search) Fallow (since missing-ref: scope dropped)");
    expect(summary(dropped, "missing-ref"))
      .toContain("The Git ref was not found\\.");
  });

  it("counts issue categories from the check result", () => {
    const check: FallowCheckResult = {
      ...emptyCheck(),
      unused_files: [{ path: "a.ts", actions: [] }],
      unused_exports: [
        {
          path: "b.ts",
          export_name: "x",
          is_type_only: false,
          line: 1,
          col: 0,
          span_start: 0,
          is_re_export: false,
          actions: [],
        },
        {
          path: "c.ts",
          export_name: "y",
          is_type_only: false,
          line: 1,
          col: 0,
          span_start: 0,
          is_re_export: false,
          actions: [],
        },
      ],
      unused_optional_dependencies: [
        {
          path: "package.json",
          package_name: "fsevents",
          location: "optionalDependencies",
          line: 1,
          actions: [],
        },
      ],
      unresolved_imports: [
        { path: "d.ts", specifier: "./missing", line: 1, col: 0, specifier_col: 0, actions: [] },
      ],
      private_type_leaks: [
        {
          path: "api.ts",
          export_name: "makeWidget",
          type_name: "WidgetState",
          line: 2,
          col: 9,
          span_start: 12,
          actions: [],
        },
      ],
      test_only_dependencies: [
        {
          path: "package.json",
          package_name: "vitest",
          line: 2,
          actions: [],
        },
      ],
      boundary_violations: [
        {
          from_path: "ui/button.ts",
          to_path: "db/client.ts",
          from_zone: "ui",
          to_zone: "data",
          import_specifier: "../db/client",
          line: 3,
          col: 0,
          actions: [],
        },
      ],
      stale_suppressions: [
        {
          path: "src/index.ts",
          line: 4,
          col: 0,
          origin: {
            type: "comment",
            issue_kind: "unused-export",
            is_file_level: false,
          },
          actions: [],
        },
      ],
    };

    const params = buildParamsFromCli(check, null, null);
    expect(params.unusedFiles).toBe(1);
    expect(params.unusedExports).toBe(2);
    expect(params.unusedOptionalDependencies).toBe(1);
    expect(params.privateTypeLeaks).toBe(1);
    expect(params.testOnlyDependencies).toBe(1);
    expect(params.boundaryViolations).toBe(1);
    expect(params.staleSuppressions).toBe(1);
    expect(params.unresolvedImports).toBe(1);
    expect(params.totalIssues).toBe(9);
    expect(params.duplicationPercentage).toBe(0);
  });

  it("collapses boundary sub-results into the boundary status count", () => {
    const check: FallowCheckResult = {
      ...emptyCheck(),
      boundary_coverage_violations: [
        {
          path: "src/unzoned.ts",
          line: 4,
          col: 0,
          actions: [],
        },
      ],
      boundary_call_violations: [
        {
          path: "src/domain.ts",
          line: 8,
          col: 2,
          zone: "domain",
          callee: "console.log",
          pattern: "console.*",
          actions: [],
        },
      ],
    };

    const params = buildParamsFromCli(check, null, null);
    expect(params.boundaryViolations).toBe(2);
    expect(params.totalIssues).toBe(2);
  });

  it("propagates duplication stats from the dupes result so the tooltip matches the status bar text", () => {
    const dupes: FallowDupesResult = {
      clone_groups: [],
      clone_families: [],
      stats: {
        total_files: 10,
        files_with_clones: 2,
        total_lines: 1000,
        duplicated_lines: 8,
        total_tokens: 5000,
        duplicated_tokens: 40,
        clone_groups: 3,
        clone_families: 1,
        clone_instances: 6,
        duplication_percentage: 0.8,
      },
    };

    const params = buildParamsFromCli(null, dupes, null);
    expect(params.duplicationPercentage).toBe(0.8);
    expect(params.cloneGroups).toBe(3);
  });

  it("treats missing optional check fields as zero counts", () => {
    const check = emptyCheck();
    delete (check as { type_only_dependencies?: unknown })
      .type_only_dependencies;
    delete (check as { circular_dependencies?: unknown })
      .circular_dependencies;
    delete (check as { unused_optional_dependencies?: unknown })
      .unused_optional_dependencies;
    delete (check as { private_type_leaks?: unknown }).private_type_leaks;
    delete (check as { test_only_dependencies?: unknown })
      .test_only_dependencies;
    delete (check as { boundary_violations?: unknown }).boundary_violations;
    delete (check as { stale_suppressions?: unknown }).stale_suppressions;

    const params = buildParamsFromCli(check, null, null);
    expect(params.unusedOptionalDependencies).toBe(0);
    expect(params.privateTypeLeaks).toBe(0);
    expect(params.typeOnlyDependencies).toBe(0);
    expect(params.testOnlyDependencies).toBe(0);
    expect(params.devDependenciesInProduction).toBe(0);
    expect(params.circularDependencies).toBe(0);
    expect(params.boundaryViolations).toBe(0);
    expect(params.staleSuppressions).toBe(0);
  });
});

const toField = (key: string): string =>
  key.replace(/_([a-z])/g, (_match: string, letter: string) => letter.toUpperCase());

/** Params with a field for each counted category, as the current LSP sends. */
const lspParams = (counts: Record<string, number>): AnalysisCompleteParams => {
  const fields: Record<string, number> = {};
  for (const key of COUNTED_FINDING_KEYS) {
    fields[toField(key)] = 0;
  }
  delete fields["boundaryCoverageViolations"];
  delete fields["boundaryCallViolations"];
  return { ...baseParams(), ...fields, ...counts } as AnalysisCompleteParams;
};

describe("alignWithSidebarCount", () => {
  // The status bar count must equal the sidebar count and the "found N issues"
  // notification (#5278).
  it("removes the categories that fallow.issueTypes turns off", () => {
    const params = lspParams({ totalIssues: 250, circularDependencies: 72, unusedFiles: 35 });
    const aligned = alignWithSidebarCount(params, {
      ...ISSUE_TYPE_DEFAULTS,
      "circular-dependencies": false,
    });
    expect(aligned.totalIssues).toBe(35);
    expect(aligned.circularDependencies).toBe(0);
    expect(aligned.unusedFiles).toBe(35);
  });

  it("removes a category that has no typed field in the extension", () => {
    const params = lspParams({ totalIssues: 7, routeCollisions: 4, unusedExports: 3 });
    const aligned = alignWithSidebarCount(params, {
      ...ISSUE_TYPE_DEFAULTS,
      "route-collision": false,
    });
    expect(aligned.totalIssues).toBe(3);
  });

  it("leaves out the findings that the sidebar does not count", () => {
    // The LSP total also counts deprecated exports in use and enforced
    // component findings. The sidebar count leaves them out.
    const params = lspParams({ totalIssues: 12, unusedExports: 9 });
    expect(alignWithSidebarCount(params, ISSUE_TYPE_DEFAULTS).totalIssues).toBe(9);
  });

  it("removes the boundary count once for the three boundary arrays", () => {
    const params = lspParams({ totalIssues: 9, boundaryViolations: 5, unusedFiles: 4 });
    const aligned = alignWithSidebarCount(params, {
      ...ISSUE_TYPE_DEFAULTS,
      "boundary-violation": false,
    });
    expect(aligned.totalIssues).toBe(4);
  });

  it("subtracts from the total of an older LSP that sends fewer fields", () => {
    const params = baseParams({ totalIssues: 250, circularDependencies: 72 });
    const aligned = alignWithSidebarCount(params, {
      ...ISSUE_TYPE_DEFAULTS,
      "circular-dependencies": false,
    });
    expect(aligned.totalIssues).toBe(178);
  });

  it("sends a params field for each category that the sidebar counts", () => {
    const protocol = readFileSync(
      resolve(__dirname, "../../../crates/lsp/src/protocol.rs"),
      "utf8",
    );
    const struct = protocol.slice(protocol.indexOf("pub struct AnalysisCompleteParams"));
    const fields = new Set(
      [...struct.slice(0, struct.indexOf("\n}\n")).matchAll(/pub ([a-z_]+):/g)].map(
        (match) => match[1],
      ),
    );
    const sharedBoundaryCount = new Set([
      "boundary_coverage_violations",
      "boundary_call_violations",
    ]);
    const missing = COUNTED_FINDING_KEYS.filter(
      (key) => !fields.has(key) && !sharedBoundaryCount.has(key),
    );
    expect(missing).toEqual([]);
  });
});
