import { describe, expect, it } from "vitest";
import {
  buildFixArgs,
  createFixPreviewItems,
  resolveFixLocation,
  zeroAppliedAdvice,
} from "../src/fix-utils.js";
import type { FallowFixResult, FixAction } from "../src/types.js";

describe("buildFixArgs", () => {
  it("builds dry-run arguments without production mode", () => {
    expect(buildFixArgs(true, false, false, null).args).toEqual([
      "fix",
      "--dry-run",
      "--format",
      "json",
      "--quiet",
    ]);
  });

  it("appends production mode when enabled", () => {
    expect(buildFixArgs(false, true, false, null).args).toEqual([
      "fix",
      "--yes",
      "--format",
      "json",
      "--quiet",
      "--production",
    ]);
  });

  it("includes --allow-remote-extends when the setting is on", () => {
    const { args, skipped } = buildFixArgs(true, false, true, null);
    expect(args).toContain("--allow-remote-extends");
    expect(skipped).toEqual([]);
  });

  it("skips --allow-remote-extends on a CLI that predates it", () => {
    const { args, skipped } = buildFixArgs(true, false, true, "3.4.1");
    expect(args).not.toContain("--allow-remote-extends");
    expect(skipped).toEqual([
      { flag: "--allow-remote-extends", requires: "3.4.2", cliVersion: "3.4.1" },
    ]);
  });
});

describe("createFixPreviewItems", () => {
  it("preserves distinct fixes even when labels collide", () => {
    const fixes: FixAction[] = [
      {
        type: "remove_export",
        name: "duplicateName",
        path: "src/first.ts",
        line: 2,
      },
      {
        type: "remove_export",
        name: "duplicateName",
        path: "src/second.ts",
        line: 8,
      },
    ];

    const items = createFixPreviewItems(fixes);

    expect(items).toHaveLength(3);
    expect(items[0]).toMatchObject({
      action: "navigate",
      label: "duplicateName",
      detail: "src/first.ts:2",
    });
    expect(items[1]).toMatchObject({
      action: "navigate",
      label: "duplicateName",
      detail: "src/second.ts:8",
    });
    expect(items[0].action === "navigate" ? items[0].fix : undefined).toBe(
      fixes[0]
    );
    expect(items[1].action === "navigate" ? items[1].fix : undefined).toBe(
      fixes[1]
    );
    expect(items[2]).toMatchObject({
      action: "apply-all",
      label: "Apply all fixes",
      description: "2 fixes",
    });
  });
});

describe("resolveFixLocation", () => {
  it("resolves relative paths against the workspace root", () => {
    expect(
      resolveFixLocation("/workspace/project", {
        type: "remove_export",
        path: "src/index.ts",
        line: 5,
      })
    ).toEqual({
      absolutePath: "/workspace/project/src/index.ts",
      line: 4,
    });
  });

  it("keeps absolute paths unchanged and clamps line numbers", () => {
    expect(
      resolveFixLocation("/workspace/project", {
        type: "remove_export",
        file: "/tmp/file.ts",
        line: 0,
      })
    ).toEqual({
      absolutePath: "/tmp/file.ts",
      line: 0,
    });
  });

  it("returns null when no path information exists", () => {
    expect(
      resolveFixLocation("/workspace/project", {
        type: "remove_dependency",
        package: "left-pad",
      })
    ).toBeNull();
  });
});

describe("zeroAppliedAdvice", () => {
  const skipped = (help: string): FixAction =>
    ({ type: "remove_export", path: "src/a.ts", skipped: true, help }) as FixAction;
  const result = (fixes: FixAction[], totalFixed = 0, dryRun = false): FallowFixResult => ({
    dry_run: dryRun,
    fixes,
    total_fixed: totalFixed,
  });

  it("shows the help that held back the most fixes", () => {
    const advice = zeroAppliedAdvice(
      result([skipped("Add `!.claude/**`."), skipped("Add `!.claude/**`."), skipped("Other.")]),
    );
    expect(advice).toBe(
      "Add `!.claude/**`. 1 other reason also held back fixes. Run `fallow fix --dry-run` in a terminal to see each one.",
    );
  });

  it("shows a single reason as it is", () => {
    expect(zeroAppliedAdvice(result([skipped("Add `!.claude/**`.")]))).toBe("Add `!.claude/**`.");
  });

  it("stays silent when a fix was applied or nothing has help", () => {
    expect(zeroAppliedAdvice(result([skipped("x")], 1))).toBeNull();
    expect(zeroAppliedAdvice(result([skipped("x")], 0, true))).toBeNull();
    expect(zeroAppliedAdvice(result([]))).toBeNull();
  });
});
