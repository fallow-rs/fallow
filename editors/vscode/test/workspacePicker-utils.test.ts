import { describe, expect, it } from "vitest";
import {
  CLEAR_WORKSPACE_SCOPE,
  WORKSPACE_SCOPE_DISCLOSURE,
  buildWorkspaceQuickPickItems,
  clearedScopeToast,
  parseWorkspacesOutput,
  partitionWorkspaces,
  renderWorkspaceStatusBarText,
  renderWorkspaceStatusBarTooltip,
  resolveWorkspaceScope,
  shouldShowWorkspacePicker,
} from "../src/workspacePicker-utils.js";
import { buildAnalysisArgs } from "../src/analysis-utils.js";
import type { WorkspaceInfo, WorkspacesOutput } from "../src/workspace-types.js";

const ws = (
  name: string,
  path: string,
  is_internal_dependency = false,
): WorkspaceInfo => ({ name, path, is_internal_dependency });

const baseArgsOptions = {
  production: false,
  changedSince: "",
  workspace: "",
  configPath: "",
  dupesMode: "mild" as const,
  dupesThreshold: 0,
  dupesMinTokens: 50,
  dupesMinLines: 5,
  minOccurrences: 2,
  dupesSkipLocal: false,
  dupesCrossLanguage: false,
  dupesIgnoreImports: false,
  cliVersion: null,
};

describe("parseWorkspacesOutput", () => {
  it("parses a valid envelope", () => {
    const json = JSON.stringify({
      workspace_count: 2,
      workspaces: [
        { name: "app", path: "apps/app", is_internal_dependency: false },
        { name: "ui", path: "packages/ui", is_internal_dependency: false },
      ],
    });
    const result = parseWorkspacesOutput(json);
    expect(result).not.toBeNull();
    expect(result?.workspace_count).toBe(2);
    expect(result?.workspaces).toHaveLength(2);
    expect(result?.workspaces[0]).toEqual({
      name: "app",
      path: "apps/app",
      is_internal_dependency: false,
    });
  });

  it("returns null on empty input", () => {
    expect(parseWorkspacesOutput("")).toBeNull();
    expect(parseWorkspacesOutput("   \n ")).toBeNull();
  });

  it("returns null on invalid JSON", () => {
    expect(parseWorkspacesOutput("{not json")).toBeNull();
  });

  it("returns null when the workspaces array is missing", () => {
    expect(parseWorkspacesOutput(JSON.stringify({ workspace_count: 0 }))).toBeNull();
  });

  it("drops malformed entries but keeps valid ones", () => {
    const json = JSON.stringify({
      workspaces: [
        { name: "ok", path: "p", is_internal_dependency: false },
        { path: "no-name" },
        null,
        42,
        { name: "", path: "empty-name" },
      ],
    });
    const result = parseWorkspacesOutput(json);
    expect(result?.workspaces).toHaveLength(1);
    expect(result?.workspaces[0].name).toBe("ok");
  });

  it("defaults a missing path to empty string and missing internal flag to false", () => {
    const json = JSON.stringify({ workspaces: [{ name: "x" }] });
    const result = parseWorkspacesOutput(json);
    expect(result?.workspaces[0]).toEqual({
      name: "x",
      path: "",
      is_internal_dependency: false,
    });
  });

  it("falls back workspace_count to the parsed length when absent", () => {
    const json = JSON.stringify({
      workspaces: [{ name: "a", path: "a" }, { name: "b", path: "b" }],
    });
    expect(parseWorkspacesOutput(json)?.workspace_count).toBe(2);
  });
});

describe("partitionWorkspaces", () => {
  it("separates internal from real packages and sorts each by name", () => {
    const { real, internal } = partitionWorkspaces([
      ws("zebra", "z"),
      ws("@scope/win32", "npm/win32", true),
      ws("apple", "a"),
      ws("@scope/darwin", "npm/darwin", true),
    ]);
    expect(real.map((w) => w.name)).toEqual(["apple", "zebra"]);
    expect(internal.map((w) => w.name)).toEqual(["@scope/darwin", "@scope/win32"]);
  });

  it("handles an all-real list", () => {
    const { real, internal } = partitionWorkspaces([ws("b", "b"), ws("a", "a")]);
    expect(real.map((w) => w.name)).toEqual(["a", "b"]);
    expect(internal).toHaveLength(0);
  });
});

