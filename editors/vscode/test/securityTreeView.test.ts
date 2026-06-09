import { describe, expect, it, vi } from "vitest";

vi.mock("vscode", async () => {
  const { createTreeViewVscodeMock } = await import("./vscodeTreeMock.js");
  return createTreeViewVscodeMock("/workspace");
});

import type { TestRange, TestTreeItem } from "./vscodeTreeMock.js";
import { SecurityTreeProvider } from "../src/securityTreeView.js";
import { OPEN_FILE_COMMAND, type OpenFileCommandArgs } from "../src/openFileCommand.js";
import type { SecurityFinding, SecurityOutput } from "../src/types.js";

interface FakeBadge {
  readonly value: number;
  readonly tooltip: string;
}

type SecurityFindingInput = Omit<SecurityFinding, "candidate" | "finding_id"> &
  Partial<Pick<SecurityFinding, "candidate" | "finding_id">>;

const makeView = (): { badge: FakeBadge | undefined } => ({ badge: undefined });

const finding = (input: SecurityFindingInput): SecurityFinding => ({
  ...input,
  finding_id: input.finding_id ?? `security:${input.path}:${input.line}:${input.kind}`,
  candidate: input.candidate ?? {
    sink: {
      path: input.path,
      line: input.line,
      col: input.col,
      category: input.category,
      cwe: input.cwe,
    },
    boundary: {
      client_server: false,
      cross_module: false,
    },
  },
});

const result = (
  findings: ReadonlyArray<SecurityFindingInput>,
  overrides: Partial<
    Pick<SecurityOutput, "unresolved_edge_files" | "unresolved_callee_sites">
  > = {},
): SecurityOutput => ({
  schema_version: "1",
  security_findings: findings.map(finding),
  unresolved_edge_files: overrides.unresolved_edge_files ?? 0,
  unresolved_callee_sites: overrides.unresolved_callee_sites ?? 0,
});

const selectionOf = (item: TestTreeItem): TestRange => {
  expect(item.command?.command).toBe(OPEN_FILE_COMMAND);
  const args = item.command?.arguments[0] as OpenFileCommandArgs | undefined;
  expect(args).toBeDefined();
  const line = Math.max(0, (args as OpenFileCommandArgs).line - 1);
  const character = Math.max(0, (args as OpenFileCommandArgs).col);
  return {
    startLine: line,
    startCharacter: character,
    endLine: line,
    endCharacter: character,
  };
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
      ]),
    );

    const groups = provider.getChildren() as TestTreeItem[];
    expect(groups).toHaveLength(1);
    const group = groups[0]!;
    expect(group.label).toBe("dangerous-html (CWE-79) (1)");
    expect(group.description).toBe("security candidates to verify");
    expect(group.tooltip).toContain("UNVERIFIED CANDIDATE");
    expect(group.iconPath?.id).toBe("shield");
    expect(group.collapsibleState).toBe(1);

    const findings = provider.getChildren(group as never) as TestTreeItem[];
    expect(findings).toHaveLength(1);
    const item = findings[0]!;

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
      ]),
    );

    const groups = provider.getChildren() as TestTreeItem[];
    expect(groups).toHaveLength(1);
    expect(groups[0]?.label).toBe("client-server-leak (1)");

    const findings = provider.getChildren(groups[0] as never) as TestTreeItem[];
    expect(findings).toHaveLength(1);
    const finding = findings[0]!;
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
    expect(provider.getChildren(hops[2] as never)).toEqual([]);
  });

  it("groups findings by kind and CWE category", () => {
    const provider = new SecurityTreeProvider();
    provider.update(
      result([
        {
          kind: "tainted-sink",
          category: "dangerous-html",
          cwe: 79,
          path: "a.ts",
          line: 1,
          col: 0,
          evidence: "x",
          trace: [],
          actions: [],
        },
        {
          kind: "tainted-sink",
          category: "dangerous-html",
          cwe: 79,
          path: "b.ts",
          line: 2,
          col: 0,
          evidence: "y",
          trace: [],
          actions: [],
        },
        {
          kind: "tainted-sink",
          category: "command-injection",
          cwe: 78,
          path: "c.ts",
          line: 3,
          col: 0,
          evidence: "z",
          trace: [],
          actions: [],
        },
        {
          kind: "client-server-leak",
          path: "d.ts",
          line: 4,
          col: 0,
          evidence: "secret",
          trace: [],
          actions: [],
        },
      ]),
    );

    const groups = provider.getChildren() as TestTreeItem[];
    expect(groups.map((group) => group.label)).toEqual([
      "dangerous-html (CWE-79) (2)",
      "command-injection (CWE-78) (1)",
      "client-server-leak (1)",
    ]);

    const htmlFindings = provider.getChildren(groups[0] as never) as TestTreeItem[];
    expect(htmlFindings.map((finding) => finding.description)).toEqual(["a.ts:1", "b.ts:2"]);
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
      ]),
    );

    expect(view.badge).toMatchObject({ value: 2 });
  });

  it("renders non-zero blind-spot counts as a non-actionable info node", () => {
    const provider = new SecurityTreeProvider();
    const view = makeView();
    provider.setView(view as never);
    provider.update(
      result(
        [
          {
            kind: "tainted-sink",
            path: "a.ts",
            line: 1,
            col: 0,
            evidence: "x",
            trace: [],
            actions: [],
          },
        ],
        { unresolved_edge_files: 2, unresolved_callee_sites: 1 },
      ),
    );

    const items = provider.getChildren() as TestTreeItem[];
    expect(items.map((item) => item.label)).toEqual([
      "tainted-sink (1)",
      "Blind spots: 2 unresolved import edges, 1 unresolved sink site",
    ]);
    const blindSpot = items[1]!;
    expect(blindSpot.description).toBe("not a clean bill of health");
    expect(blindSpot.tooltip).toContain("UNVERIFIED CANDIDATE");
    expect(blindSpot.tooltip).toContain("2 unresolved import edges not analyzed");
    expect(blindSpot.tooltip).toContain("1 unresolved sink site not analyzed");
    expect(blindSpot.iconPath?.id).toBe("info");
    expect(blindSpot.command).toBeUndefined();
    expect(provider.getChildren(blindSpot as never)).toEqual([]);
    expect(view.badge).toMatchObject({ value: 1 });
  });

  it("omits the blind-spot node when both counters are zero", () => {
    const provider = new SecurityTreeProvider();
    provider.update(result([]));

    expect(provider.getChildren()).toEqual([]);
  });
});
