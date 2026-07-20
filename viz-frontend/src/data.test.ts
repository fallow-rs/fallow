import { describe, expect, it } from "vitest";
import { runSearch } from "./state";
import type { AppState } from "./state";
import {
  basename,
  buildIndex,
  dirname,
  reachSet,
  reachSetMulti,
  dupRatio,
  formatSize,
  legendText,
  lensColor,
  lensFindingLevel,
} from "./data";
import { getTheme } from "./theme";
import type { VizData, VizFile } from "./types";

const file = (over: Partial<VizFile> = {}): VizFile => ({
  path: "src/a.ts",
  size: 340,
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

const data = (over: Partial<VizData> = {}): VizData => ({
  root: "demo",
  files: [file({ path: "src/a.ts" }), file({ path: "src/b.ts" }), file({ path: "lib/c.ts" })],
  edges: [
    [0, 1, 0],
    [1, 2, 0],
  ],
  summary: {
    total_files: 3,
    total_size: 1020,
    total_edges: 2,
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
  zones: [],
  cycles: [],
  clones: [],
  violations: [],
  ...over,
});

describe("buildIndex", () => {
  it("mirrors edges into importer and import lists", () => {
    const index = buildIndex(data());
    expect(index.importsOf[0]).toEqual([1]);
    expect(index.importersOf[1]).toEqual([0]);
    expect(index.importersOf[0]).toEqual([]);
  });

  it("marks every directed pair of a cycle in both directions", () => {
    const index = buildIndex(data({ cycles: [[0, 1]] }));
    const n = 3;
    expect(index.cycleEdges.has(0 * n + 1)).toBe(true);
    expect(index.cycleEdges.has(1 * n + 0)).toBe(true);
  });

  it("collects violation sources", () => {
    const index = buildIndex(
      data({
        violations: [{ from: 0, to: 2, from_zone: 0, to_zone: 1, line: 3, specifier: "../lib/c" }],
      }),
    );
    expect(index.violationSources.has(0)).toBe(true);
    expect(index.violationSources.has(1)).toBe(false);
  });

  it("ignores cycle pairs and violations that point outside the file table", () => {
    const index = buildIndex(
      data({
        cycles: [[0, 99]],
        violations: [{ from: 0, to: 99, from_zone: 0, to_zone: 1, line: 3, specifier: "x" }],
      }),
    );
    expect(index.cycleEdges.size).toBe(0);
    expect(index.violationSources.has(0)).toBe(false);
    expect(index.violationEdges.size).toBe(0);
  });

  it("builds a directory tree that survives chain collapsing", () => {
    const index = buildIndex(data());
    expect(index.nodesByPath.has("src")).toBe(true);
    expect(index.tree.size).toBeGreaterThan(0);
  });
});

describe("dupRatio", () => {
  it("is zero without duplicated lines and capped at one", () => {
    expect(dupRatio(file())).toBe(0);
    expect(dupRatio(file({ size: 34, dup_lines: 500 }))).toBe(1);
  });
});

describe("legendText", () => {
  it("explains the neutral map when a finding lens is clean", () => {
    expect(legendText("deadcode", data(), "graph")).toContain("No findings");
  });

  it("keeps the color key when findings exist", () => {
    const d = data();
    d.summary.unused_files = 2;
    expect(legendText("deadcode", d, "graph")).toContain("Red is never imported");
  });

  it("describes tiles in map view and dots in graph view", () => {
    expect(legendText("overview", data(), "map")).toContain("Each tile is a file");
    expect(legendText("overview", data(), "graph")).toContain("Each dot is a file");
  });
});

describe("lens coloring", () => {
  const theme = getTheme(true);

  it("keeps the overview neutral except entry points", () => {
    const index = buildIndex(data());
    expect(lensColor("overview", theme, index, file())).toBe(theme.cellNeutral);
    expect(lensColor("overview", theme, index, file({ status: "entryPoint" }))).toBe(
      theme.cellEntry,
    );
  });

  it("grades findings per lens for the non-color texture channel", () => {
    const index = buildIndex(data());
    // deadcode: unused file severe, unused exports mild, clean none.
    expect(lensFindingLevel("deadcode", index, file({ status: "unused" }), 0)).toBe(2);
    expect(lensFindingLevel("deadcode", index, file({ unused_export_count: 2 }), 0)).toBe(1);
    expect(lensFindingLevel("deadcode", index, file(), 0)).toBe(0);
    // dupes: >= 30% duplicated lines severe, any duplication mild.
    expect(lensFindingLevel("dupes", index, file({ size: 340, dup_lines: 9 }), 0)).toBe(2);
    expect(lensFindingLevel("dupes", index, file({ size: 3400, dup_lines: 1 }), 0)).toBe(1);
    expect(lensFindingLevel("dupes", index, file(), 0)).toBe(0);
    // hotspots: cc thresholds match the panel's sev split.
    expect(lensFindingLevel("hotspots", index, file({ max_cyclomatic: 25 }), 0)).toBe(2);
    expect(lensFindingLevel("hotspots", index, file({ max_cyclomatic: 12 }), 0)).toBe(1);
    expect(lensFindingLevel("hotspots", index, file({ max_cyclomatic: 5 }), 0)).toBe(0);
    // boundaries: violation sources severe; overview always none.
    const vIndex = buildIndex(
      data({
        violations: [{ from: 0, to: 2, from_zone: 0, to_zone: 1, line: 3, specifier: "x" }],
      }),
    );
    expect(lensFindingLevel("boundaries", vIndex, file(), 0)).toBe(2);
    expect(lensFindingLevel("boundaries", vIndex, file(), 1)).toBe(0);
    expect(lensFindingLevel("overview", vIndex, file({ status: "unused" }), 0)).toBe(0);
  });
});

describe("formatting", () => {
  it("scales byte sizes", () => {
    expect(formatSize(512)).toBe("512 B");
    expect(formatSize(2048)).toBe("2.0 KB");
  });

  it("splits paths", () => {
    expect(basename("a/b/c.ts")).toBe("c.ts");
    expect(dirname("a/b/c.ts")).toBe("a/b");
    expect(dirname("c.ts")).toBe("");
  });
});

describe("reachSet", () => {
  // Chain a -> b -> c: c is reached from a downstream; a affects c upstream.
  const adjDown = [[1], [2], []]; // importsOf: a imports b, b imports c
  const adjUp = [[], [0], [1]]; // importersOf: b imported by a, c imported by b

  it("collects the full transitive downstream set, excluding the start", () => {
    const r = reachSet(adjDown, 0);
    expect([...r].toSorted()).toEqual([1, 2]);
    expect(r.has(0)).toBe(false);
  });

  it("collects the full transitive upstream (blast radius) set", () => {
    expect([...reachSet(adjUp, 2)].toSorted()).toEqual([0, 1]);
    expect(reachSet(adjUp, 0).size).toBe(0);
  });

  it("terminates on a cycle instead of looping forever", () => {
    const cyclic = [[1], [0]];
    expect([...reachSet(cyclic, 0)].toSorted()).toEqual([1]);
  });
});

describe("reachSetMulti", () => {
  // Diamond: 3 and 4 both import 1 and 2; 1 and 2 both import 0.
  const adjUp = [[1, 2], [3, 4], [3, 4], [], []]; // importersOf
  const seeds = [1, 2];

  it("matches the union of the per-seed reachSet, minus the seeds", () => {
    const union = new Set([...reachSet(adjUp, 1), ...reachSet(adjUp, 2)]);
    for (const s of seeds) union.delete(s);
    expect([...reachSetMulti(adjUp, seeds)].toSorted()).toEqual([...union].toSorted());
    expect([...reachSetMulti(adjUp, seeds)].toSorted()).toEqual([3, 4]);
  });

  it("excludes every seed even when one seed is reachable from another", () => {
    // 1 imports 0, and 0 is also a seed: 0 must not appear in the reach.
    const adj = [[1], [], []]; // importersOf: 0 imported by 1
    expect([...reachSetMulti(adj, [0, 1])].toSorted()).toEqual([]);
  });

  it("returns an empty set for empty seeds", () => {
    expect(reachSetMulti(adjUp, []).size).toBe(0);
  });
});

describe("runSearch combined blast radius", () => {
  it("collects the union upstream reach of every matched file", () => {
    // a-alpha.ts imported by b, which is imported by c: searching "alpha"
    // should mark b and c as affected.
    const d = data({
      files: [
        file({ path: "src/alpha.ts" }),
        file({ path: "src/b.ts" }),
        file({ path: "src/c.ts" }),
      ],
      edges: [
        [1, 0, 0],
        [2, 1, 0],
      ],
    });
    const state = {
      data: d,
      index: buildIndex(d),
      search: "",
      searchMatches: new Set<number>(),
      searchReach: new Set<number>(),
    } as unknown as AppState;
    runSearch(state, "alpha");
    expect(state.searchMatches.has(0)).toBe(true);
    expect([...state.searchReach].toSorted()).toEqual([1, 2]);
    expect(state.searchReach.has(0)).toBe(false);
  });

  it("computes the blast radius even for match sets past the old 40-file cap", () => {
    // 50 matches (over the retired cap) all imported by one consumer: the
    // multi-source traversal must still surface that consumer.
    const widgets = Array.from({ length: 50 }, (_, i) => file({ path: `src/widget-${i}.ts` }));
    const files = [...widgets, file({ path: "src/app.ts" })];
    const consumer = files.length - 1;
    const edges: [number, number, number][] = widgets.map((_, i) => [consumer, i, 0]);
    const d = data({ files, edges });
    const state = {
      data: d,
      index: buildIndex(d),
      search: "",
      searchMatches: new Set<number>(),
      searchReach: new Set<number>(),
    } as unknown as AppState;
    runSearch(state, "widget");
    expect(state.searchMatches.size).toBe(50);
    expect(state.searchReach.has(consumer)).toBe(true);
  });
});