describe("buildWorkspaceQuickPickItems", () => {
  it("places the clear entry first and marks the active scope", () => {
    const partitioned = partitionWorkspaces([ws("app", "apps/app"), ws("ui", "packages/ui")]);
    const items = buildWorkspaceQuickPickItems(partitioned, "ui");

    expect(items[0].kind).toBe("clear");
    expect(items[0].name).toBe(CLEAR_WORKSPACE_SCOPE);
    expect(items[0].description).toBe("Clear scope");

    const uiRow = items.find((i) => i.name === "ui");
    expect(uiRow?.description).toContain("Current scope");

    const appRow = items.find((i) => i.name === "app");
    expect(appRow?.description).toBe("apps/app");
  });

  it("marks the clear entry as current when unscoped", () => {
    const items = buildWorkspaceQuickPickItems(
      partitionWorkspaces([ws("app", "apps/app")]),
      CLEAR_WORKSPACE_SCOPE,
    );
    expect(items[0].description).toBe("Current scope");
  });

  it("adds a separator before internal packages", () => {
    const partitioned = partitionWorkspaces([
      ws("app", "apps/app"),
      ws("@scope/plat", "npm/plat", true),
    ]);
    const items = buildWorkspaceQuickPickItems(partitioned, "");
    const separatorIndex = items.findIndex(
      (i) => i.kind === "separator" && i.label === "Generated packages",
    );
    expect(separatorIndex).toBeGreaterThan(0);
    const platIndex = items.findIndex((i) => i.name === "@scope/plat");
    expect(platIndex).toBeGreaterThan(separatorIndex);
  });

  it("always ends with a refresh row", () => {
    const items = buildWorkspaceQuickPickItems(partitionWorkspaces([ws("a", "a")]), "");
    expect(items[items.length - 1].kind).toBe("refresh");
  });
});

describe("renderWorkspaceStatusBarText", () => {
  it("shows Fallow: All when unscoped", () => {
    expect(renderWorkspaceStatusBarText(CLEAR_WORKSPACE_SCOPE)).toBe("$(layers) Fallow: All");
  });

  it("shows the package name when scoped", () => {
    expect(renderWorkspaceStatusBarText("my-pkg")).toBe("$(layers) my-pkg");
  });
});

describe("renderWorkspaceStatusBarTooltip", () => {
  it("uses neutral, scope-only copy (never implies confirmed defects)", () => {
    const scoped = renderWorkspaceStatusBarTooltip("my-pkg");
    expect(scoped).toContain("scoped to my-pkg");
    expect(scoped.toLowerCase()).not.toContain("vulnerab");
    expect(scoped.toLowerCase()).not.toContain("issues in");

    const all = renderWorkspaceStatusBarTooltip(CLEAR_WORKSPACE_SCOPE);
    expect(all).toContain("whole project");
  });

  it("discloses that editor diagnostics stay project-wide (#906 C3)", () => {
    // Health and Security ARE scoped now (#906 C2); only the LSP gutter stays
    // project-wide, and the disclosure must say so on both tooltip states.
    expect(renderWorkspaceStatusBarTooltip("my-pkg")).toContain(WORKSPACE_SCOPE_DISCLOSURE);
    expect(renderWorkspaceStatusBarTooltip(CLEAR_WORKSPACE_SCOPE)).toContain(
      WORKSPACE_SCOPE_DISCLOSURE,
    );
    expect(WORKSPACE_SCOPE_DISCLOSURE).toContain("Health");
    expect(WORKSPACE_SCOPE_DISCLOSURE).toContain("Security");
    expect(WORKSPACE_SCOPE_DISCLOSURE).toContain("editor diagnostics remain project-wide");
  });
});

