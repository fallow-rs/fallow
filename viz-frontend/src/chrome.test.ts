import { describe, expect, it } from "vitest";
import { lensSummaryText } from "./chrome";
import type { AppState } from "./state";
import type { Lens, VizSummary } from "./types";

const summary = (over: Partial<VizSummary> = {}): VizSummary => ({
  total_files: 0,
  total_size: 0,
  total_edges: 0,
  unused_files: 0,
  unused_exports: 0,
  unused_types: 0,
  unused_deps: 0,
  unresolved_imports: 0,
  circular_deps: 0,
  clone_groups: 0,
  duplicated_lines: 0,
  boundary_violations: 0,
  hotspot_files: 0,
  ...over,
});

const stateFor = (
  lens: Lens,
  over: Partial<VizSummary> = {},
  selectedClone: number | null = null,
): AppState => ({ lens, selectedClone, data: { summary: summary(over) } }) as unknown as AppState;

describe("lensSummaryText", () => {
  it("counts files and imports for the overview lens", () => {
    expect(lensSummaryText(stateFor("overview", { total_files: 12, total_edges: 34 }))).toBe(
      "12 files, 34 imports",
    );
  });

  it("explains the neutral state vs the finding count for dead code", () => {
    expect(lensSummaryText(stateFor("deadcode"))).toContain("Every file is reachable");
    expect(lensSummaryText(stateFor("deadcode", { unused_files: 3, unused_exports: 5 }))).toBe(
      "3 unused files and 5 unused exports, shown in red and amber.",
    );
  });

  it("pluralizes loops and forbidden imports independently", () => {
    expect(
      lensSummaryText(stateFor("boundaries", { circular_deps: 1, boundary_violations: 1 })),
    ).toContain("1 loop and 1 forbidden import,");
    expect(
      lensSummaryText(stateFor("boundaries", { circular_deps: 2, boundary_violations: 3 })),
    ).toContain("2 loops and 3 forbidden imports");
    expect(lensSummaryText(stateFor("boundaries"))).toBe("Every import stays inside its layer.");
  });

  it("switches the dupes summary between empty, list, and drilled states", () => {
    expect(lensSummaryText(stateFor("dupes"))).toBe("No block of code appears twice.");
    expect(lensSummaryText(stateFor("dupes", { clone_groups: 4, duplicated_lines: 40 }))).toContain(
      "4 blocks, 40 duplicated lines",
    );
    expect(lensSummaryText(stateFor("dupes", { clone_groups: 4 }, 2))).toContain(
      "Viewing 1 of 4 duplicated blocks",
    );
  });

  it("names the most complex files for the hotspots lens", () => {
    expect(lensSummaryText(stateFor("hotspots"))).toBe(
      "No file crosses the complexity thresholds.",
    );
    expect(lensSummaryText(stateFor("hotspots", { hotspot_files: 7 }))).toContain(
      "The 7 most complex files",
    );
  });
});
