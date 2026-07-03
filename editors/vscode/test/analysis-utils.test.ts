import { describe, expect, it } from "vitest";
import {
  buildAnalysisArgs,
  buildCleanAnalysisSummary,
  compareVersions,
  countDuplicationGroups,
  parseUnexpectedArgument,
  planDegradation,
  stripArgument,
} from "../src/analysis-utils.js";
import type { FallowCheckResult, FallowDupesResult } from "../src/types.js";

const baseOptions = {
  // `undefined` is the "auto"/defer state: no production flag is forwarded.
  production: undefined as boolean | undefined,
  changedSince: "",
  workspace: "",
  configPath: "",
  dupesMode: undefined,
  dupesThreshold: undefined,
  dupesMinTokens: undefined,
  dupesMinLines: undefined,
  minOccurrences: undefined,
  dupesSkipLocal: undefined,
  dupesCrossLanguage: undefined,
  dupesIgnoreImports: undefined,
  cliVersion: null,
};

const emptyCheck = (): FallowCheckResult => ({
  schema_version: 7,
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
  summary: {
    total_issues: 0,
    unused_files: 0,
    unused_exports: 0,
    unused_types: 0,
    private_type_leaks: 0,
    unused_dependencies: 0,
    unused_enum_members: 0,
    unused_class_members: 0,
    unresolved_imports: 0,
    unlisted_dependencies: 0,
    duplicate_exports: 0,
    type_only_dependencies: 0,
    test_only_dependencies: 0,
    dev_dependencies_in_production: 0,
    circular_dependencies: 0,
    boundary_violations: 0,
    stale_suppressions: 0,
    unused_catalog_entries: 0,
    empty_catalog_groups: 0,
    unresolved_catalog_references: 0,
    unused_dependency_overrides: 0,
    misconfigured_dependency_overrides: 0,
  },
});

const dupesResult = (
  cloneGroups: number,
  totalFiles: number,
  duplicationPercentage: number,
): FallowDupesResult => ({
  clone_groups: [],
  clone_families: [],
  stats: {
    total_files: totalFiles,
    files_with_clones: cloneGroups > 0 ? 1 : 0,
    total_lines: 100,
    duplicated_lines: duplicationPercentage > 0 ? 10 : 0,
    total_tokens: 1000,
    duplicated_tokens: duplicationPercentage > 0 ? 100 : 0,
    clone_groups: cloneGroups,
    clone_instances: cloneGroups * 2,
    duplication_percentage: duplicationPercentage,
    clone_groups_below_min_occurrences: 0,
  },
});

