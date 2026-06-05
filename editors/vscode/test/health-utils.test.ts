import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { describe, expect, it } from "vitest";
import {
  buildHealthArgs,
  countHealthItems,
  escapeHealthMarkdown,
  formatComplexityOffense,
  formatHealthStatusPart,
  formatHotspotDescription,
  formatScoreLabel,
  gradeIcon,
  gradeThemeColor,
  parseUnknownHealthSubcommand,
  recognizedPenaltyKeys,
  severityIcon,
  severityThemeColor,
  topPenalties,
} from "../src/health-utils.js";
import type { HealthReport, HealthScorePenalties } from "../src/types.js";

const baseArgs = {
  hotspots: false,
  topFindings: 20,
  configPath: "",
  changedSince: "",
  production: false,
};

describe("buildHealthArgs", () => {
  it("always requests the cheap health sections and never --skip", () => {
    const args = buildHealthArgs(baseArgs);
    expect(args).toEqual([
      "health",
      "--format",
      "json",
      "--quiet",
      "--score",
      "--complexity",
      "--targets",
      "--top",
      "20",
    ]);
    expect(args).not.toContain("--skip");
    expect(args).not.toContain("--hotspots");
  });

  it("adds --hotspots only when enabled", () => {
    expect(buildHealthArgs({ ...baseArgs, hotspots: true })).toContain("--hotspots");
    expect(buildHealthArgs({ ...baseArgs, hotspots: false })).not.toContain("--hotspots");
  });

  it("forwards --config, --changed-since, and --production only when set", () => {
    const none = buildHealthArgs(baseArgs);
    expect(none).not.toContain("--config");
    expect(none).not.toContain("--changed-since");
    expect(none).not.toContain("--production");

    const all = buildHealthArgs({
      ...baseArgs,
      hotspots: true,
      configPath: "/repo/.fallowrc.json",
      changedSince: "main",
      production: true,
    });
    expect(all).toEqual([
      "health",
      "--format",
      "json",
      "--quiet",
      "--score",
      "--complexity",
      "--targets",
      "--hotspots",
      "--top",
      "20",
      "--production",
      "--changed-since",
      "main",
      "--config",
      "/repo/.fallowrc.json",
    ]);
  });

  it("floors and omits a non-positive --top", () => {
    expect(buildHealthArgs({ ...baseArgs, topFindings: 7.9 })).toContain("7");
    expect(buildHealthArgs({ ...baseArgs, topFindings: 0 })).not.toContain("--top");
    expect(buildHealthArgs({ ...baseArgs, topFindings: -5 })).not.toContain("--top");
  });

  it("forwards --workspace only when a workspace scope is set (#906 C2)", () => {
    expect(buildHealthArgs(baseArgs)).not.toContain("--workspace");
    expect(buildHealthArgs({ ...baseArgs, workspace: "" })).not.toContain("--workspace");
    const scoped = buildHealthArgs({ ...baseArgs, workspace: "pkg-a" });
    expect(scoped).toContain("--workspace");
    expect(scoped[scoped.indexOf("--workspace") + 1]).toBe("pkg-a");
  });
});

describe("formatScoreLabel", () => {
  it("rounds the score and pairs it with the grade", () => {
    expect(formatScoreLabel(82.4, "B")).toBe("B (82)");
    expect(formatScoreLabel(89.9, "A")).toBe("A (90)");
  });

  it("falls back to a placeholder grade when blank", () => {
    expect(formatScoreLabel(50, "")).toBe("? (50)");
  });

  it("handles a non-finite score safely", () => {
    expect(formatScoreLabel(Number.NaN, "C")).toBe("C (0)");
  });
});

const scoredReport = (score: number, grade: string): HealthReport =>
  ({
    findings: [],
    summary: {} as HealthReport["summary"],
    health_score: { formula_version: 2, score, grade, penalties: {} as HealthScorePenalties },
  }) as HealthReport;

