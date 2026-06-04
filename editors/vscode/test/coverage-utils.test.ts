import { describe, expect, it } from "vitest";
import {
  buildCoverageArgs,
  buildCoverageGateMessage,
  countCoverageItems,
  COVERAGE_ANALYZE_MIN_VERSION,
  coverageWatermarkMessage,
  formatConfidence,
  sortHotPaths,
  splitCleanupCandidates,
} from "../src/coverage-utils.js";
import type {
  RuntimeCoverageFinding,
  RuntimeCoverageHotPath,
  RuntimeCoverageReport,
  RuntimeCoverageVerdict,
  RuntimeCoverageWatermark,
} from "../src/types.js";

const hotPath = (overrides: Partial<RuntimeCoverageHotPath>): RuntimeCoverageHotPath => ({
  id: "fallow:hot:0",
  path: "src/a.ts",
  function: "fn",
  line: 1,
  end_line: 0,
  invocations: 0,
  percentile: 0,
  ...overrides,
});

const finding = (
  verdict: RuntimeCoverageVerdict,
  overrides: Partial<RuntimeCoverageFinding> = {},
): RuntimeCoverageFinding => ({
  id: "fallow:prod:0",
  path: "src/a.ts",
  function: "fn",
  line: 1,
  verdict,
  confidence: "high",
  evidence: {
    static_status: "unused",
    test_coverage: "not_covered",
    v8_tracking: "tracked",
    observation_days: 7,
    deployments_observed: 3,
  },
  ...overrides,
});

const report = (overrides: Partial<RuntimeCoverageReport>): RuntimeCoverageReport =>
  ({
    schema_version: "1",
    verdict: "clean",
    summary: { data_source: "local" },
    blast_radius: [],
    importance: [],
    ...overrides,
  }) as RuntimeCoverageReport;

describe("buildCoverageArgs", () => {
  it("emits the base local-mode argv", () => {
    expect(
      buildCoverageArgs({ capturePath: "/cap", production: false, top: 0, configPath: "" }),
    ).toEqual([
      "coverage",
      "analyze",
      "--runtime-coverage",
      "/cap",
      "--format",
      "json",
      "--quiet",
    ]);
  });

  it("appends --production only when set", () => {
    const args = buildCoverageArgs({
      capturePath: "/cap",
      production: true,
      top: 0,
      configPath: "",
    });
    expect(args).toContain("--production");
  });

  it("appends --top only when greater than zero", () => {
    expect(
      buildCoverageArgs({ capturePath: "/cap", production: false, top: 0, configPath: "" }),
    ).not.toContain("--top");
    expect(
      buildCoverageArgs({ capturePath: "/cap", production: false, top: -1, configPath: "" }),
    ).not.toContain("--top");
    const args = buildCoverageArgs({
      capturePath: "/cap",
      production: false,
      top: 5,
      configPath: "",
    });
    expect(args.slice(args.indexOf("--top"))).toEqual(["--top", "5"]);
  });

  it("appends --config only when non-empty", () => {
    expect(
      buildCoverageArgs({ capturePath: "/cap", production: false, top: 0, configPath: "" }),
    ).not.toContain("--config");
    const args = buildCoverageArgs({
      capturePath: "/cap",
      production: false,
      top: 0,
      configPath: "/cfg.json",
    });
    expect(args.slice(args.indexOf("--config"))).toEqual(["--config", "/cfg.json"]);
  });

  it("never emits --cloud (local-only feature)", () => {
    const args = buildCoverageArgs({
      capturePath: "/cap",
      production: true,
      top: 10,
      configPath: "/cfg.json",
    });
    expect(args).not.toContain("--cloud");
  });
});

describe("splitCleanupCandidates", () => {
  it("partitions safe-to-delete and review-required, excluding other verdicts", () => {
    const r = report({
      findings: [
        finding("safe_to_delete", { function: "del" }),
        finding("review_required", { function: "rev" }),
        finding("low_traffic", { function: "low" }),
        finding("coverage_unavailable", { function: "unavail" }),
        finding("active", { function: "active" }),
        finding("unknown", { function: "unknown" }),
      ],
    });
    const { safeToDelete, reviewRequired } = splitCleanupCandidates(r);
    expect(safeToDelete.map((f) => f.function)).toEqual(["del"]);
    expect(reviewRequired.map((f) => f.function)).toEqual(["rev"]);
  });

  it("yields empty buckets for a null report", () => {
    expect(splitCleanupCandidates(null)).toEqual({ safeToDelete: [], reviewRequired: [] });
  });

  it("yields empty buckets when findings are undefined", () => {
    expect(splitCleanupCandidates(report({}))).toEqual({
      safeToDelete: [],
      reviewRequired: [],
    });
  });
});

