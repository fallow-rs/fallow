import { describe, expect, it } from "vitest";
import {
  buildParamsFromCli,
  buildStatusBarPartsFromLsp,
  buildStatusBarTooltipMarkdown,
  formatChangedSinceRefForStatusBar,
  getStatusBarSeverityKey,
  getDuplicationPercentage,
  renderStatusBarText,
} from "../src/statusBar-utils.js";
import type { AnalysisCompleteParams } from "../src/statusBar-utils.js";
import type { FallowCheckResult, FallowDupesResult } from "../src/types.js";

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
  duplicateExports: 0,
  typeOnlyDependencies: 0,
  testOnlyDependencies: 0,
  circularDependencies: 0,
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

describe("buildStatusBarPartsFromLsp", () => {
  it("builds issue and duplication summary parts", () => {
    expect(
      buildStatusBarPartsFromLsp(
        baseParams({ totalIssues: 3, duplicationPercentage: 1.234 })
      )
    ).toEqual(["3 issues", "1.2% duplication"]);
  });
});

describe("getStatusBarSeverityKey", () => {
  it("prefers error styling for unresolved imports", () => {
    expect(
      getStatusBarSeverityKey(
        baseParams({ totalIssues: 2, unresolvedImports: 1 })
      )
    ).toBe("statusBarItem.errorBackground");
  });

  it("uses warning styling when issues exist without unresolved imports", () => {
    expect(
      getStatusBarSeverityKey(baseParams({ totalIssues: 2 }))
    ).toBe("statusBarItem.warningBackground");
  });

  it("returns null when there are no issues", () => {
    expect(getStatusBarSeverityKey(baseParams())).toBeNull();
  });
});

describe("buildStatusBarTooltipMarkdown", () => {
  it("includes only present issue categories and action links", () => {
    const markdown = buildStatusBarTooltipMarkdown(
      baseParams({
        totalIssues: 4,
        unusedFiles: 1,
        unresolvedImports: 2,
        cloneGroups: 1,
        duplicationPercentage: 3.25,
      })
    );

    expect(markdown).toContain("**Fallow** - Analysis Results");
    expect(markdown).toContain("$(error) 2 unresolved imports");
    expect(markdown).toContain("$(warning) 1 unused files");
    expect(markdown).toContain("$(copy) 1 clone groups (3.3% duplication)");
    expect(markdown).toContain("command:fallow.analyze");
    expect(markdown).not.toContain("unused exports");
  });

  it("shows a success message when no issues or clones exist", () => {
    const markdown = buildStatusBarTooltipMarkdown(baseParams());

    expect(markdown).toContain("$(check) No issues found");
  });

  it("surfaces the changedSince ref when scoped", () => {
    const markdown = buildStatusBarTooltipMarkdown(baseParams(), "fallow-baseline");
    expect(markdown).toContain("Scoped to changes since fallow\\-baseline");
  });

  it("escapes changedSince markdown in trusted tooltip text", () => {
    const markdown = buildStatusBarTooltipMarkdown(
      baseParams(),
      "base` [open](command:workbench.action.openSettings)"
    );
    expect(markdown).toContain(
      "base\\` \\[open\\]\\(command:workbench\\.action\\.openSettings\\)"
    );
  });

  it("omits the scope line when no changedSince ref is given", () => {
    const markdown = buildStatusBarTooltipMarkdown(baseParams());
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
    schema_version: 6,
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
  });

  it("returns zero counts when both inputs are null", () => {
    const params = buildParamsFromCli(null, null);
    expect(params.totalIssues).toBe(0);
    expect(params.duplicationPercentage).toBe(0);
    expect(params.cloneGroups).toBe(0);
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
        { path: "d.ts", specifier: "./missing", line: 1, col: 0, actions: [] },
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
        },
      ],
    };

    const params = buildParamsFromCli(check, null);
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

  it("propagates duplication stats from the dupes result so the tooltip matches the status bar text", () => {
    const dupes: FallowDupesResult = {
      schema_version: 6,
      version: "0.0.0-test",
      elapsed_ms: 0,
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
        clone_instances: 6,
        duplication_percentage: 0.8,
      },
    };

    const params = buildParamsFromCli(null, dupes);
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

    const params = buildParamsFromCli(check, null);
    expect(params.unusedOptionalDependencies).toBe(0);
    expect(params.privateTypeLeaks).toBe(0);
    expect(params.typeOnlyDependencies).toBe(0);
    expect(params.testOnlyDependencies).toBe(0);
    expect(params.circularDependencies).toBe(0);
    expect(params.boundaryViolations).toBe(0);
    expect(params.staleSuppressions).toBe(0);
  });
});
