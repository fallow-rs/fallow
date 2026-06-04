import type { RuntimeCoverageFinding, RuntimeCoverageHotPath, RuntimeCoverageReport } from "./types.js";

/** First CLI version that ships `fallow coverage analyze --format json`. */
export const COVERAGE_ANALYZE_MIN_VERSION = "2.77.0";

/** Options for building the `coverage analyze` argument vector. */
export interface CoverageArgsOptions {
  /** Absolute path to a local runtime-coverage capture (file or directory). */
  readonly capturePath: string;
  /** Mirror `fallow.production`; appends `--production` when true. */
  readonly production: boolean;
  /** Cap on findings + hot paths (`--top`); `0` (or less) omits the flag. */
  readonly top: number;
  /** Resolved config path; appends `--config <path>` when non-empty. */
  readonly configPath: string;
}

/**
 * Build the argv for a local `fallow coverage analyze` run. Kept pure (no VS
 * Code or config access) so flag-forwarding rules can be unit-tested, mirroring
 * `buildAnalysisArgs`. Local mode is selected purely by `--runtime-coverage`;
 * `--cloud` is deliberately never emitted, so this stays a free, offline,
 * local-capture feature.
 */
export const buildCoverageArgs = (options: CoverageArgsOptions): string[] => {
  const args = [
    "coverage",
    "analyze",
    "--runtime-coverage",
    options.capturePath,
    "--format",
    "json",
    "--quiet",
  ];

  if (options.production) {
    args.push("--production");
  }

  if (options.top > 0) {
    args.push("--top", String(options.top));
  }

  if (options.configPath) {
    args.push("--config", options.configPath);
  }

  return args;
};

/** Cleanup candidates partitioned by verdict. */
export interface CleanupCandidates {
  readonly safeToDelete: readonly RuntimeCoverageFinding[];
  readonly reviewRequired: readonly RuntimeCoverageFinding[];
}

/**
 * Split runtime findings into the two cleanup buckets the editor surfaces.
 * Other verdicts (`low_traffic`, `coverage_unavailable`, `active`, `unknown`)
 * are intentionally excluded so the view stays actionable and matches the CLI
 * human output. All findings are CANDIDATES pending verification (#903), never
 * facts.
 */
export const splitCleanupCandidates = (
  report: RuntimeCoverageReport | null,
): CleanupCandidates => {
  const findings = report?.findings ?? [];
  const safeToDelete: RuntimeCoverageFinding[] = [];
  const reviewRequired: RuntimeCoverageFinding[] = [];

  for (const finding of findings) {
    if (finding.verdict === "safe_to_delete") {
      safeToDelete.push(finding);
    } else if (finding.verdict === "review_required") {
      reviewRequired.push(finding);
    }
  }

  return { safeToDelete, reviewRequired };
};

/**
 * Return the report's hot paths sorted busiest-first by invocation count,
 * regardless of producer order. Stable for ties (preserves input order of
 * equal-invocation entries).
 */
export const sortHotPaths = (
  report: RuntimeCoverageReport | null,
): readonly RuntimeCoverageHotPath[] => {
  const hotPaths = report?.hot_paths ?? [];
  return [...hotPaths].sort((a, b) => b.invocations - a.invocations);
};

/**
 * Total renderable items across the hot-paths group and both cleanup buckets,
 * used for the view badge.
 */
export const countCoverageItems = (report: RuntimeCoverageReport | null): number => {
  if (!report) {
    return 0;
  }
  const { safeToDelete, reviewRequired } = splitCleanupCandidates(report);
  return (report.hot_paths?.length ?? 0) + safeToDelete.length + reviewRequired.length;
};
