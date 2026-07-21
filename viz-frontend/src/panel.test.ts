import { describe, expect, it } from "vitest";
import { panelRenderKey, rankRowsFor, searchPanelModel } from "./panel";
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
    ...over,
  };
  // The ranking only touches data, index, and the active lens.
  return { lens, data, index: buildIndex(data), theme: getTheme(true) } as AppState;
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
    state.lens = "deadcode";
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

describe("rankRowsFor", () => {
  it("ranks unused files by size before partially unused files", () => {
    const rows = rankRowsFor(
      stateFor("deadcode", [
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
      stateFor("hotspots", [
        file("src/lonely.ts", { max_cyclomatic: 23, importer_count: 0 }),
        file("src/popular.ts", { max_cyclomatic: 14, importer_count: 36 }),
      ]),
    ).rows;
    expect(rows[0].label).toBe("popular.ts");
    expect(rows[0].metric).toContain("used by 36");
  });

  it("writes boundary crossings as an arrow into the target zone", () => {
    const rows = rankRowsFor(
      stateFor("boundaries", [file("src/a.ts"), file("lib/b.ts")], {
        violations: [{ from: 0, to: 1, from_zone: 0, to_zone: 1, line: 5, specifier: "../lib/b" }],
      }),
    ).rows;
    expect(rows[0].label).toBe("a.ts → b.ts");
    expect(rows[0].metric).toBe("→ shared");
  });

  it("drops malformed clone groups instead of throwing", () => {
    const rows = rankRowsFor(
      stateFor("dupes", [file("src/a.ts")], {
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

  it("skips violations and cycles that point outside the file table", () => {
    const rows = rankRowsFor(
      stateFor("boundaries", [file("src/a.ts")], {
        violations: [{ from: 0, to: 99, from_zone: 0, to_zone: 1, line: 1, specifier: "x" }],
        cycles: [[], [99]],
      }),
    ).rows;
    expect(rows).toHaveLength(0);
  });

  it("carries the clone group index so rows open the clone panel", () => {
    const rows = rankRowsFor(
      stateFor("dupes", [file("src/a.ts"), file("src/b.ts")], {
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
