import { escapeMarkdownMultiline } from "./markdown-utils.js";
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
  /**
   * Production mode (`--production`), forwarded when `true`. `false`/`undefined`
   * defer to the project config (force-off via `--no-production` is
   * editor-diagnostics-only, see #1055).
   */
  readonly production: boolean | undefined;
  /**
   * Monorepo workspace scope (a package name). When a non-empty string,
   * forwarded as `--workspace <name>` so the Health view honors the selected
   * workspace. NOT version-gated: `--workspace` is a long-standing global CLI
   * flag. Mirrors the workspace forwarding in `buildAnalysisArgs`.
   */
  readonly workspace?: string;
  /**
   * Request the per-decision-point complexity breakdown (`--complexity-breakdown`),
   * forwarded when true. Drives the inline editor decorations.
   */
  readonly complexityBreakdown?: boolean;
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
  const args = ["health", "--format", "json", "--quiet", "--score", "--complexity", "--targets"];

  if (options.hotspots) {
    args.push("--hotspots");
  }

  if (options.complexityBreakdown) {
    args.push("--complexity-breakdown");
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

  if (options.workspace) {
    args.push("--workspace", options.workspace);
  }

  if (options.configPath) {
    args.push("--config", options.configPath);
  }

  return args;
};

/**
 * Escape text destined for a trusted `MarkdownString` health tooltip. Alias of
 * `escapeMarkdownMultiline` kept for call-site readability; delegates to the
 * canonical implementation in `markdown-utils.ts`.
 */
export const escapeHealthMarkdown = escapeMarkdownMultiline;

/**
 * Detect a clap "unrecognized subcommand" error for `health`, raised when the
 * resolved CLI predates the `fallow health` command. Lets the caller degrade to
 * a one-line "update fallow" warning instead of surfacing a raw stderr blob and
 * re-spawning on every Health-view reveal. Handles modern clap (`unrecognized
 * subcommand 'health'`) and the legacy phrasing (`The subcommand 'health'
 * wasn't recognized`). Unrelated errors return false so genuine failures stay
 * loud. Mirrors {@link parseUnknownSubcommand} in `security-utils.ts`.
 */
export const parseUnknownHealthSubcommand = (message: string): boolean => {
  if (/unrecognized subcommand '?health'?/i.test(message)) {
    return true;
  }
  if (/subcommand '?health'? (?:wasn't|was not) recognized/i.test(message)) {
    return true;
  }
  return false;
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
 * Codicon for a complexity-finding severity. Complexity findings are heuristic
 * candidates, not errors, so `critical` uses the section's own `flame` glyph
 * rather than the alarming error `X`. Distinct shapes across the three
 * severities keep them legible without relying on color; unknown values fall
 * back to a neutral circle.
 */
export const severityIcon = (severity: FindingSeverity | string): string => {
  switch (severity) {
    case "critical":
      return "flame";
    case "high":
      return "warning";
    case "moderate":
      return "info";
    default:
      return "circle-outline";
  }
};

/**
 * Theme color for a complexity-finding severity, paired with {@link severityIcon}
 * so severity reads via both icon shape and color (never color alone). Uses the
 * neutral chart palette rather than the error/warning foregrounds to avoid
 * over-claiming that a heuristic finding is a broken-code error.
 */
export const severityThemeColor = (severity: FindingSeverity | string): string | null => {
  switch (severity) {
    case "critical":
      return "charts.red";
    case "high":
      return "charts.orange";
    case "moderate":
      return "charts.blue";
    default:
      return null;
  }
};

/**
 * Compact offense summary for a complexity finding's row description, e.g.
 * `parseArgs · 24 cyc · 18 cog · CRAP 31`. The row label is the file:line (for
 * consistency with the other file-led Health sections), so this dimmed detail
 * leads with the function name and abbreviates the metrics (cyc/cog) to fit
 * beside it; the full-word metrics live in the row tooltip. CRAP is omitted when
 * the finding carries no score.
 */
export const formatComplexityOffense = (finding: {
  readonly name: string;
  readonly cyclomatic: number;
  readonly cognitive: number;
  readonly crap?: number | null;
}): string => {
  const crapSegment = typeof finding.crap === "number" ? ` · CRAP ${finding.crap.toFixed(0)}` : "";
  return `${finding.name} · ${finding.cyclomatic} cyc · ${finding.cognitive} cog${crapSegment}`;
};

/** A single penalty contributor to the health score, for the score tooltip. */
export interface PenaltyContribution {
  readonly key: string;
  readonly points: number;
}

/**
 * Human-readable labels for the penalty components shown in the tooltip. The
 * key set must stay in lockstep with the `HealthScorePenalties` wire contract
 * (`crates/cli/src/health_types/scores.rs` via the generated TS interface): a
 * new penalty field that is not labelled here is silently omitted from the
 * score tooltip. The parity is guarded by a test in `health-utils.test.ts` that
 * diffs these keys against the generated `HealthScorePenalties` interface.
 */
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
  prop_drilling: "Prop drilling",
};

/**
 * The penalty wire keys this module knows how to label. Exposed so a drift test
 * can assert it matches the generated `HealthScorePenalties` contract; a Rust
 * penalty field that flows through codegen but is missing here would otherwise
 * be silently dropped from the score tooltip.
 */
export const recognizedPenaltyKeys: ReadonlyArray<keyof HealthScorePenalties> = Object.keys(
  PENALTY_LABELS,
) as (keyof HealthScorePenalties)[];

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
    (report.findings?.length ?? 0) + (report.hotspots?.length ?? 0) + (report.targets?.length ?? 0)
  );
};

/** Compact one-line label for a hotspot row: `score · N commits`. */
export const formatHotspotDescription = (score: number, commits: number): string => {
  const safeScore = Number.isFinite(score) ? Math.round(score) : 0;
  return `score ${safeScore} · ${commits} commit${commits === 1 ? "" : "s"}`;
};
