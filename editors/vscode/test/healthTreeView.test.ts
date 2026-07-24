import { describe, expect, it, vi } from "vitest";

vi.mock("vscode", async () => {
  const { createTreeViewVscodeMock } = await import("./vscodeTreeMock.js");
  return createTreeViewVscodeMock("/workspace");
});

import { HealthTreeProvider } from "../src/healthTreeView.js";
import { OPEN_FILE_COMMAND, type OpenFileCommandArgs } from "../src/openFileCommand.js";
import type { HealthOutput } from "../src/types.js";
import type { TestTreeItem } from "./vscodeTreeMock.js";

const commandArgs = (item: TestTreeItem): OpenFileCommandArgs => {
  expect(item.command?.command).toBe(OPEN_FILE_COMMAND);
  return item.command?.arguments[0] as OpenFileCommandArgs;
};

const typeCouplingReport = (): HealthOutput =>
  ({
    findings: [],
    summary: {},
    _meta: {
      type_aware: {
        type_coupling: {
          status: "complete",
          summary: {
            scope: "project-local-public-signatures",
            direction: "directed",
            project_size: 2,
            files_analyzed: 2,
            distinct_coupled_files: 2,
            edge_count: 2,
            coupled_file_pct: 100,
            p50_distinct_connections: 1,
            p90_distinct_connections: 1,
            p95_public_types_used_by: 1,
            p95_public_api_depends_on: 1,
            high_coupling_pct: 100,
            concentration: 1,
            cycle_count: 1,
          },
          files: [
            {
              path: "src/a.ts",
              public_api_depends_on: 1,
              public_types_used_by: 1,
              edges: [
                {
                  source: {
                    path: "src/a.ts",
                    exported_name: "A",
                  },
                  target: {
                    path: "src/b.ts",
                    exported_name: "B",
                  },
                  relation: "return-type",
                  evidence: {
                    path: "src/a.ts",
                    line: 17,
                    col: 9,
                  },
                  scope: "module-export",
                },
              ],
            },
            {
              path: "src/b.ts",
              public_api_depends_on: 1,
              public_types_used_by: 1,
              edges: [
                {
                  source: {
                    path: "src/b.ts",
                    exported_name: "B",
                  },
                  target: {
                    path: "src/a.ts",
                    exported_name: "A",
                  },
                  relation: "parameter-type",
                  evidence: {
                    path: "src/b.ts",
                    line: 23,
                    col: 4,
                  },
                  scope: "module-export",
                },
              ],
            },
          ],
          top_contributors: [],
          cycles: [{ files: ["src/a.ts", "src/b.ts", "src/a.ts"] }],
        },
      },
    },
  }) as unknown as HealthOutput;

describe("HealthTreeProvider type coupling", () => {
  it("navigates contributor evidence and cycle hops to exact semantic locations", () => {
    const provider = new HealthTreeProvider();
    provider.update(typeCouplingReport());

    const sections = provider.getChildren() as TestTreeItem[];
    expect(sections.map((section) => section.label)).toEqual(["Type Coupling (2)"]);

    const leaves = provider.getChildren(sections[0] as never) as TestTreeItem[];
    const contributor = leaves.find((leaf) => leaf.label === "src/a.ts");
    expect(contributor).toBeDefined();
    const contributorEvidence = provider.getChildren(contributor as never) as TestTreeItem[];
    expect(contributorEvidence[0]?.label).toBe("A -> B (return-type)");
    expect(commandArgs(contributorEvidence[0]!)).toMatchObject({
      absolutePath: "/workspace/src/a.ts",
      line: 17,
      col: 9,
    });

    const cycle = leaves.find((leaf) => leaf.label.startsWith("Cycle:"));
    expect(cycle).toBeDefined();
    const hops = provider.getChildren(cycle as never) as TestTreeItem[];
    expect(hops.map((hop) => commandArgs(hop))).toEqual([
      expect.objectContaining({
        absolutePath: "/workspace/src/a.ts",
        line: 17,
        col: 9,
      }),
      expect.objectContaining({
        absolutePath: "/workspace/src/b.ts",
        line: 23,
        col: 4,
      }),
    ]);
  });
});
