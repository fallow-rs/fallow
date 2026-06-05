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
  class FakeThemeColor {
    public constructor(public readonly id: string) {}
  }
  class FakeMarkdownString {
    public value = "";
    public appendMarkdown(text: string): this {
      this.value += text;
      return this;
    }
  }
  return {
    Range: FakeRange,
    ThemeColor: FakeThemeColor,
    MarkdownString: FakeMarkdownString,
  };
});

import { buildComplexityDecorations, crapExplanation } from "../src/complexityDecorations.js";
import type { ComplexityContribution, HealthFinding } from "../src/types.js";

const contribution = (
  line: number,
  metric: "cyclomatic" | "cognitive",
  kind: ComplexityContribution["kind"],
  weight: number,
  nesting = 0,
): ComplexityContribution => ({ line, col: 0, metric, kind, weight, nesting });

const finding = (overrides: Partial<HealthFinding> = {}): HealthFinding =>
  ({
    path: "src/index.ts",
    name: "parseArgs",
    line: 1,
    col: 0,
    cyclomatic: 13,
    cognitive: 13,
    line_count: 30,
    param_count: 1,
    exceeded: "both",
    severity: "warning",
    actions: [],
    ...overrides,
  }) as HealthFinding;

const root = "/project";
const docPath = "/project/src/index.ts";

describe("buildComplexityDecorations", () => {
  it("matches a finding to the open file by resolved path", () => {
    const f = finding({
      contributions: [contribution(5, "cyclomatic", "if", 1)],
    });
    const result = buildComplexityDecorations([f], docPath, root, { afterText: true });
    expect(result.functions).toHaveLength(1);
    expect(result.contributions).toHaveLength(1);
  });

  it("ignores findings for other files", () => {
    const f = finding({
      path: "src/other.ts",
      contributions: [contribution(5, "cyclomatic", "if", 1)],
    });
    const result = buildComplexityDecorations([f], docPath, root, { afterText: true });
    expect(result.functions).toHaveLength(0);
    expect(result.contributions).toHaveLength(0);
  });

  it("anchors the function spec on the 0-based signature line", () => {
    const f = finding({ line: 10, contributions: [] });
    const result = buildComplexityDecorations([f], docPath, root, { afterText: true });
    // 1-based line 10 -> 0-based 9. The controller turns this into an
    // end-of-line range; the spec only carries the line.
    expect(result.functions[0]?.line).toBe(9);
  });

  it("groups two contributions on the same line into one spec summed by metric", () => {
    // A nested `if`: one cyclomatic (+1) and one cognitive (+2) on the same line.
    const f = finding({
      contributions: [
        contribution(6, "cyclomatic", "if", 1, 0),
        contribution(6, "cognitive", "if", 2, 1),
      ],
    });
    const result = buildComplexityDecorations([f], docPath, root, { afterText: true });
    expect(result.contributions).toHaveLength(1);
    const after = result.contributions[0]?.afterText ?? "";
    // Cognitive is the headline (2), with the dominant kind label.
    expect(after).toContain("+2");
    expect(after).toContain("if");
  });

  it("renders an else-if as a flat +1", () => {
    const f = finding({
      contributions: [
        contribution(8, "cyclomatic", "else-if", 1),
        contribution(8, "cognitive", "else-if", 1),
      ],
    });
    const result = buildComplexityDecorations([f], docPath, root, { afterText: true });
    const after = result.contributions[0]?.afterText ?? "";
    expect(after).toContain("+1");
    expect(after).toContain("else if");
  });

  it("omits inline after-text but keeps the hover when afterText is off", () => {
    const f = finding({
      contributions: [contribution(5, "cognitive", "if", 1)],
    });
    const result = buildComplexityDecorations([f], docPath, root, { afterText: false });
    expect(result.contributions[0]?.afterText).toBeUndefined();
    expect(result.contributions[0]?.hover).toBeDefined();
  });
});

describe("crapExplanation", () => {
  it("explains an untested high-CRAP function and the path to clear it", () => {
    const text = crapExplanation(finding({ cyclomatic: 20, crap: 420, coverage_pct: 0 }));
    expect(text).toContain("CRAP 420");
    expect(text).toContain("cyclomatic 20");
    expect(text).toContain("untested");
    expect(text).toContain("would bring CRAP down to 20");
  });

  it("returns undefined when the finding carries no CRAP", () => {
    expect(crapExplanation(finding({ crap: undefined }))).toBeUndefined();
  });
});
