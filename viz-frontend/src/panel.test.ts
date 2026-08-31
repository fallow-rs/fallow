import { describe, expect, it } from "vitest";
import {
  MAX_RENDERED_RANK_ROWS,
  filePanelModel,
  panelRenderKey,
  rankRowsFor,
  rankRowsForRender,
  searchPanelModel,
} from "./panel";
import { buildIndex } from "./data";
import { getTheme } from "./theme";
import type { AppState } from "./state";
import type { Lens, VizData, VizFile } from "./types";

const file = (path: string, over: Partial<VizFile> = {}): VizFile => ({
  path,
  size: 100,
  status: "clean",
  export_count: 1,
  unused_export_count: 0,
  is_entry: false,
  importer_count: 1,
  import_count: 0,
  fn_count: 1,
  max_cyclomatic: 1,
  max_cognitive: 1,
  react_hooks: 0,
  jsx_depth: 0,
  dup_lines: 0,
  in_cycle: false,
  ...over,
});

const stateFor = (lens: Lens, files: VizFile[], over: Partial<VizData> = {}): AppState => {
  const data: VizData = {
    schema_version: 2,
    root: "demo",
    files,
    edges: [],
    summary: {
      total_files: files.length,
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
    },
    workspaces: [],
    zones: [
      { name: "app", files: 1 },
      { name: "shared", files: 1 },
    ],
    cycles: [],
    clones: [],
    violations: [],
    architecture: {
      availability: { state: "complete", count: 0, unit: "violations" },
      findings: [],
    },
    dependencies: {
      availability: { state: "complete", count: 0, unit: "findings" },
      findings: [],
    },
    health: {
      availability: { state: "complete", count: 0, unit: "files" },
      capabilities: {
        complexity: { state: "complete", count: 0, unit: "findings" },
        maintainability: { state: "complete", count: 0, unit: "files" },
        crap: { state: "complete", count: 0, unit: "files" },
        coverage: { state: "complete", count: 0, unit: "gaps" },
        churn: { state: "unavailable", count: 0, unit: "files" },
        hotspots: { state: "unavailable", count: 0, unit: "files" },
        ownership: { state: "unavailable", count: 0, unit: "files" },
      },
      files: [],
      findings: [],
    },
    security: {
      availability: { state: "complete", count: 0, unit: "candidates" },
      runtime_availability: { state: "unavailable", count: 0, unit: "observations" },
      candidates: [],
      blind_spot_count: 0,
      blind_spots: [],
    },
    frameworks: {
      availability: { state: "complete", count: 0, unit: "findings" },
      detector_availability: { state: "complete", count: 0, unit: "detectors" },
      findings: [],
      detected_frameworks: [],
      detectors: [],
    },
    styling: {
      availability: { state: "complete", count: 0, unit: "findings" },
      findings: [],
    },
    feature_flags: {
      availability: { state: "complete", count: 0, unit: "flags" },
      findings: [],
    },
    ...over,
  };
  // The ranking only touches data, index, and the active lens.
  return {
    lens,
    activeAnalysis: null,
    data,
    index: buildIndex(data),
    theme: getTheme(true),
  } as AppState;
};

describe("panelRenderKey", () => {
  it("changes on selection and lens changes, not on hover", () => {
    const state = stateFor("overview", [file("src/a.ts")]);
    state.selected = null;
    state.selectedClone = null;
    state.selectedRoad = null;
    const base = panelRenderKey(state);
    state.graphHovered = 0;
    expect(panelRenderKey(state)).toBe(base);
    state.selected = 0;
    const selectedKey = panelRenderKey(state);
    expect(selectedKey).not.toBe(base);
    state.selected = null;
    state.lens = "unused";
    expect(panelRenderKey(state)).not.toBe(base);
    state.lens = "overview";
    state.activeAnalysis = "dependencies";
    expect(panelRenderKey(state)).not.toBe(base);
  });

  it("changes when the search query changes", () => {
    const state = stateFor("overview", [file("src/a.ts")]);
    state.selected = null;
    state.selectedClone = null;
    state.selectedRoad = null;
    state.search = "";
    const base = panelRenderKey(state);
    state.search = "cal";
    expect(panelRenderKey(state)).not.toBe(base);
  });

  it("identifies a selected road by its endpoint keys", () => {
    const state = stateFor("overview", [file("src/a.ts")]);
    state.selected = null;
    state.selectedClone = null;
    state.selectedRoad = null;
    const base = panelRenderKey(state);
    state.selectedRoad = {
      srcKey: "src",
      dstKey: "lib",
      count: 1,
      violations: 0,
      cycleEdges: 0,
      pairs: [],
    };
    expect(panelRenderKey(state)).not.toBe(base);
  });
});

