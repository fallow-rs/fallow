import { describe, expect, it, vi } from "vitest";

vi.mock("vscode", async () => {
  const { createTreeViewVscodeMock } = await import("./vscodeTreeMock.js");
  return createTreeViewVscodeMock("/workspace");
});

import { emptyCheck } from "./checkFixtures.js";
import type { TestRange, TestTreeItem } from "./vscodeTreeMock.js";
import { DeadCodeTreeProvider } from "../src/treeView.js";
import { OPEN_FILE_COMMAND, type OpenFileCommandArgs } from "../src/openFileCommand.js";

const findCategory = (categories: ReadonlyArray<TestTreeItem>, label: string): TestTreeItem => {
  const category = categories.find((item) => item.label === label);
  expect(category).toBeDefined();
  return category as TestTreeItem;
};

const firstIssue = (provider: DeadCodeTreeProvider, category: TestTreeItem): TestTreeItem => {
  const issues = provider.getChildren(category as never) as TestTreeItem[];
  expect(issues).toHaveLength(1);
  return issues[0] as TestTreeItem;
};

const commandArgsOf = (item: TestTreeItem): OpenFileCommandArgs => {
  expect(item.command?.command).toBe(OPEN_FILE_COMMAND);
  const args = item.command?.arguments[0] as OpenFileCommandArgs | undefined;
  expect(args).toBeDefined();
  return args as OpenFileCommandArgs;
};

const selectionOf = (item: TestTreeItem): TestRange => {
  const args = commandArgsOf(item);
  return {
    startLine: Math.max(0, args.line - 1),
    startCharacter: Math.max(0, args.col),
    endLine: Math.max(0, (args.endLine ?? args.line) - 1),
    endCharacter: Math.max(0, args.endCol ?? args.col),
  };
};

describe("DeadCodeTreeProvider", () => {
  it("renders new schema categories and navigates to their reported locations", () => {
    const provider = new DeadCodeTreeProvider();
    provider.update({
      ...emptyCheck(),
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
          package_name: "vitest",
          path: "package.json",
          line: 7,
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
          col: 4,
          actions: [],
        },
      ],
      stale_suppressions: [
        {
          path: "src/index.ts",
          line: 5,
          col: 2,
          origin: {
            type: "comment",
            issue_kind: "unused-export",
            is_file_level: false,
          },
        },
      ],
    });

    const categories = provider.getChildren() as TestTreeItem[];
    const privateLeak = firstIssue(provider, findCategory(categories, "Private Type Leaks (1)"));
    const testOnlyDep = firstIssue(
      provider,
      findCategory(categories, "Test-Only Dependencies (1)"),
    );
    const boundaryViolation = firstIssue(
      provider,
      findCategory(categories, "Boundary Violations (1)"),
    );
    const staleSuppression = firstIssue(
      provider,
      findCategory(categories, "Stale Suppressions (1)"),
    );

    expect(privateLeak.label).toBe("makeWidget -> WidgetState");
    expect(privateLeak.description).toBe("api.ts:2");
    expect(selectionOf(privateLeak)).toMatchObject({
      startLine: 1,
      startCharacter: 9,
    });

    expect(testOnlyDep.label).toBe("vitest");
    expect(testOnlyDep.description).toBe("package.json:7");
    expect(selectionOf(testOnlyDep)).toMatchObject({
      startLine: 6,
      startCharacter: 0,
    });

    expect(boundaryViolation.label).toBe("ui -> data");
    expect(boundaryViolation.description).toBe("ui/button.ts:3");
    expect(selectionOf(boundaryViolation)).toMatchObject({
      startLine: 2,
      startCharacter: 4,
    });

    expect(staleSuppression.label).toBe("unused-export");
    expect(staleSuppression.description).toBe("src/index.ts:5");
    expect(selectionOf(staleSuppression)).toMatchObject({
      startLine: 4,
      startCharacter: 2,
    });
  });

  it("keeps bracketed dynamic route paths decoded in open commands", () => {
    const provider = new DeadCodeTreeProvider();
    provider.update({
      ...emptyCheck(),
      unused_files: [
        {
          path: "src/app/[productId]/page.tsx",
          actions: [],
        },
      ],
    });

    const categories = provider.getChildren() as TestTreeItem[];
    const dynamicRoute = firstIssue(provider, findCategory(categories, "Unused Files (1)"));
    const args = commandArgsOf(dynamicRoute);

    expect(dynamicRoute.description).toBe("src/app/[productId]/page.tsx:1");
    expect(args.absolutePath).toBe("/workspace/src/app/[productId]/page.tsx");
    expect(args.absolutePath).not.toContain("%5B");
    expect(args.absolutePath).not.toContain("%5D");
  });

  it("labels stale suppressions by origin variant", () => {
    const provider = new DeadCodeTreeProvider();
    provider.update({
      ...emptyCheck(),
      stale_suppressions: [
        {
          path: "a.ts",
          line: 1,
          col: 0,
          origin: { type: "jsdoc_tag", export_name: "Widget" },
        },
        {
          path: "b.ts",
          line: 2,
          col: 0,
          origin: {
            type: "comment",
            issue_kind: "unused-export",
            is_file_level: true,
          },
        },
        {
          path: "c.ts",
          line: 3,
          col: 0,
          origin: { type: "comment", is_file_level: false },
        },
        {
          path: "d.ts",
          line: 4,
          col: 0,
          origin: { type: "comment", is_file_level: true },
        },
      ],
    });

    const categories = provider.getChildren() as TestTreeItem[];
    const category = findCategory(categories, "Stale Suppressions (4)");
    const issues = provider.getChildren(category as never) as TestTreeItem[];
    expect(issues.map((i) => i.label)).toEqual([
      "@expected-unused Widget",
      "file unused-export",
      "line suppression",
      "file suppression",
    ]);
  });
});
