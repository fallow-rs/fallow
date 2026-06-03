import { describe, expect, it } from "vitest";
import {
  buildAnalysisArgs,
  compareVersions,
  parseUnexpectedArgument,
  planDegradation,
  stripArgument,
} from "../src/analysis-utils.js";

const baseOptions = {
  production: false,
  changedSince: "",
  configPath: "",
  dupesMode: "mild" as const,
  dupesThreshold: 0,
  minOccurrences: 2,
  cliVersion: null,
};

describe("buildAnalysisArgs", () => {
  it("emits the combined-analysis flags with dupes mode and threshold", () => {
    expect(buildAnalysisArgs(baseOptions)).toEqual({
      args: [
        "--format",
        "json",
        "--quiet",
        "--skip",
        "health",
        "--dupes-mode",
        "mild",
        "--dupes-threshold",
        "0",
      ],
      skipped: [],
    });
  });

  it("omits --dupes-min-occurrences at the floor so older pinned binaries don't reject it", () => {
    const { args, skipped } = buildAnalysisArgs({ ...baseOptions, minOccurrences: 2 });
    expect(args).not.toContain("--dupes-min-occurrences");
    expect(skipped).toEqual([]);
  });

  it("forwards --dupes-min-occurrences when raised and the CLI version is unknown", () => {
    const { args, skipped } = buildAnalysisArgs({ ...baseOptions, minOccurrences: 3 });
    expect(args[args.indexOf("--dupes-min-occurrences") + 1]).toBe("3");
    expect(skipped).toEqual([]);
  });

  it("forwards --dupes-min-occurrences when the resolved CLI is new enough", () => {
    const { args, skipped } = buildAnalysisArgs({
      ...baseOptions,
      minOccurrences: 3,
      cliVersion: "2.88.0",
    });
    expect(args).toContain("--dupes-min-occurrences");
    expect(skipped).toEqual([]);
  });

  it("omits --dupes-min-occurrences and records the skip when the CLI predates it", () => {
    const { args, skipped } = buildAnalysisArgs({
      ...baseOptions,
      minOccurrences: 5,
      cliVersion: "2.87.0",
    });
    expect(args).not.toContain("--dupes-min-occurrences");
    expect(skipped).toEqual([
      { flag: "--dupes-min-occurrences", requires: "2.88.0", cliVersion: "2.87.0" },
    ]);
  });

  it("appends production, changed-since, and config flags when set", () => {
    const { args } = buildAnalysisArgs({
      ...baseOptions,
      production: true,
      changedSince: "main",
      configPath: "/abs/.fallowrc.json",
    });
    expect(args).toContain("--production");
    expect(args[args.indexOf("--changed-since") + 1]).toBe("main");
    expect(args[args.indexOf("--config") + 1]).toBe("/abs/.fallowrc.json");
  });
});

describe("compareVersions", () => {
  it("orders by major, minor, then patch", () => {
    expect(compareVersions("2.88.0", "2.87.9")).toBeGreaterThan(0);
    expect(compareVersions("2.87.0", "2.88.0")).toBeLessThan(0);
    expect(compareVersions("2.88.0", "2.88.0")).toBe(0);
    expect(compareVersions("10.0.0", "9.99.99")).toBeGreaterThan(0);
  });

  it("treats missing segments as zero and ignores pre-release suffixes", () => {
    expect(compareVersions("2.88", "2.88.0")).toBe(0);
    expect(compareVersions("2.88.0-beta", "2.88.0")).toBe(0);
  });
});

describe("parseUnexpectedArgument", () => {
  it("extracts the offending long flag from a modern clap error", () => {
    expect(
      parseUnexpectedArgument(
        "error: unexpected argument '--dupes-min-occurrences' found tip: a similar argument exists",
      ),
    ).toBe("--dupes-min-occurrences");
  });

  it("extracts the flag from legacy clap 3.x / early-4.x wording", () => {
    expect(
      parseUnexpectedArgument(
        "error: Found argument '--dupes-min-occurrences' which wasn't expected, or isn't valid in this context",
      ),
    ).toBe("--dupes-min-occurrences");
  });

  it("extracts a short flag", () => {
    expect(parseUnexpectedArgument("unexpected argument '-x' found")).toBe("-x");
  });

  it("returns null for unrelated failures", () => {
    expect(parseUnexpectedArgument("fallow exited with code 101: panic")).toBeNull();
  });

  it("ignores a positional unexpected argument that is not a flag", () => {
    expect(parseUnexpectedArgument("unexpected argument 'foo' found")).toBeNull();
  });
});

describe("stripArgument", () => {
  it("drops a space-separated flag and its value", () => {
    expect(
      stripArgument(
        ["--format", "json", "--dupes-min-occurrences", "3", "--quiet"],
        "--dupes-min-occurrences",
      ),
    ).toEqual(["--format", "json", "--quiet"]);
  });

  it("drops an --flag=value spelling", () => {
    expect(
      stripArgument(["--format", "json", "--dupes-min-occurrences=3"], "--dupes-min-occurrences"),
    ).toEqual(["--format", "json"]);
  });

  it("does not consume a following flag as a value", () => {
    expect(stripArgument(["--production", "--quiet"], "--production")).toEqual(["--quiet"]);
  });

  it("returns an equal-length vector when the flag is absent", () => {
    const args = ["--format", "json", "--quiet"];
    expect(stripArgument(args, "--missing")).toEqual(args);
  });
});

describe("planDegradation", () => {
  const argv = ["--format", "json", "--quiet", "--skip", "health", "--dupes-min-occurrences", "3"];

  it("retries with a known version-gated flag stripped (modern wording)", () => {
    const plan = planDegradation("unexpected argument '--dupes-min-occurrences' found", argv);
    expect(plan).toEqual({
      kind: "retry",
      dropped: "--dupes-min-occurrences",
      args: ["--format", "json", "--quiet", "--skip", "health"],
    });
  });

  it("retries against legacy clap wording too", () => {
    const plan = planDegradation(
      "Found argument '--dupes-min-occurrences' which wasn't expected",
      argv,
    );
    expect(plan.kind).toBe("retry");
  });

  it("rethrows when the offending flag is not on the version-gated allowlist", () => {
    // A flag the extension did not intend to be auto-stripped must stay loud, so
    // a real bug or corrupt binary is not silently masked.
    expect(planDegradation("unexpected argument '--dupes-mode' found", argv)).toEqual({
      kind: "rethrow",
    });
  });

  it("rethrows unrelated failures", () => {
    expect(planDegradation("fallow exited with code 101: panic", argv)).toEqual({
      kind: "rethrow",
    });
  });

  it("rethrows when the gated flag is named but not actually present in argv", () => {
    expect(
      planDegradation("unexpected argument '--dupes-min-occurrences' found", ["--format", "json"]),
    ).toEqual({ kind: "rethrow" });
  });
});
