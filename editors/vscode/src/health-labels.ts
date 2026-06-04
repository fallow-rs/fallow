/**
 * Health tree-view section labels and icons. Kept separate from `labels.ts`
 * so the dead-code `IssueCategory` surface (which mirrors `fallow.issueTypes.*`
 * setting keys) is untouched: these section names are pure UI strings, not
 * rule names or setting keys.
 *
 * Hotspots and refactoring targets are heuristic, so their section labels read
 * as "candidates" (pending verification), while the score/grade and complexity
 * findings are measured and framed plainly.
 */

export type HealthSection = "score" | "complexity" | "hotspots" | "targets";

export const HEALTH_SECTION_LABELS: Record<HealthSection, string> = {
  score: "Score",
  complexity: "Complexity",
  hotspots: "Hotspot Candidates",
  targets: "Refactoring Candidates",
};

/** Codicon for each health section header. */
export const HEALTH_SECTION_ICONS: Record<HealthSection, string> = {
  score: "pulse",
  complexity: "flame",
  hotspots: "git-commit",
  targets: "tools",
};