describe("formatHealthStatusPart", () => {
  it("renders the status bar segment from a scored report", () => {
    expect(formatHealthStatusPart(scoredReport(82.4, "B"))).toBe("B (82)");
  });

  it("returns null when there is no score", () => {
    expect(formatHealthStatusPart(null)).toBeNull();
    expect(
      formatHealthStatusPart({ findings: [], summary: {} } as unknown as HealthReport),
    ).toBeNull();
  });
});

describe("gradeIcon", () => {
  it("maps grades to codicons with a safe default", () => {
    expect(gradeIcon("A")).toBe("check");
    expect(gradeIcon("B")).toBe("check");
    expect(gradeIcon("C")).toBe("info");
    expect(gradeIcon("D")).toBe("warning");
    expect(gradeIcon("F")).toBe("warning");
    expect(gradeIcon("Z")).toBe("pulse");
    expect(gradeIcon("")).toBe("pulse");
  });
});

describe("gradeThemeColor", () => {
  it("maps grades to chart theme tokens and null for unknown", () => {
    expect(gradeThemeColor("A")).toBe("charts.green");
    expect(gradeThemeColor("c")).toBe("charts.yellow");
    expect(gradeThemeColor("F")).toBe("charts.red");
    expect(gradeThemeColor("?")).toBeNull();
  });
});

describe("severityIcon", () => {
  it("maps severities to distinct non-error codicons with a fallback", () => {
    // `critical` uses the section's `flame` glyph, not the alarming error `X`,
    // because complexity findings are heuristic candidates, not broken code.
    expect(severityIcon("critical")).toBe("flame");
    expect(severityIcon("high")).toBe("warning");
    expect(severityIcon("moderate")).toBe("info");
    expect(severityIcon("unknown")).toBe("circle-outline");
  });
});

describe("severityThemeColor", () => {
  it("pairs each severity with a distinct neutral chart color, null for unknown", () => {
    expect(severityThemeColor("critical")).toBe("charts.red");
    expect(severityThemeColor("high")).toBe("charts.orange");
    expect(severityThemeColor("moderate")).toBe("charts.blue");
    expect(severityThemeColor("unknown")).toBeNull();
  });
});

describe("formatComplexityOffense", () => {
  it("leads with the function and abbreviates the metrics", () => {
    expect(
      formatComplexityOffense({ name: "parseArgs", cyclomatic: 24, cognitive: 18, crap: 31 }),
    ).toBe("parseArgs · 24 cyc · 18 cog · CRAP 31");
  });

  it("omits the CRAP segment when there is no score", () => {
    expect(
      formatComplexityOffense({ name: "render", cyclomatic: 9, cognitive: 7, crap: null }),
    ).toBe("render · 9 cyc · 7 cog");
    expect(formatComplexityOffense({ name: "render", cyclomatic: 9, cognitive: 7 })).toBe(
      "render · 9 cyc · 7 cog",
    );
  });

  it("rounds the CRAP score to a whole number", () => {
    expect(
      formatComplexityOffense({ name: "f", cyclomatic: 5, cognitive: 4, crap: 30.7 }),
    ).toBe("f · 5 cyc · 4 cog · CRAP 31");
  });
});

describe("topPenalties", () => {
  it("sorts non-zero penalties descending and drops null/zero", () => {
    const penalties: HealthScorePenalties = {
      dead_files: 0,
      dead_exports: null,
      complexity: 5,
      p90_complexity: 0,
      maintainability: 12,
      hotspots: null,
      unused_deps: 3,
      circular_deps: null,
      unit_size: 10,
      coupling: null,
      duplication: 0.1,
    };
    const result = topPenalties(penalties);
    expect(result.map((p) => p.key)).toEqual([
      "Maintainability",
      "Unit size",
      "Complexity",
      "Unused dependencies",
      "Duplication",
    ]);
    expect(result.every((p) => p.points > 0)).toBe(true);
  });

  it("respects the limit and handles missing penalties", () => {
    expect(topPenalties(null)).toEqual([]);
    expect(topPenalties(undefined)).toEqual([]);
    const penalties = { complexity: 5, unit_size: 10, coupling: 3 } as HealthScorePenalties;
    expect(topPenalties(penalties, 2).map((p) => p.key)).toEqual(["Unit size", "Complexity"]);
  });
});

