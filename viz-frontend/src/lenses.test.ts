import { describe, expect, it } from "vitest";
import {
  LENS_IDS,
  fitLensNavigation,
  lensAvailability,
  lensById,
  parseLens,
  parseSecondaryAnalysis,
  secondaryAnalysisMenuItems,
  secondaryAvailabilityDetails,
} from "./lenses";
import type { AppState } from "./state";

const state = (data: unknown): AppState => ({ data }) as unknown as AppState;

describe("lens registry", () => {
  it("defines the canonical lenses in shortcut order", () => {
    expect(LENS_IDS).toEqual([
      "overview",
      "unused",
      "duplication",
      "architecture",
      "health",
      "security",
    ]);
  });

  it("normalizes legacy deep-link aliases", () => {
    expect(parseLens("deadcode")).toBe("unused");
    expect(parseLens("dupes")).toBe("duplication");
    expect(parseLens("boundaries")).toBe("architecture");
    expect(parseLens("hotspots")).toBe("health");
    expect(parseLens("unknown")).toBeNull();
    expect(parseSecondaryAnalysis("flags")).toBe("feature_flags");
    expect(parseSecondaryAnalysis("dependencies")).toBe("dependencies");
  });

  it("uses one stable unit per lens", () => {
    const appState = state({
      files: [
        { status: "unused", unused_export_count: 0 },
        { status: "clean", unused_export_count: 2 },
        { status: "hasUnusedExports", unused_export_count: 1 },
      ],
      summary: {
        unused_files: 3,
        unused_exports: 20,
        clone_groups: 4,
        boundary_violations: 5,
        hotspot_files: 6,
      },
      security: { candidates: [{}, {}] },
    });

    expect(lensById("unused").count(appState)).toEqual({ value: 3, unit: "affected files" });
    expect(lensById("duplication").count(appState)).toEqual({
      value: 4,
      unit: "clone groups",
    });
    expect(lensById("architecture").count(appState)).toEqual({
      value: 5,
      unit: "violations",
    });
    expect(lensById("health").count(appState)).toEqual({ value: 6, unit: "hotspot files" });
    expect(lensById("security").count(appState)).toEqual({ value: 2, unit: "candidates" });
  });

  it("distinguishes unavailable security from zero findings", () => {
    expect(lensAvailability(state({ summary: {}, files: [] }), "security")).toBe("unavailable");
    expect(
      lensAvailability(state({ summary: { security_candidates: 0 }, files: [] }), "security"),
    ).toBe("complete");
    expect(
      lensAvailability(
        state({
          summary: {},
          files: [],
          security: {
            availability: { state: "disabled", count: 0, unit: "candidates" },
          },
        }),
        "security",
      ),
    ).toBe("disabled");
  });

  it("keeps the active lens visible when navigation overflows", () => {
    const widths = LENS_IDS.map((id) => ({ id, width: 100 }));
    expect(fitLensNavigation(widths, 350, 70, "security")).toEqual(["overview", "security"]);
    expect(fitLensNavigation(widths, 800, 70, "security")).toEqual(LENS_IDS);
  });

  it("reports secondary analysis availability and units", () => {
    const availability = secondaryAvailabilityDetails(
      state({
        summary: {},
        files: [],
        dependencies: {
          availability: {
            state: "complete",
            count: 7,
            unit: "dependency findings",
            truncated: 2,
          },
        },
      }),
      "dependencies",
    );

    expect(availability).toEqual({
      state: "complete",
      value: 7,
      unit: "dependency findings",
      reason: null,
      truncated: 2,
    });
  });

  it("builds the More menu model with active and unavailable states", () => {
    const appState = state({
      summary: {},
      files: [],
      dependencies: {
        availability: { state: "complete", count: 2, unit: "findings" },
      },
    });
    appState.activeAnalysis = "dependencies";

    const items = secondaryAnalysisMenuItems(appState);
    expect(items.find((item) => item.id === "dependencies")).toMatchObject({
      active: true,
      availability: { state: "complete", value: 2, unit: "findings" },
    });
    expect(items.find((item) => item.id === "styling")).toMatchObject({
      active: false,
      availability: { state: "unavailable" },
    });
  });
});