describe("filePanelModel", () => {
  it("puts the active lens first while retaining supporting file signals", () => {
    const state = stateFor(
      "security" as Lens,
      [file("src/a.ts", { status: "unused", clone_groups: [0] })],
      {
        clones: [
          {
            lines: 4,
            tokens: 12,
            instances: [{ file: 0, start_line: 1, end_line: 4 }],
            preview: "",
            highlight_start: 0,
            highlight_lines: 0,
          },
        ],
      },
    );
    state.data = {
      ...state.data,
      schema_version: 2,
      security: {
        availability: { state: "complete", count: 1, unit: "candidates" },
        runtime_availability: { state: "unavailable", count: 0, unit: "observations" },
        blind_spot_count: 0,
        blind_spots: [],
        candidates: [
          {
            id: "candidate-1",
            kind: "injection",
            file: 0,
            path: "src/a.ts",
            line: 2,
            col: 1,
            evidence: "input reaches sink",
            severity: "high",
            crosses_boundary: true,
            client_server_boundary: false,
            cross_module_boundary: false,
            trace: [],
            actions: [],
          },
        ],
      },
      architecture: {
        availability: { state: "complete", count: 0, unit: "violations" },
        findings: [],
      },
      dependencies: {
        availability: { state: "complete", count: 0, unit: "findings" },
        findings: [],
      },
      health: {
        availability: { state: "complete", count: 0, unit: "files" },
        files: [],
        findings: [],
      },
      frameworks: {
        availability: { state: "unavailable", count: 0, unit: "findings" },
        findings: [],
      },
      styling: {
        availability: { state: "notApplicable", count: 0, unit: "findings" },
        findings: [],
      },
      feature_flags: {
        availability: { state: "disabled", count: 0, unit: "flags" },
        findings: [],
      },
    } as unknown as VizData;
    state.index = buildIndex(state.data);
    const model = filePanelModel(state, 0);
    expect(model.active).toBe("security");
    expect(model.signals.find((signal) => signal.id === "security")).toMatchObject({
      count: 1,
      state: "complete",
      active: true,
    });
    expect(model.signals.find((signal) => signal.id === "unused")?.count).toBe(1);
    expect(model.signals.find((signal) => signal.id === "duplication")?.count).toBe(1);
    expect(model.signals.find((signal) => signal.id === "frameworks")?.state).toBe("unavailable");
    expect(model.signals.find((signal) => signal.id === "styling")?.state).toBe("notApplicable");
    expect(model.signals.find((signal) => signal.id === "flags")?.state).toBe("disabled");
  });

  it("keeps complete zero distinct from unavailable for the selected file", () => {
    const state = stateFor("health" as Lens, [file("src/a.ts")]);
    state.data = {
      ...state.data,
      schema_version: 2,
      health: {
        availability: { state: "complete", count: 0, unit: "files" },
        files: [],
        findings: [],
      },
      security: {
        availability: {
          state: "unavailable",
          count: 0,
          unit: "candidates",
          reason: "Security artifacts were not retained",
        },
        runtime_availability: { state: "unavailable", count: 0, unit: "observations" },
        candidates: [],
        blind_spot_count: 0,
        blind_spots: [],
      },
    } as unknown as VizData;
    const model = filePanelModel(state, 0);
    expect(model.signals.find((signal) => signal.id === "health")).toMatchObject({
      state: "complete",
      count: 0,
    });
    expect(model.signals.find((signal) => signal.id === "security")).toMatchObject({
      state: "unavailable",
      count: 0,
    });
  });

  it("makes a More-menu analysis active for ranking and selected-file detail", () => {
    const state = stateFor("overview", [file("src/a.ts")], {
      dependencies: {
        availability: { state: "complete", count: 2, unit: "findings" },
        findings: [
          {
            kind: "unlisted-dependency",
            title: "Unlisted dependency",
            file: 0,
            path: "src/a.ts",
            line: 3,
            description: "Package is imported but not declared",
            actions: [],
          },
          {
            kind: "unused-override",
            title: "Unused dependency override",
            path: "package.json",
            description: "Override does not affect the resolved graph",
            actions: [],
          },
        ],
      },
    });
    state.activeAnalysis = "dependencies";
    expect(filePanelModel(state, 0)).toMatchObject({ active: "dependencies" });
    expect(
      filePanelModel(state, 0).signals.find((signal) => signal.id === "dependencies"),
    ).toMatchObject({ active: true, count: 1, state: "complete" });
    expect(rankRowsFor(state).rows[0]).toMatchObject({
      label: "a.ts",
      metric: "Unlisted dependency",
    });
    expect(rankRowsFor(state).rows[1]).toMatchObject({
      label: "package.json",
      fileIndex: null,
      metric: "Unused dependency override",
    });
  });
});

