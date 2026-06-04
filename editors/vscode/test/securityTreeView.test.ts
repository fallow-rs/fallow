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

import { SecurityTreeProvider } from "../src/securityTreeView.js";
import type { SecurityFinding, SecurityOutput } from "../src/types.js";

interface TestTreeItem {
  readonly label: string;
  readonly description?: string;
  readonly tooltip?: string;
  readonly iconPath?: { readonly id: string };
  readonly collapsibleState: number;
  readonly command?: {
    readonly command: string;
    readonly arguments: ReadonlyArray<unknown>;
  };
}

interface TestRange {
  readonly startLine: number;
  readonly startCharacter: number;
}

interface FakeBadge {
  readonly value: number;
  readonly tooltip: string;
}

const makeView = (): { badge: FakeBadge | undefined } => ({ badge: undefined });

const result = (findings: ReadonlyArray<SecurityFinding>): SecurityOutput => ({
  schema_version: "1",
  security_findings: [...findings],
  unresolved_edge_files: 0,
  unresolved_callee_sites: 0,
});

const selectionOf = (item: TestTreeItem): TestRange => {
  expect(item.command?.command).toBe("vscode.open");
  const selection = (item.command?.arguments[1] as { selection: TestRange } | undefined)?.selection;
  expect(selection).toBeDefined();
  return selection as TestRange;
};

describe("SecurityTreeProvider", () => {
  it("renders no children and clears the badge for a null result", () => {
    const provider = new SecurityTreeProvider();
    const view = makeView();
    provider.setView(view as never);
    provider.update(null);

    expect(provider.getChildren()).toEqual([]);
    expect(view.badge).toBeUndefined();
  });

  it("renders one finding with label, description, navigation, icon, and tooltip framing", () => {
    const provider = new SecurityTreeProvider();
    const view = makeView();
    provider.setView(view as never);
    provider.update(
      result([
        {
          kind: "tainted-sink",
          category: "dangerous-html",
          cwe: 79,
          path: "src/app.tsx",
          line: 12,
          col: 4,
          evidence: "innerHTML reaches req.query.html",
          trace: [],
          actions: [],
        },
      ])
    );

    const items = provider.getChildren() as TestTreeItem[];
    expect(items).toHaveLength(1);
    const item = items[0]!;

    expect(item.label).toBe("dangerous-html (CWE-79)");
    expect(item.description).toBe("src/app.tsx:12");
    expect(item.iconPath?.id).toBe("shield");
    expect(item.tooltip).toContain("UNVERIFIED CANDIDATE");
    expect(item.tooltip).toContain("innerHTML reaches req.query.html");
    expect(selectionOf(item)).toMatchObject({ startLine: 11, startCharacter: 4 });
    // No trace -> not collapsible.
    expect(item.collapsibleState).toBe(0);
    expect(view.badge).toMatchObject({ value: 1 });
  });

  it("renders trace hops as navigable children with role descriptions", () => {
    const provider = new SecurityTreeProvider();
    provider.update(
      result([
        {
          kind: "client-server-leak",
          path: "src/app.tsx",
          line: 12,
          col: 0,
          evidence: "imports a server-only secret",
          trace: [
            { path: "src/app.tsx", line: 12, col: 0, role: "client-boundary" },
            { path: "src/lib/wrap.ts", line: 4, col: 2, role: "intermediate" },
            { path: "src/lib/secret.ts", line: 8, col: 0, role: "secret-source" },
          ],
          actions: [],
        },
      ])
    );

    const items = provider.getChildren() as TestTreeItem[];
    expect(items).toHaveLength(1);
    const finding = items[0]!;
    expect(finding.label).toBe("client-server-leak");
    expect(finding.collapsibleState).toBe(1);

    const hops = provider.getChildren(finding as never) as TestTreeItem[];
    expect(hops).toHaveLength(3);
    expect(hops.map((h) => h.label)).toEqual([
      "src/app.tsx:12",
      "src/lib/wrap.ts:4",
      "src/lib/secret.ts:8",
    ]);
    expect(hops.map((h) => h.description)).toEqual([
      "client boundary",
      "intermediate",
      "secret source",
    ]);
    expect(selectionOf(hops[2]!)).toMatchObject({ startLine: 7, startCharacter: 0 });
  });

  it("sets the badge to the finding count", () => {
    const provider = new SecurityTreeProvider();
    const view = makeView();
    provider.setView(view as never);
    provider.update(
      result([
        {
          kind: "tainted-sink",
          path: "a.ts",
          line: 1,
          col: 0,
          evidence: "x",
          trace: [],
          actions: [],
        },
        {
          kind: "tainted-sink",
          path: "b.ts",
          line: 2,
          col: 0,
          evidence: "y",
          trace: [],
          actions: [],
        },
      ])
    );

    expect(view.badge).toMatchObject({ value: 2 });
  });
});
