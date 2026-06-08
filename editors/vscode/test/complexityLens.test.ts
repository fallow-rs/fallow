import { describe, expect, it, vi } from "vitest";

vi.mock("vscode", () => {
  class FakeRange {
    public constructor(
      public readonly startLine: number,
      public readonly startCharacter: number,
      public readonly endLine: number,
      public readonly endCharacter: number,
    ) {}
  }
  class FakeCodeLens {
    public constructor(
      public readonly range: FakeRange,
      public readonly command: { title: string; command: string; arguments?: unknown[] },
    ) {}
  }
  return { Range: FakeRange, CodeLens: FakeCodeLens };
});

let mockLensEnabled = true;
let mockBreakdownEnabled = true;
vi.mock("../src/config.js", () => ({
  getHealthInlineComplexity: () => mockLensEnabled,
  getComplexityBreakdownEnabled: () => mockBreakdownEnabled,
}));

import {
  ComplexityLensProvider,
  TOGGLE_COMPLEXITY_BREAKDOWN_COMMAND,
} from "../src/complexityLens.js";
import type { HealthFinding } from "../src/types.js";

const finding = (overrides: Partial<HealthFinding> = {}): HealthFinding =>
  ({
    path: "src/index.ts",
    name: "parseArgs",
    line: 10,
    col: 0,
    cyclomatic: 13,
    cognitive: 9,
    line_count: 30,
    param_count: 1,
    exceeded: "both",
    severity: "warning",
    actions: [],
    ...overrides,
  }) as HealthFinding;

interface FakeController {
  onDidChange: unknown;
  isStale: () => boolean;
  findingsForDocument: () => readonly HealthFinding[];
  isExpanded: (path: string, line: number) => boolean;
}

const makeController = (
  findings: readonly HealthFinding[],
  expanded: ReadonlySet<string> = new Set(),
  stale = false,
): FakeController => ({
  onDidChange: () => ({ dispose: () => undefined }),
  isStale: () => stale,
  findingsForDocument: () => findings,
  isExpanded: (path, line) => expanded.has(`${path}:${line}`),
});

const fileDoc = { uri: { scheme: "file", fsPath: "/p/src/index.ts" } } as never;

describe("ComplexityLensProvider", () => {
  it("emits one lens per finding with a show-breakdown toggle when not pinned", () => {
    const provider = new ComplexityLensProvider(makeController([finding()]) as never);
    const lenses = provider.provideCodeLenses(fileDoc);
    expect(lenses).toHaveLength(1);
    const command = (lenses[0] as { command: { title: string; command: string; arguments: unknown[] } })
      .command;
    expect(command.title).toBe("parseArgs: 13 cyc, 9 cog · show breakdown");
    expect(command.command).toBe(TOGGLE_COMPLEXITY_BREAKDOWN_COMMAND);
    expect(command.arguments).toEqual([{ path: "src/index.ts", line: 10 }]);
  });

  it("flips the toggle to hide-breakdown when the function is expanded (pinned or selected)", () => {
    const provider = new ComplexityLensProvider(
      makeController([finding()], new Set(["src/index.ts:10"])) as never,
    );
    const [lens] = provider.provideCodeLenses(fileDoc);
    expect((lens as { command: { title: string } }).command.title).toContain("hide breakdown");
  });

  it("returns no lenses when the inline-complexity setting is off", () => {
    mockLensEnabled = false;
    const provider = new ComplexityLensProvider(makeController([finding()]) as never);
    expect(provider.provideCodeLenses(fileDoc)).toHaveLength(0);
    mockLensEnabled = true;
  });

  it("returns no lenses when the breakdown master switch is off", () => {
    mockBreakdownEnabled = false;
    const provider = new ComplexityLensProvider(makeController([finding()]) as never);
    expect(provider.provideCodeLenses(fileDoc)).toHaveLength(0);
    mockBreakdownEnabled = true;
  });

  it("returns no lenses on a stale document", () => {
    const provider = new ComplexityLensProvider(makeController([finding()], new Set(), true) as never);
    expect(provider.provideCodeLenses(fileDoc)).toHaveLength(0);
  });
});