describe("rankRowsFor", () => {
  it("ranks unused files by size before partially unused files", () => {
    const rows = rankRowsFor(
      stateFor("unused", [
        file("src/small.ts", { status: "unused", size: 10 }),
        file("src/big.ts", { status: "unused", size: 900 }),
        file("src/partial.ts", { unused_export_count: 3 }),
      ]),
    ).rows;
    expect(rows[0].label).toBe("big.ts");
    expect(rows[1].label).toBe("small.ts");
    expect(rows[2].metric).toContain("exports");
  });

  it("ranks complexity by risk, not raw cyclomatic", () => {
    const rows = rankRowsFor(
      stateFor(
        "health",
        [
          file("src/lonely.ts", { max_cyclomatic: 23, importer_count: 0 }),
          file("src/popular.ts", { max_cyclomatic: 14, importer_count: 36 }),
        ],
        {
          health: {
            availability: { state: "complete", count: 2, unit: "files" },
            capabilities: {
              complexity: { state: "complete", count: 2, unit: "findings" },
              maintainability: { state: "complete", count: 2, unit: "files" },
              crap: { state: "complete", count: 2, unit: "files" },
              coverage: { state: "unavailable", count: 0, unit: "gaps" },
              churn: { state: "unavailable", count: 0, unit: "files" },
              hotspots: { state: "unavailable", count: 0, unit: "files" },
              ownership: { state: "unavailable", count: 0, unit: "files" },
            },
            files: [
              {
                file: 0,
                path: "src/lonely.ts",
                maintainability_index: 70,
                crap_max: 12,
                complexity_density: 0.3,
                fan_in: 0,
                fan_out: 1,
              },
              {
                file: 1,
                path: "src/popular.ts",
                maintainability_index: 40,
                crap_max: 44,
                complexity_density: 0.8,
                fan_in: 36,
                fan_out: 4,
              },
            ],
            findings: [
              {
                kind: "health-finding",
                title: "Health threshold exceeded",
                file: 0,
                path: "src/lonely.ts",
                severity: "warning",
                actions: [],
              },
              {
                kind: "health-finding",
                title: "Health threshold exceeded",
                file: 1,
                path: "src/popular.ts",
                severity: "warning",
                actions: [],
              },
            ],
          },
        },
      ),
    ).rows;
    expect(rows[0].label).toBe("popular.ts");
    expect(rows[0].metric).toContain("CRAP 44");
  });

  it("ranks retained Architecture records beyond import boundaries", () => {
    const rows = rankRowsFor(
      stateFor("architecture", [file("src/a.ts"), file("lib/b.ts")], {
        architecture: {
          availability: { state: "complete", count: 1, unit: "violations" },
          findings: [
            {
              kind: "boundary-call",
              title: "Forbidden call",
              file: 0,
              path: "src/a.ts",
              line: 5,
              description: "app cannot call the data layer directly",
              actions: [],
            },
          ],
        },
      }),
    ).rows;
    expect(rows[0].label).toBe("a.ts");
    expect(rows[0].metric).toBe("Forbidden call");
  });

  it("drops malformed clone groups instead of throwing", () => {
    const rows = rankRowsFor(
      stateFor("duplication", [file("src/a.ts")], {
        clones: [
          {
            lines: 9,
            tokens: 40,
            instances: [],
            preview: "",
            highlight_start: 0,
            highlight_lines: 0,
          },
          {
            lines: 5,
            tokens: 20,
            instances: [{ file: 99, start_line: 1, end_line: 5 }],
            preview: "",
            highlight_start: 0,
            highlight_lines: 0,
          },
          {
            lines: 3,
            tokens: 12,
            instances: [{ file: 0, start_line: 1, end_line: 3 }],
            preview: "",
            highlight_start: 0,
            highlight_lines: 0,
          },
        ],
      }),
    ).rows;
    expect(rows).toHaveLength(1);
    expect(rows[0].label).toContain("a.ts");
  });

  it("keeps project findings discoverable when their file index is stale", () => {
    const rows = rankRowsFor(
      stateFor("architecture", [file("src/a.ts")], {
        architecture: {
          availability: { state: "complete", count: 1, unit: "violations" },
          findings: [
            {
              kind: "policy-violation",
              title: "Policy violation",
              file: 99,
              actions: [],
            },
          ],
        },
      }),
    ).rows;
    expect(rows[0]).toMatchObject({ label: "Policy violation", fileIndex: null });
  });

  it("carries the clone group index so rows open the clone panel", () => {
    const rows = rankRowsFor(
      stateFor("duplication", [file("src/a.ts"), file("src/b.ts")], {
        clones: [
          {
            lines: 12,
            tokens: 80,
            instances: [
              { file: 0, start_line: 1, end_line: 12 },
              { file: 1, start_line: 4, end_line: 15 },
            ],
            preview: "",
            highlight_start: 0,
            highlight_lines: 0,
          },
        ],
      }),
    ).rows;
    expect(rows[0].clone).toBe(0);
    expect(rows[0].metric).toBe("12 lines");
  });

  it("caps rendered ranking rows and reports the omitted count", () => {
    const files = Array.from({ length: MAX_RENDERED_RANK_ROWS + 3 }, (_, index) =>
      file(`src/file-${index}.ts`, { importer_count: index + 1 }),
    );
    const ranked = rankRowsFor(stateFor("overview", files)).rows;
    const visible = rankRowsForRender(ranked);
    expect(visible.rows).toHaveLength(MAX_RENDERED_RANK_ROWS);
    expect(visible.truncated).toBe(3);
    expect(visible.rows[0].label).toBe(`file-${MAX_RENDERED_RANK_ROWS + 2}.ts`);
  });
});

