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

import {
  buildComplexityDecorations,
  complexityKey,
  crapExplanation,
  hoverForLine,
} from "../src/complexityDecorations.js";
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
const all = (): boolean => true;
const none = (): boolean => false;

describe("buildComplexityDecorations", () => {
  it("renders per-line detail for an expanded finding in the open file", () => {
    const f = finding({ contributions: [contribution(5, "cyclomatic", "if", 1)] });
    const result = buildComplexityDecorations([f], docPath, root, all);
    expect(result).toHaveLength(1);
    expect(result[0]?.line).toBe(4); // 1-based contribution line 5 -> 0-based 4
    expect(result[0]?.afterText).toContain("if");
  });

  it("renders nothing for a finding that is not expanded", () => {
    const f = finding({ contributions: [contribution(5, "cyclomatic", "if", 1)] });
    expect(buildComplexityDecorations([f], docPath, root, none)).toHaveLength(0);
  });

  it("expands only the functions the predicate selects", () => {
    const a = finding({ name: "parseArgs", contributions: [contribution(5, "cyclomatic", "if", 1)] });
    const b = finding({
      name: "other",
      line: 20,
      contributions: [contribution(25, "cyclomatic", "if", 1)],
    });
    const result = buildComplexityDecorations(
      [a, b],
      docPath,
      root,
      (f) => f.name === "parseArgs",
    );
    expect(result).toHaveLength(1);
    expect(result[0]?.line).toBe(4); // only parseArgs' contribution line
  });

  it("ignores findings for other files even when expanded", () => {
    const f = finding({
      path: "src/other.ts",
      contributions: [contribution(5, "cyclomatic", "if", 1)],
    });
    expect(buildComplexityDecorations([f], docPath, root, all)).toHaveLength(0);
  });

  it("groups two contributions on the same line into one spec summed by metric", () => {
    // A nested `if`: one cyclomatic (+1) and one cognitive (+2) on the same line.
    const f = finding({
      contributions: [
        contribution(6, "cyclomatic", "if", 1, 0),
        contribution(6, "cognitive", "if", 2, 1),
      ],
    });
    const result = buildComplexityDecorations([f], docPath, root, all);
    expect(result).toHaveLength(1);
    const after = result[0]?.afterText ?? "";
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
    const result = buildComplexityDecorations([f], docPath, root, all);
    const after = result[0]?.afterText ?? "";
    expect(after).toContain("+1");
    expect(after).toContain("else if");
  });
});

describe("hoverForLine", () => {
  it("returns the function summary on the signature line", () => {
    const f = finding({ line: 10, contributions: [contribution(12, "cyclomatic", "if", 1)] });
    const md = hoverForLine([f], 10);
    expect(md?.value).toContain("parseArgs");
    expect(md?.value).toContain("cyclomatic 13");
  });

  it("returns the per-line breakdown on a contribution line", () => {
    const f = finding({ line: 10, contributions: [contribution(12, "cognitive", "for", 2, 1)] });
    const md = hoverForLine([f], 12);
    expect(md?.value).toContain("Complexity contributions");
    expect(md?.value).toContain("for loop");
  });

  it("returns undefined on a line with neither a function nor a contribution", () => {
    const f = finding({ line: 10, contributions: [contribution(12, "cyclomatic", "if", 1)] });
    expect(hoverForLine([f], 99)).toBeUndefined();
  });
});

describe("complexityKey", () => {
  it("composes the path and 1-based line so all surfaces agree", () => {
    expect(complexityKey("src/index.ts", 10)).toBe("src/index.ts:10");
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
