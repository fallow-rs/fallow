import { describe, expect, it, vi } from "vitest";

vi.mock("vscode", async () => {
  const { createTreeViewVscodeMock } = await import("./vscodeTreeMock.js");
  return createTreeViewVscodeMock("/workspace");
});

import type { TestTreeItem } from "./vscodeTreeMock.js";
import { RuntimeCoverageTreeProvider } from "../src/coverageView.js";
import type { RuntimeCoverageHotPath, RuntimeCoverageReport } from "../src/types.js";

const hotPath = (name: string): RuntimeCoverageHotPath => ({
  id: `fallow:hot:${name}`,
  path: "src/a.ts",
  function: name,
  line: 1,
  end_line: 0,
  invocations: 1,
  percentile: 0,
});

const report = (hotPaths: ReadonlyArray<RuntimeCoverageHotPath>): RuntimeCoverageReport =>
  ({ hot_paths: hotPaths, findings: [] }) as unknown as RuntimeCoverageReport;

describe("RuntimeCoverageTreeProvider", () => {
  it("keeps a stable group id when the group count changes", () => {
    const provider = new RuntimeCoverageTreeProvider();
    provider.update(report([hotPath("a")]));
    const before = (provider.getChildren() as TestTreeItem[])[0];

    provider.update(report([hotPath("a"), hotPath("b")]));
    const after = (provider.getChildren() as TestTreeItem[])[0];

    expect(after?.label).toBe("Hot Paths (2)");
    expect(before?.id).toBeDefined();
    expect(after?.id).toBe(before?.id);
  });
});