describe("sortHotPaths", () => {
  it("sorts by invocations descending", () => {
    const r = report({
      hot_paths: [
        hotPath({ function: "low", invocations: 10 }),
        hotPath({ function: "high", invocations: 100 }),
        hotPath({ function: "mid", invocations: 50 }),
      ],
    });
    expect(sortHotPaths(r).map((h) => h.function)).toEqual(["high", "mid", "low"]);
  });

  it("is stable for ties (preserves input order)", () => {
    const r = report({
      hot_paths: [
        hotPath({ function: "first", invocations: 50 }),
        hotPath({ function: "second", invocations: 50 }),
      ],
    });
    expect(sortHotPaths(r).map((h) => h.function)).toEqual(["first", "second"]);
  });

  it("tolerates end_line === 0", () => {
    const r = report({ hot_paths: [hotPath({ end_line: 0, invocations: 1 })] });
    expect(sortHotPaths(r)).toHaveLength(1);
  });

  it("yields [] for null or undefined hot paths", () => {
    expect(sortHotPaths(null)).toEqual([]);
    expect(sortHotPaths(report({}))).toEqual([]);
  });
});

describe("countCoverageItems", () => {
  it("sums hot paths plus both cleanup buckets", () => {
    const r = report({
      hot_paths: [hotPath({}), hotPath({})],
      findings: [
        finding("safe_to_delete"),
        finding("review_required"),
        finding("review_required"),
        finding("active"),
      ],
    });
    // 2 hot paths + 1 safe + 2 review (active excluded) = 5
    expect(countCoverageItems(r)).toBe(5);
  });

  it("returns 0 for a null report", () => {
    expect(countCoverageItems(null)).toBe(0);
  });
});

describe("COVERAGE_ANALYZE_MIN_VERSION", () => {
  it("is pinned to 2.57.0 (when local coverage analyze --format json shipped)", () => {
    // Pinning higher (e.g. 2.77.0) needlessly rejects valid CLIs 2.57.0-2.76.x.
    expect(COVERAGE_ANALYZE_MIN_VERSION).toBe("2.57.0");
  });
});

describe("buildCoverageGateMessage", () => {
  const envelope = (message: string, exitCode: number): string =>
    JSON.stringify({ error: true, message, exit_code: exitCode });

  it("special-cases the license gate (exit 3) with an activate next-step", () => {
    const out = buildCoverageGateMessage(
      3,
      envelope("Continuous runtime monitoring requires a valid license or trial.", 3),
      "fallow exited with code 3",
    );
    expect(out).toContain("Continuous runtime monitoring requires a valid license or trial.");
    expect(out).toContain("fallow license activate");
  });

  it("special-cases a missing sidecar (exit 4) with a setup next-step", () => {
    const out = buildCoverageGateMessage(
      4,
      envelope("fallow-cov sidecar not found.", 4),
      "fallow exited with code 4",
    );
    expect(out).toContain("fallow-cov sidecar not found.");
    expect(out).toContain("fallow coverage setup");
  });

  it("special-cases an invalid sidecar (exit 5) with a setup next-step", () => {
    const out = buildCoverageGateMessage(
      5,
      envelope("sidecar signature verification failed.", 5),
      "fallow exited with code 5",
    );
    expect(out).toContain("fallow coverage setup");
  });

  it("surfaces the structured message verbatim for other non-zero codes", () => {
    const out = buildCoverageGateMessage(
      2,
      envelope("runtime coverage report was not produced", 2),
      "fallow exited with code 2",
    );
    expect(out).toBe("runtime coverage report was not produced");
  });

  it("falls back to the rejection message when stdout is not structured JSON", () => {
    expect(buildCoverageGateMessage(3, "some stderr noise", "fallow exited with code 3")).toContain(
      "fallow exited with code 3",
    );
    expect(buildCoverageGateMessage(3, "", "fallow exited with code 3")).toContain(
      "fallow license activate",
    );
  });
});

describe("coverageWatermarkMessage", () => {
  const withWatermark = (watermark: RuntimeCoverageWatermark): RuntimeCoverageReport =>
    report({ watermark });

  it("returns null when no watermark is present", () => {
    expect(coverageWatermarkMessage(null)).toBeNull();
    expect(coverageWatermarkMessage(report({}))).toBeNull();
  });

  it("warns about a license-expired grace watermark with a refresh hint", () => {
    const message = coverageWatermarkMessage(withWatermark("license-expired-grace"));
    expect(message).toContain("grace");
    expect(message).toContain("fallow license refresh");
  });

  it("warns about an expired trial watermark", () => {
    const message = coverageWatermarkMessage(withWatermark("trial-expired"));
    expect(message).toContain("trial");
  });

  it("warns generically for an unknown watermark", () => {
    expect(coverageWatermarkMessage(withWatermark("unknown"))).toContain("watermark");
  });
});

describe("formatConfidence", () => {
  it("spaces snake_case confidence values", () => {
    expect(formatConfidence("very_high")).toBe("very high");
  });

  it("leaves single-word values unchanged", () => {
    expect(formatConfidence("high")).toBe("high");
    expect(formatConfidence("low")).toBe("low");
  });
});
