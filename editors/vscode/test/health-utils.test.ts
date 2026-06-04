import { describe, expect, it } from "vitest";
import {
  buildHealthArgs,
  countHealthItems,
  formatHealthStatusPart,
  formatHotspotDescription,
  formatScoreLabel,
  gradeIcon,
  gradeThemeColor,
  severityIcon,
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
  it("maps severities to distinct codicons with a fallback", () => {
    expect(severityIcon("critical")).toBe("error");
    expect(severityIcon("high")).toBe("warning");
    expect(severityIcon("moderate")).toBe("info");
    expect(severityIcon("unknown")).toBe("circle-outline");
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
