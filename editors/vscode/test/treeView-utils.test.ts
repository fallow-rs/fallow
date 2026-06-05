import { describe, expect, it } from "vitest";
import {
  ELLIPSIS,
  middleElidePath,
  resolveFilePath,
  sortCloneGroupsBySize,
} from "../src/treeView-utils.js";

describe("resolveFilePath", () => {
  it("returns empty strings when the input path is undefined", () => {
    // Regression for issue #323: stale type for UnlistedDependency caused the
    // tree view to pass undefined into path.* and crash the extension.
    expect(resolveFilePath(undefined, "/workspace")).toEqual({
      absolute: "",
      relative: "",
    });
  });

  it("returns empty strings when the input path is empty", () => {
    expect(resolveFilePath("", "/workspace")).toEqual({
      absolute: "",
      relative: "",
    });
  });

  it("resolves a relative path against the workspace root", () => {
    expect(resolveFilePath("src/foo.ts", "/workspace")).toEqual({
      absolute: "/workspace/src/foo.ts",
      relative: "src/foo.ts",
    });
  });

  it("keeps an absolute path absolute and computes a relative form", () => {
    expect(resolveFilePath("/workspace/src/foo.ts", "/workspace")).toEqual({
      absolute: "/workspace/src/foo.ts",
      relative: "src/foo.ts",
    });
  });

  it("falls back to the raw path when no workspace root is provided", () => {
    expect(resolveFilePath("src/foo.ts", undefined)).toEqual({
      absolute: "src/foo.ts",
      relative: "src/foo.ts",
    });
  });
});

describe("middleElidePath", () => {
  it("returns the path unchanged when it already fits the budget", () => {
    expect(middleElidePath("src/foo.ts", 40)).toBe("src/foo.ts");
  });

  it("keeps the first segment and the basename, collapsing the middle", () => {
    // The basename (most identifying part) and the leading context both survive.
    const out = middleElidePath("a/b/c/d/VeryLongFileName.tsx", 24);
    expect(out).toBe(`a/${ELLIPSIS}/VeryLongFileName.tsx`);
    expect(out.startsWith("a/")).toBe(true);
    expect(out.endsWith("/VeryLongFileName.tsx")).toBe(true);
  });

  it("grows the kept tail while it still fits the budget", () => {
    const out = middleElidePath("dashboard/src/app/components/FunctionTable.tsx", 40);
    expect(out.startsWith("dashboard/")).toBe(true);
    expect(out.endsWith("/FunctionTable.tsx")).toBe(true);
    expect(out).toContain(ELLIPSIS);
    expect(out.length).toBeLessThanOrEqual(40);
  });

  it("falls back to a character-level head+tail elide when no interior fits", () => {
    // Single very long segment: no path separators to collapse, so keep both ends.
    const out = middleElidePath("AnExtremelyLongSingleSegmentFileNameWithoutSlashes.tsx", 20);
    expect(out).toContain(ELLIPSIS);
    expect(out.length).toBeLessThanOrEqual(20);
    expect(out.startsWith("AnExtreme")).toBe(true);
    expect(out.endsWith(".tsx")).toBe(true);
  });

  it("never truncates a trailing :line because callers append it after eliding", () => {
    const line = 314;
    const display = `${middleElidePath("dashboard/src/components/FunctionTable.tsx", 40)}:${line}`;
    expect(display.endsWith(`:${line}`)).toBe(true);
  });
});

describe("sortCloneGroupsBySize", () => {
  const group = (line_count: number, instances: number) => ({
    line_count,
    instances: Array.from({ length: instances }, (_, i) => i),
  });

  it("orders by total duplicated lines (line_count x instances) descending", () => {
    const sorted = sortCloneGroupsBySize([group(11, 2), group(75, 2), group(9, 2)]);
    expect(sorted.map((g) => g.line_count)).toEqual([75, 11, 9]);
  });

  it("breaks ties on equal impact by line_count descending", () => {
    // 12x2 = 24 impact, 8x3 = 24 impact: same impact, larger line_count first.
    const sorted = sortCloneGroupsBySize([group(8, 3), group(12, 2)]);
    expect(sorted.map((g) => g.line_count)).toEqual([12, 8]);
  });

  it("does not mutate the input array", () => {
    const input = [group(9, 2), group(50, 2)];
    const order = input.map((g) => g.line_count);
    sortCloneGroupsBySize(input);
    expect(input.map((g) => g.line_count)).toEqual(order);
  });
});