describe("buildAnalysisArgs", () => {
  it("does not emit duplication overrides when VS Code settings are unconfigured", () => {
    expect(buildAnalysisArgs(baseOptions)).toEqual({
      args: ["--format", "json", "--quiet", "--skip", "health"],
      skipped: [],
    });
  });

  it("forwards every configured duplication knob when enabled", () => {
    const { args, skipped } = buildAnalysisArgs({
      ...baseOptions,
      dupesMode: "mild",
      dupesThreshold: 0,
      dupesMinTokens: 80,
      dupesMinLines: 8,
      minOccurrences: 3,
      dupesSkipLocal: true,
      dupesCrossLanguage: true,
      dupesIgnoreImports: true,
      cliVersion: "2.88.3",
    });

    expect(args).toEqual([
      "--format",
      "json",
      "--quiet",
      "--skip",
      "health",
      "--dupes-mode",
      "mild",
      "--dupes-threshold",
      "0",
      "--dupes-min-tokens",
      "80",
      "--dupes-min-lines",
      "8",
      "--dupes-min-occurrences",
      "3",
      "--dupes-skip-local",
      "--dupes-cross-language",
      "--dupes-ignore-imports",
    ]);
    expect(skipped).toEqual([]);
  });

  it("forwards --dupes-no-ignore-imports when the user opts out (false) on a new CLI", () => {
    const { args, skipped } = buildAnalysisArgs({
      ...baseOptions,
      dupesIgnoreImports: false,
      cliVersion: "2.96.0",
    });
    expect(args).toContain("--dupes-no-ignore-imports");
    expect(args).not.toContain("--dupes-ignore-imports");
    expect(skipped).toEqual([]);
  });

  it("skips --dupes-no-ignore-imports on an older CLI (default was already count-imports)", () => {
    const { args, skipped } = buildAnalysisArgs({
      ...baseOptions,
      dupesIgnoreImports: false,
      cliVersion: "2.95.0",
    });
    expect(args).not.toContain("--dupes-no-ignore-imports");
    expect(skipped.some((s) => s.flag === "--dupes-no-ignore-imports")).toBe(true);
  });

  it("forwards neither import flag when the setting is unset", () => {
    const { args } = buildAnalysisArgs({
      ...baseOptions,
      dupesIgnoreImports: undefined,
      cliVersion: "2.96.0",
    });
    expect(args).not.toContain("--dupes-ignore-imports");
    expect(args).not.toContain("--dupes-no-ignore-imports");
  });

  it("forwards --dupes-min-occurrences at the floor when explicitly configured", () => {
    const { args, skipped } = buildAnalysisArgs({ ...baseOptions, minOccurrences: 2 });
    expect(args[args.indexOf("--dupes-min-occurrences") + 1]).toBe("2");
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
      cliVersion: "2.88.3",
    });
    expect(args).toContain("--dupes-min-occurrences");
    expect(skipped).toEqual([]);
  });

  it("omits --dupes-min-occurrences and records the skip when the CLI predates it", () => {
    const { args, skipped } = buildAnalysisArgs({
      ...baseOptions,
      dupesMinTokens: 50,
      dupesMinLines: 5,
      minOccurrences: 5,
      dupesSkipLocal: true,
      cliVersion: "2.87.0",
    });
    expect(args).not.toContain("--dupes-min-tokens");
    expect(args).not.toContain("--dupes-min-lines");
    expect(args).not.toContain("--dupes-min-occurrences");
    expect(args).not.toContain("--dupes-skip-local");
    expect(skipped).toEqual([
      { flag: "--dupes-min-tokens", requires: "2.88.3", cliVersion: "2.87.0" },
      { flag: "--dupes-min-lines", requires: "2.88.3", cliVersion: "2.87.0" },
      { flag: "--dupes-min-occurrences", requires: "2.88.0", cliVersion: "2.87.0" },
      { flag: "--dupes-skip-local", requires: "2.88.3", cliVersion: "2.87.0" },
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

  it("forwards neither production flag when deferring to the project config (#1055)", () => {
    const { args } = buildAnalysisArgs({ ...baseOptions, production: undefined });
    expect(args).not.toContain("--production");
    expect(args).not.toContain("--no-production");
  });

  it("forwards --no-production to force production off (#1055)", () => {
    const { args, skipped } = buildAnalysisArgs({
      ...baseOptions,
      production: false,
      cliVersion: "2.90.0",
    });
    expect(args).toContain("--no-production");
    expect(args).not.toContain("--production");
    expect(skipped).toEqual([]);
  });

  it("omits --no-production and reports the skip when the resolved CLI is too old", () => {
    const { args, skipped } = buildAnalysisArgs({
      ...baseOptions,
      production: false,
      cliVersion: "2.89.0",
    });
    expect(args).not.toContain("--no-production");
    expect(skipped).toEqual([
      { flag: "--no-production", requires: "2.90.0", cliVersion: "2.89.0" },
    ]);
  });

  it("forwards --no-production optimistically when the CLI version is unknown", () => {
    const { args } = buildAnalysisArgs({
      ...baseOptions,
      production: false,
      cliVersion: null,
    });
    expect(args).toContain("--no-production");
  });
});

describe("buildCleanAnalysisSummary", () => {
  it("summarizes a clean dead-code and duplication run without claiming non-JS languages", () => {
    const summary = buildCleanAnalysisSummary(emptyCheck(), dupesResult(0, 63, 0));

    expect(summary.notification).toBe(
      "Fallow: no issues found in analyzed JS/TS files (63 analyzed JS/TS files).",
    );
    expect(summary.outputLines).toEqual([
      "Fallow analysis summary:",
      "- Dead code: no issues found in analyzed JS/TS files.",
      "- Duplication: no duplicate-code groups found across 63 analyzed JS/TS files (0% duplicated lines).",
    ]);
  });

  it("does not imply duplication passed when dupes output is unavailable", () => {
    const summary = buildCleanAnalysisSummary(emptyCheck(), null);

    expect(summary.notification).toBe(
      "Fallow: no dead-code issues found in analyzed JS/TS files. Duplication summary unavailable.",
    );
    expect(summary.outputLines).toContain("- Duplication: summary unavailable.");
  });

  it("does not imply dead-code analysis passed when check output is unavailable", () => {
    const summary = buildCleanAnalysisSummary(null, null);

    expect(summary.notification).toBe(
      "Fallow: analysis completed, but no dead-code summary was available.",
    );
    expect(summary.outputLines).toContain("- Dead code: summary unavailable.");
  });
});

describe("countDuplicationGroups", () => {
  it("uses duplication stats for duplicate-group notification routing", () => {
    expect(countDuplicationGroups(dupesResult(2, 5, 12.5))).toBe(2);
    expect(countDuplicationGroups(null)).toBe(0);
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

  it("retries with complexity breakdown stripped for older CLIs", () => {
    const plan = planDegradation("unexpected argument '--complexity-breakdown' found", [
      "health",
      "--format",
      "json",
      "--quiet",
      "--complexity-breakdown",
    ]);
    expect(plan).toEqual({
      kind: "retry",
      dropped: "--complexity-breakdown",
      args: ["health", "--format", "json", "--quiet"],
    });
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