describe("rankRowsFor overview", () => {
  it("ranks the most depended-on files first", () => {
    const rows = rankRowsFor(
      stateFor("overview", [
        file("src/a.ts", { importer_count: 2 }),
        file("src/hub.ts", { importer_count: 40 }),
        file("src/leaf.ts", { importer_count: 0 }),
      ]),
    ).rows;
    expect(rows[0].label).toBe("hub.ts");
    expect(rows[0].metric).toBe("used by 40");
    expect(rows.some((r) => r.label === "leaf.ts")).toBe(false);
  });
});

describe("searchPanelModel", () => {
  it("ranks matches and affected files most-depended-on first", () => {
    const state = stateFor("overview", [
      file("src/calendar/grid.ts", { importer_count: 3 }),
      file("src/calendar/index.ts", { importer_count: 30 }),
      file("src/app.ts", { importer_count: 0 }),
    ]);
    state.search = "calendar";
    state.searchMatches = new Set([0, 1]);
    state.searchReach = new Set([2]);
    const model = searchPanelModel(state);
    expect(model.query).toBe("calendar");
    // index.ts (used by 30) ranks before grid.ts (used by 3).
    expect(model.matches).toEqual([1, 0]);
    expect(model.affected).toEqual([2]);
  });

  it("trims the query and reports an empty match set", () => {
    const state = stateFor("overview", [file("src/a.ts")]);
    state.search = "  none  ";
    state.searchMatches = new Set();
    state.searchReach = new Set();
    const model = searchPanelModel(state);
    expect(model.query).toBe("none");
    expect(model.matches).toEqual([]);
    expect(model.affected).toEqual([]);
  });
});
