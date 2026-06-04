import type { FindingSeverity, HealthReport, HealthScorePenalties } from "./types.js";

/**
 * Options for {@link buildHealthArgs}. Kept as a plain data object (no config
 * or VS Code access) so the argv-construction rules can be unit-tested.
 */
export interface HealthArgsOptions {
  /** Include git churn hotspots (`--hotspots`). Touches git history. */
  readonly hotspots: boolean;
  /** Cap on complexity findings serialized into the tree (`--top <N>`). */
  readonly topFindings: number;
  /** Resolved config path, forwarded as `--config <path>` when non-empty. */
  readonly configPath: string;
  /** Git ref for `--changed-since <ref>`, forwarded when non-empty. */
  readonly changedSince: string;
  /** Production mode (`--production`), forwarded when true. */
  readonly production: boolean;
}

/**
 * Build the argument vector for the standalone `fallow health` run that backs
 * the Health view. Kept pure so flag-forwarding rules can be unit-tested.
 *
 * `fallow health` shows every section by default; passing ANY section flag
 * switches it to "only these sections". We always request the cheap sections
 * (`--score --complexity --targets`, no git) and add `--hotspots` only when the
 * user opted in, since that section walks git history. The combined sidebar run
 * is untouched (it keeps `--skip health`); this is a separate, lazy spawn.
 */
export const buildHealthArgs = (options: HealthArgsOptions): string[] => {
  const args = [
    "health",
    "--format",
    "json",
    "--quiet",
    "--score",
    "--complexity",
    "--targets",
  ];

  if (options.hotspots) {
    args.push("--hotspots");
  }

  if (Number.isFinite(options.topFindings) && options.topFindings > 0) {
    args.push("--top", String(Math.floor(options.topFindings)));
  }

  if (options.production) {
    args.push("--production");
  }

  if (options.changedSince) {
    args.push("--changed-since", options.changedSince);
  }

  if (options.configPath) {
    args.push("--config", options.configPath);
  }

  return args;
};

/**
 * Format the score label shown in the Score tree row and status bar, e.g.
 * `B (82)`. The score is rounded to a whole number for a compact, stable
 * display; the grade is taken verbatim from the CLI.
 */
export const formatScoreLabel = (score: number, grade: string): string => {
  const rounded = Number.isFinite(score) ? Math.round(score) : 0;
  const safeGrade = grade.trim() || "?";
  return `${safeGrade} (${rounded})`;
};

/**
 * Status bar segment for the health score, e.g. `B (82)`. Returns null when
 * there is no score to show, so the caller can omit the segment entirely.
 */
export const formatHealthStatusPart = (report: HealthReport | null): string | null => {
  const score = report?.health_score;
  if (!score) {
    return null;
  }
  return formatScoreLabel(score.score, score.grade);
};

/**
 * Codicon for a grade. A/B are healthy (check), C is neutral (info), D/F are
 * unhealthy (warning). Unknown grades fall back to a neutral pulse so the row
 * never renders an empty icon.
 */
export const gradeIcon = (grade: string): string => {
  const normalized = grade.trim().toUpperCase().charAt(0);
  if (normalized === "A" || normalized === "B") {
    return "check";
  }
  if (normalized === "C") {
    return "info";
  }
  if (normalized === "D" || normalized === "F") {
    return "warning";
  }
  return "pulse";
};

/**
 * VS Code theme color key for a grade, used to tint the Score row icon. A/B map
 * to a success-ish foreground, C to a warning, D/F to an error. Unknown grades
 * return null so the icon keeps its default foreground. Always a theme token,
 * never a hard-coded color.
 */
export const gradeThemeColor = (grade: string): string | null => {
  const normalized = grade.trim().toUpperCase().charAt(0);
  if (normalized === "A" || normalized === "B") {
    return "charts.green";
  }
  if (normalized === "C") {
    return "charts.yellow";
  }
  if (normalized === "D" || normalized === "F") {
    return "charts.red";
  }
  return null;
};

/**
 * Codicon for a complexity-finding severity. Distinct icons for the three known
 * severities; unknown values fall back to a neutral circle.
 */
export const severityIcon = (severity: FindingSeverity | string): string => {
  switch (severity) {
    case "critical":
      return "error";
    case "high":
      return "warning";
    case "moderate":
      return "info";
    default:
      return "circle-outline";
  }
};

/** A single penalty contributor to the health score, for the score tooltip. */
export interface PenaltyContribution {
  readonly key: string;
  readonly points: number;
}

/** Human-readable labels for the penalty components shown in the tooltip. */
const PENALTY_LABELS: Record<keyof HealthScorePenalties, string> = {
  dead_files: "Dead files",
  dead_exports: "Dead exports",
  complexity: "Complexity",
  p90_complexity: "P90 complexity",
  maintainability: "Maintainability",
  hotspots: "Hotspots",
  unused_deps: "Unused dependencies",
  circular_deps: "Circular dependencies",
  unit_size: "Unit size",
  coupling: "Coupling",
  duplication: "Duplication",
};

/**
 * Sorted, non-zero penalty contributors for the score tooltip, highest first.
 * Null/undefined/zero contributors are dropped (they did not lower the score).
 */
export const topPenalties = (
  penalties: HealthScorePenalties | null | undefined,
  limit = 5,
): PenaltyContribution[] => {
  if (!penalties) {
    return [];
  }

  const contributions: PenaltyContribution[] = [];
  for (const key of Object.keys(PENALTY_LABELS) as (keyof HealthScorePenalties)[]) {
    const points = penalties[key];
    if (typeof points === "number" && Number.isFinite(points) && points > 0) {
      contributions.push({ key: PENALTY_LABELS[key], points });
    }
  }

  contributions.sort((a, b) => b.points - a.points);
  return contributions.slice(0, Math.max(0, limit));
};

/**
 * Total number of items the Health view will render across its findings,
 * hotspots, and targets sections. Used for the view badge. Tolerates sparse or
 * absent sections (no git means no hotspots; `--targets` may be empty).
 */
export const countHealthItems = (report: HealthReport | null): number => {
  if (!report) {
    return 0;
  }
  return (
    (report.findings?.length ?? 0) +
    (report.hotspots?.length ?? 0) +
    (report.targets?.length ?? 0)
  );
};

/** Compact one-line label for a hotspot row: `score · N commits`. */
export const formatHotspotDescription = (score: number, commits: number): string => {
  const safeScore = Number.isFinite(score) ? Math.round(score) : 0;
  return `score ${safeScore} · ${commits} commit${commits === 1 ? "" : "s"}`;
};