describe("countHealthItems", () => {
  it("sums findings, hotspots, and targets across sparse sections", () => {
    expect(countHealthItems(null)).toBe(0);
    const report = {
      findings: [{}, {}],
      hotspots: undefined,
      targets: [{}],
    } as unknown as HealthReport;
    expect(countHealthItems(report)).toBe(3);
  });
});

describe("formatHotspotDescription", () => {
  it("pluralizes commits and rounds the score", () => {
    expect(formatHotspotDescription(12.6, 1)).toBe("score 13 · 1 commit");
    expect(formatHotspotDescription(4, 7)).toBe("score 4 · 7 commits");
  });
});

describe("escapeHealthMarkdown", () => {
  it("escapes markdown control characters that could break out of a tooltip", () => {
    expect(escapeHealthMarkdown("a*b_c`d")).toBe("a\\*b\\_c\\`d");
    expect(escapeHealthMarkdown("[link](http://x)")).toBe("\\[link\\]\\(http://x\\)");
    expect(escapeHealthMarkdown("a < b > c | d")).toBe("a \\< b \\> c \\| d");
  });

  it("leaves plain text untouched", () => {
    expect(escapeHealthMarkdown("src/foo/bar.ts")).toBe("src/foo/bar\\.ts");
    expect(escapeHealthMarkdown("plain name")).toBe("plain name");
  });
});

describe("parseUnknownHealthSubcommand", () => {
  it("matches modern and legacy clap phrasings for an unknown `health` subcommand", () => {
    expect(parseUnknownHealthSubcommand("error: unrecognized subcommand 'health'")).toBe(true);
    expect(parseUnknownHealthSubcommand("unrecognized subcommand health")).toBe(true);
    expect(parseUnknownHealthSubcommand("The subcommand 'health' wasn't recognized")).toBe(true);
    expect(parseUnknownHealthSubcommand("subcommand health was not recognized")).toBe(true);
  });

  it("returns false for unrelated errors so genuine failures stay loud", () => {
    expect(parseUnknownHealthSubcommand("fallow exited with code 101")).toBe(false);
    expect(parseUnknownHealthSubcommand("unrecognized subcommand 'security'")).toBe(false);
    expect(parseUnknownHealthSubcommand("")).toBe(false);
  });
});

describe("penalty label parity with the HealthScorePenalties wire contract", () => {
  // Parse the field names of the generated HealthScorePenalties interface and
  // assert this module labels every one of them. A new Rust penalty field that
  // flows through codegen but is not added to PENALTY_LABELS would otherwise be
  // silently omitted from the score tooltip.
  const contract = readFileSync(
    resolve(__dirname, "../src/generated/output-contract.d.ts"),
    "utf8",
  );
  const interfaceMatch = contract.match(
    /export interface HealthScorePenalties \{([\s\S]*?)\n\}/,
  );

  it("locates the generated HealthScorePenalties interface", () => {
    expect(interfaceMatch).not.toBeNull();
  });

  it("labels every penalty key emitted on the wire (no silent omissions)", () => {
    const body = interfaceMatch?.[1] ?? "";
    const wireKeys = [...body.matchAll(/^\s*([a-z0-9_]+)\??:/gim)].map((m) => m[1]);
    expect(wireKeys.length).toBeGreaterThan(0);

    const labelled = new Set<string>(recognizedPenaltyKeys);
    const unlabelled = wireKeys.filter((key) => !labelled.has(key));
    expect(unlabelled, `unlabelled penalty wire keys: ${unlabelled.join(", ")}`).toEqual([]);
  });

  it("does not carry stale labels for keys no longer on the wire", () => {
    const body = interfaceMatch?.[1] ?? "";
    const wireKeys = new Set([...body.matchAll(/^\s*([a-z0-9_]+)\??:/gim)].map((m) => m[1]));
    const stale = recognizedPenaltyKeys.filter((key) => !wireKeys.has(key));
    expect(stale, `stale penalty labels: ${stale.join(", ")}`).toEqual([]);
  });
});
