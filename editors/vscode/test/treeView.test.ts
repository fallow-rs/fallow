import { describe, expect, it, vi } from "vitest";

vi.mock("vscode", () => {
  class FakeTreeItem {
    public description: string | undefined;
    public tooltip: string | undefined;
    public contextValue: string | undefined;
    public command: unknown;
    public iconPath: unknown;

    public constructor(
      public readonly label: string,
      public readonly collapsibleState: number
    ) {}
  }

  class FakeEventEmitter<T> {
    public readonly event = vi.fn();
    public fire = vi.fn((_value?: T) => {});
    public dispose = vi.fn();
  }

  class FakeRange {
    public constructor(
      public readonly startLine: number,
      public readonly startCharacter: number,
      public readonly endLine: number,
      public readonly endCharacter: number
    ) {}
  }

  return {
    EventEmitter: FakeEventEmitter,
    Range: FakeRange,
    ThemeIcon: class {
      public constructor(public readonly id: string) {}
    },
    TreeItem: FakeTreeItem,
    TreeItemCollapsibleState: {
      None: 0,
      Collapsed: 1,
    },
    Uri: {
      file: (fsPath: string) => ({ fsPath }),
    },
    workspace: {
      workspaceFolders: [
        {
          uri: {
            fsPath: "/workspace",
          },
        },
      ],
    },
  };
});

import { DeadCodeTreeProvider } from "../src/treeView.js";
import type { FallowCheckResult } from "../src/types.js";

interface TestTreeItem {
  readonly label: string;
  readonly description?: string;
  readonly tooltip?: string;
  readonly command?: {
    readonly command: string;
    readonly arguments: ReadonlyArray<unknown>;
  };
}

interface TestRange {
  readonly startLine: number;
  readonly startCharacter: number;
  readonly endLine: number;
  readonly endCharacter: number;
}

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

const findCategory = (
  categories: ReadonlyArray<TestTreeItem>,
  label: string
): TestTreeItem => {
  const category = categories.find((item) => item.label === label);
  expect(category).toBeDefined();
  return category as TestTreeItem;
};

const firstIssue = (
  provider: DeadCodeTreeProvider,
  category: TestTreeItem
): TestTreeItem => {
  const issues = provider.getChildren(category as never) as TestTreeItem[];
  expect(issues).toHaveLength(1);
  return issues[0] as TestTreeItem;
};

const selectionOf = (item: TestTreeItem): TestRange => {
  expect(item.command?.command).toBe("vscode.open");
  const selection = (
    item.command?.arguments[1] as { selection: TestRange } | undefined
  )?.selection;
  expect(selection).toBeDefined();
  return selection as TestRange;
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
    const privateLeak = firstIssue(
      provider,
      findCategory(categories, "Private Type Leaks (1)")
    );
    const testOnlyDep = firstIssue(
      provider,
      findCategory(categories, "Test-Only Dependencies (1)")
    );
    const boundaryViolation = firstIssue(
      provider,
      findCategory(categories, "Boundary Violations (1)")
    );
    const staleSuppression = firstIssue(
      provider,
      findCategory(categories, "Stale Suppressions (1)")
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
