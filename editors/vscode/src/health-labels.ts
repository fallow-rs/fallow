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

export type HealthSection =
  | "score"
  | "complexity"
  | "hotspots"
  | "hiddenCoupling"
  | "targets"
  | "typeCoupling";

export const HEALTH_SECTION_LABELS: Record<HealthSection, string> = {
  score: "Score",
  complexity: "Complexity",
  hotspots: "Hotspot Candidates",
  hiddenCoupling: "Hidden Change Coupling",
  targets: "Refactoring Candidates",
  typeCoupling: "Type Coupling",
};

/** Codicon for each health section header. */
export const HEALTH_SECTION_ICONS: Record<HealthSection, string> = {
  score: "pulse",
  complexity: "flame",
  hotspots: "git-commit",
  hiddenCoupling: "git-compare",
  targets: "tools",
  typeCoupling: "references",
};
