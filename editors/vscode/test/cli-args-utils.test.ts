import { describe, expect, it } from "vitest";
import { buildWorkspacesArgs } from "../src/cli-args-utils.js";

describe("buildWorkspacesArgs", () => {
  it("lists the workspaces as JSON", () => {
    expect(buildWorkspacesArgs(false, null)).toEqual({
      args: ["workspaces", "--format", "json", "--quiet"],
      skipped: [],
    });
  });

  it("includes --allow-remote-extends when the setting is on", () => {
    const { args, skipped } = buildWorkspacesArgs(true, "3.4.2");
    expect(args).toContain("--allow-remote-extends");
    expect(skipped).toEqual([]);
  });

  it("skips --allow-remote-extends on a CLI that predates it", () => {
    const { args, skipped } = buildWorkspacesArgs(true, "3.4.1");
    expect(args).not.toContain("--allow-remote-extends");
    expect(skipped).toEqual([
      { flag: "--allow-remote-extends", requires: "3.4.2", cliVersion: "3.4.1" },
    ]);
  });
});
