import type {
  RuntimeCoverageConfidence,
  RuntimeCoverageFinding,
  RuntimeCoverageHotPath,
  RuntimeCoverageReport,
} from "./types.js";

/**
 * Humanize a runtime-coverage confidence enum for tooltips: the raw contract
 * value (`very_high`, `low`, ...) is snake_case, so render it as spaced words.
 * Kept pure for unit testing.
 */
export const formatConfidence = (confidence: RuntimeCoverageConfidence): string =>
  confidence.replace(/_/g, " ");

/** First CLI version that ships `fallow coverage analyze --runtime-coverage ... --format json` (CHANGELOG 2.57.0). */
export const COVERAGE_ANALYZE_MIN_VERSION = "2.57.0";

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
export const splitCleanupCandidates = (report: RuntimeCoverageReport | null): CleanupCandidates => {
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
  return hotPaths.toSorted((a, b) => b.invocations - a.invocations);
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

/** Exit code the CLI emits when the runtime-coverage license gate rejects. */
const COVERAGE_EXIT_LICENSE = 3;
/** Exit code the CLI emits when sidecar discovery fails. */
const COVERAGE_EXIT_SIDECAR_MISSING = 4;
/** Exit code the CLI emits when the sidecar binary fails signature verification. */
const COVERAGE_EXIT_SIDECAR_INVALID = 5;

/** Narrow a parsed CLI JSON envelope to the structured-error shape. */
const isStructuredError = (value: unknown): value is { error: true; message?: string } =>
  typeof value === "object" &&
  value !== null &&
  "error" in value &&
  (value as { error: unknown }).error === true;

/**
 * Build an actionable error message for a failed `coverage analyze` run from the
 * CLI's exit code and captured stdout. Kept pure (no VS Code access) so the
 * gate-error path can be unit-tested.
 *
 * The CLI writes a structured `{error:true,message,exit_code}` envelope to stdout
 * under `--format json` and exits non-zero: 3 = license/trial gate, 4 = sidecar
 * not found, 5 = sidecar signature invalid. The license and sidecar gates are the
 * default first-run state for this paid, separately-installed feature, so each is
 * special-cased with a concrete next step rather than a bare "exited with code N".
 * Falls back to the structured message (then `fallbackMessage`) for other codes.
 */
export const buildCoverageGateMessage = (
  exitCode: number | null,
  stdout: string,
  fallbackMessage: string,
): string => {
  let structured: string | undefined;
  const trimmed = stdout.trim();
  if (trimmed.length > 0) {
    try {
      const parsed: unknown = JSON.parse(trimmed);
      if (isStructuredError(parsed)) {
        structured = parsed.message;
      }
    } catch {
      // Non-JSON stdout (older CLI, partial output): fall through to fallback.
    }
  }

  const detail = structured ?? fallbackMessage;

  if (exitCode === COVERAGE_EXIT_LICENSE) {
    return `${detail} Activate a runtime-coverage license or trial: run \`fallow license activate --trial --email you@company.com\`.`;
  }
  if (exitCode === COVERAGE_EXIT_SIDECAR_MISSING || exitCode === COVERAGE_EXIT_SIDECAR_INVALID) {
    return `${detail} Install the fallow-cov sidecar: run \`fallow coverage setup\`.`;
  }
  return detail;
};

/**
 * One-line caveat for a license/trial grace watermark, or null when the report
 * carries none. When set, "Safe to Delete" candidates were produced under a
 * stale or expired license, so they must not be treated as authoritative. Kept
 * pure so the disclosure copy can be unit-tested.
 */
export const coverageWatermarkMessage = (report: RuntimeCoverageReport | null): string | null => {
  const watermark = report?.watermark;
  if (!watermark) {
    return null;
  }
  if (watermark === "trial-expired") {
    return "Runtime coverage was produced under an expired trial; treat these candidates as stale and re-run after activating a license.";
  }
  if (watermark === "license-expired-grace") {
    return "Runtime coverage was produced under license grace (the license has expired); refresh with `fallow license refresh` before acting on these candidates.";
  }
  return "Runtime coverage carries a license watermark; verify your license before acting on these candidates.";
};