describe("clearedScopeToast", () => {
  it("claims whole-project only when the residual scope is truly empty (#906 C4)", () => {
    expect(clearedScopeToast(CLEAR_WORKSPACE_SCOPE)).toContain("Analyzing the whole project");
    expect(clearedScopeToast(CLEAR_WORKSPACE_SCOPE).toLowerCase()).not.toContain(
      "fallow.workspace setting",
    );
  });

  it("reports the residual setting scope when a pinned workspace setting remains", () => {
    // Regression for #906 C4: clearing the picker override while `fallow.workspace`
    // is pinned must NOT falsely claim whole-project.
    const toast = clearedScopeToast("pinned-pkg");
    expect(toast).toContain("pinned-pkg");
    expect(toast).toContain("fallow.workspace setting");
    expect(toast.toLowerCase()).not.toContain("whole project");
  });
});

describe("shouldShowWorkspacePicker", () => {
  const output = (count: number): WorkspacesOutput => ({
    workspace_count: count,
    workspaces: Array.from({ length: count }, (_unused, i) => ({
      name: `pkg-${i}`,
      path: `/repo/pkg-${i}`,
      is_internal_dependency: false,
    })),
  });

  it("hides the picker on a single-package repo (#906 n2)", () => {
    expect(shouldShowWorkspacePicker(output(1))).toBe(false);
  });

  it("hides the picker when no workspaces are listable", () => {
    expect(shouldShowWorkspacePicker(output(0))).toBe(false);
  });

  it("shows the picker on a multi-package monorepo", () => {
    expect(shouldShowWorkspacePicker(output(2))).toBe(true);
  });

  it("keeps the picker shown when the list was never probed (older CLI)", () => {
    expect(shouldShowWorkspacePicker(null)).toBe(true);
  });
});

describe("resolveWorkspaceScope", () => {
  it("prefers the workspaceState override over the setting", () => {
    expect(resolveWorkspaceScope("override-pkg", "setting-pkg")).toBe("override-pkg");
  });

  it("falls back to the setting when no override", () => {
    expect(resolveWorkspaceScope("", "setting-pkg")).toBe("setting-pkg");
    expect(resolveWorkspaceScope(undefined, "setting-pkg")).toBe("setting-pkg");
  });

  it("returns the clear scope when both are empty or unset", () => {
    expect(resolveWorkspaceScope("", "")).toBe(CLEAR_WORKSPACE_SCOPE);
    expect(resolveWorkspaceScope(undefined, undefined)).toBe(CLEAR_WORKSPACE_SCOPE);
  });

  it("trims whitespace-only values to the clear scope", () => {
    expect(resolveWorkspaceScope("   ", "   ")).toBe(CLEAR_WORKSPACE_SCOPE);
    expect(resolveWorkspaceScope("  pkg  ", "")).toBe("pkg");
  });
});

describe("buildAnalysisArgs workspace forwarding", () => {
  it("omits --workspace when scope is empty", () => {
    const { args } = buildAnalysisArgs(baseArgsOptions);
    expect(args).not.toContain("--workspace");
  });

  it("appends --workspace <name> when scoped, not version-gated", () => {
    const { args, skipped } = buildAnalysisArgs({ ...baseArgsOptions, workspace: "my-pkg" });
    const idx = args.indexOf("--workspace");
    expect(idx).toBeGreaterThanOrEqual(0);
    expect(args[idx + 1]).toBe("my-pkg");
    // --workspace is a long-standing global flag, never recorded as skipped
    // even when the CLI version cannot be probed.
    expect(skipped).toHaveLength(0);
  });

  it("forwards --workspace even with a known-old CLI version (no gate)", () => {
    const { args, skipped } = buildAnalysisArgs({
      ...baseArgsOptions,
      workspace: "pkg",
      cliVersion: "2.0.0",
    });
    expect(args).toContain("--workspace");
    expect(skipped.some((s) => s.flag === "--workspace")).toBe(false);
  });
});
