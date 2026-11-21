import { describe, expect, it } from "vitest";
import {
  buildSpatialGrid,
  clusterBounds,
  routePoint,
  fitTransform,
  getGVS,
  gridQuery,
  isTestCluster,
  middleTruncate,
  nodeHitTest,
  roadWidth,
  selectedRoadIndex,
  setStandaloneOpen,
  standaloneVisible,
  tailTruncate,
  type ClusterInfo,
  type FileNode,
} from "./shared";
import type { AppState } from "../state";
import { assignCoordinates, assignLayers, partitionEdges, tarjanSCC, type MetaEdge } from "./build";
import { centerOnFile, graphFocusSearch, graphHandleClick, reconcileGraphView } from "./interact";

/** Canvas text metrics stub: every glyph is 7px wide. */
const ctx = {
  measureText: (s: string) => ({ width: s.length * 7 }),
} as CanvasRenderingContext2D;

const cluster = (over: Partial<ClusterInfo>): ClusterInfo =>
  ({
    key: "src",
    indices: [0],
    cx: 0,
    cy: 0,
    r: 50,
    order: 0,
    layer: 0,
    tangle: false,
    isolated: false,
    hull: [],
    ...over,
  }) as ClusterInfo;

describe("text truncation", () => {
  it("keeps short strings whole", () => {
    expect(middleTruncate(ctx, "short.ts", 200)).toBe("short.ts");
    expect(tailTruncate(ctx, "src/deep", 200)).toBe("src/deep");
  });

  it("cuts the middle, preserving both ends", () => {
    const cut = middleTruncate(ctx, "a-very-long-component-name.tsx", 100);
    expect(cut).toContain("…");
    expect(cut.length * 7).toBeLessThanOrEqual(100);
    expect(cut.startsWith("a-ver")).toBe(true);
    expect(cut.endsWith("tsx")).toBe(true);
  });

  it("drops whole leading directory segments, never partial ones", () => {
    const cut = tailTruncate(ctx, "packages/design-system/src/components/", 180);
    expect(cut.startsWith("…/")).toBe(true);
    expect(cut.endsWith("components/")).toBe(true);
  });
});

describe("road and cluster helpers", () => {
  it("scales road width by log of import count, capped", () => {
    expect(roadWidth(1)).toBeLessThan(roadWidth(16));
    expect(roadWidth(100000)).toBe(8);
  });

  it("recognizes test-suite folders anywhere in the key", () => {
    expect(isTestCluster("test")).toBe(true);
    expect(isTestCluster("src/__tests__/util")).toBe(true);
    expect(isTestCluster("src/contest")).toBe(false);
  });

  it("bounds only the clusters the predicate keeps", () => {
    const clusters = [cluster({ cx: 0, r: 10 }), cluster({ cx: 100, r: 10, isolated: true })];
    const all = clusterBounds(clusters, () => true);
    const flowing = clusterBounds(clusters, (c) => !c.isolated);
    expect(all.maxX).toBe(110);
    expect(flowing.maxX).toBe(10);
  });

  it("fits content into the viewport with the label margin", () => {
    const fit = fitTransform(1600, 1000, { minX: 0, minY: 0, maxX: 700, maxY: 300 });
    expect(fit.k).toBeGreaterThan(0);
    expect(fit.k).toBeLessThanOrEqual(1.4);
    const screenRight = fit.x + 700 * fit.k;
    expect(screenRight).toBeLessThanOrEqual(1600);
  });

  it("falls back to a finite identity transform for empty bounds", () => {
    const fit = fitTransform(
      1600,
      1000,
      clusterBounds([], () => true),
    );
    expect(Number.isFinite(fit.x)).toBe(true);
    expect(Number.isFinite(fit.y)).toBe(true);
    expect(Number.isFinite(fit.k)).toBe(true);
  });

  it("keeps a positive scale on a stage narrower than the label margin", () => {
    const fit = fitTransform(120, 1000, { minX: 0, minY: 0, maxX: 700, maxY: 300 });
    expect(fit.k).toBeGreaterThan(0);
  });

  it("walks a census route by length through its bend", () => {
    const start = { x: 0, y: 0 };
    const bend = { x: 30, y: 0 };
    const end = { x: 30, y: 30 };
    expect(routePoint(start, bend, bend, end, 0)).toEqual(start);
    expect(routePoint(start, bend, bend, end, 0.5)).toEqual(bend);
    expect(routePoint(start, bend, bend, end, 0.75).y).toBeCloseTo(15);
    expect(routePoint(start, bend, bend, end, 1)).toEqual(end);
  });
});

describe("layering", () => {
  it("condenses a cycle into one strongly connected component", () => {
    const scc = tarjanSCC(3, [[1], [0], []]);
    expect(scc[0]).toBe(scc[1]);
    expect(scc[2]).not.toBe(scc[0]);
  });

  it("layers importers before their dependencies", () => {
    const meta: MetaEdge[] = [
      { src: 0, dst: 1, count: 3, violations: 0, cycleEdges: 0 },
      { src: 1, dst: 2, count: 3, violations: 0, cycleEdges: 0 },
    ];
    const layers = assignLayers(3, meta, [0, 1, 2]);
    expect(layers[0]).toBeLessThan(layers[1]);
    expect(layers[1]).toBeLessThan(layers[2]);
  });
});

describe("spatial hit grid", () => {
  /** Deterministic LCG so failures reproduce byte-for-byte. */
  const lcg = (seed: number): (() => number) => {
    let s = seed >>> 0;
    return () => {
      s = (Math.imul(s, 1664525) + 1013904223) >>> 0;
      return s / 4294967296;
    };
  };

  const syntheticState = (nodes: FileNode[], k: number): AppState => {
    const state = {} as AppState;
    const gvs = getGVS(state);
    gvs.fileNodes = nodes;
    gvs.clusters = [cluster({})];
    gvs.transform = { x: 0, y: 0, k };
    gvs.grid = buildSpatialGrid(nodes);
    setStandaloneOpen(state, gvs, true);
    return state;
  };

  /** Byte-for-byte copy of the pre-grid brute-force hit loop. */
  const bruteForceHit = (state: AppState, canvasX: number, canvasY: number): number | null => {
    const gvs = getGVS(state);
    const { transform, fileNodes, clusters } = gvs;
    const gx = (canvasX - transform.x) / transform.k;
    const gy = (canvasY - transform.y) / transform.k;
    const floor = 9 / transform.k;
    let best: number | null = null;
    let bestD = Infinity;
    const standaloneShown = standaloneVisible(state, gvs);
    for (const node of fileNodes) {
      if (!node || node.x == null || node.y == null) continue;
      if (clusters[node.cluster].isolated && !standaloneShown) continue;
      const dx = gx - node.x;
      const dy = gy - node.y;
      const d = dx * dx + dy * dy;
      const r = Math.max(node.radius + 3 / transform.k, floor);
      if (d <= r * r && d < bestD) {
        bestD = d;
        best = node.fileIndex;
      }
    }
    return best;
  };

  it("matches the brute-force hit test across zoom levels", () => {
    const rand = lcg(42);
    const nodes: FileNode[] = Array.from({ length: 200 }, (_, i) => ({
      fileIndex: i,
      cluster: 0,
      radius: 2.5 + rand() * 7.5,
      x: rand() * 2000,
      y: rand() * 1200,
    }));
    for (const k of [0.5, 1, 4]) {
      const state = syntheticState(nodes, k);
      let hits = 0;
      for (let q = 0; q < 500; q++) {
        const px = (rand() * 2100 - 50) * k;
        const py = (rand() * 1300 - 50) * k;
        const viaGrid = nodeHitTest(state, px, py);
        expect(viaGrid).toBe(bruteForceHit(state, px, py));
        if (viaGrid !== null) hits++;
      }
      // The query set must actually exercise hits, not just misses.
      expect(hits).toBeGreaterThan(0);
    }
  });

  it("covers both cells when a query point sits exactly on a cell edge", () => {
    const nodes: FileNode[] = [
      { fileIndex: 0, cluster: 0, radius: 5, x: 0, y: 0 },
      { fileIndex: 1, cluster: 0, radius: 5, x: 40, y: 0 },
    ];
    const grid = buildSpatialGrid(nodes);
    expect(grid).not.toBeNull();
    if (!grid) return;
    // cell = max(2 * 5, 40) = 40: the nodes sit in adjacent columns.
    expect(grid.cell).toBe(40);
    expect([...gridQuery(grid, 40, 0, 1)].toSorted()).toEqual([0, 1]);
    expect(gridQuery(grid, 200, 0, 1)).toEqual([]);
  });

  it("returns null for a node-less project", () => {
    expect(buildSpatialGrid([])).toBeNull();
    const state = syntheticState([], 1);
    expect(nodeHitTest(state, 10, 10)).toBeNull();
  });
});

describe("edge partitioning", () => {
  it("splits intra- from inter-cluster edges and buckets per cluster", () => {
    const clusterOf = [0, 0, 1];
    const edges: Array<[number, number, number]> = [
      [0, 1, 0],
      [1, 2, 0],
      [2, 2, 1],
    ];
    const p = partitionEdges(edges, clusterOf, 2);
    expect(p.intra).toEqual([
      [0, 1],
      [2, 2],
    ]);
    expect(p.inter).toEqual([[1, 2]]);
    expect(p.byCluster).toEqual([[[0, 1]], [[2, 2]]]);
  });

  it("skips edges whose endpoints carry no cluster assignment", () => {
    const p = partitionEdges([[0, 9, 0]], [0], 1);
    expect(p.intra).toEqual([]);
    expect(p.inter).toEqual([]);
    expect(p.byCluster).toEqual([[]]);
  });
});

describe("coordinate assignment", () => {
  const grid = (n: number, layerOf: (i: number) => number): ClusterInfo[] =>
    Array.from({ length: n }, (_, i) =>
      cluster({ key: `c${i}`, layer: layerOf(i), order: i, r: 60 }),
    );

  it("re-wraps a single-layer stack into a landscape grid", () => {
    const clusters = grid(8, () => 0);
    assignCoordinates(clusters, []);
    const b = clusterBounds(clusters, () => true);
    const aspect = (b.maxX - b.minX) / Math.max(1, b.maxY - b.minY);
    expect(aspect).toBeGreaterThanOrEqual(1);
  });

  it("keeps distinct layers in distinct columns", () => {
    const clusters = grid(4, (i) => i % 2);
    assignCoordinates(clusters, []);
    const xsByLayer = new Map<number, Set<number>>();
    for (const c of clusters) {
      if (!xsByLayer.has(c.layer)) xsByLayer.set(c.layer, new Set());
      xsByLayer.get(c.layer)?.add(c.cx);
    }
    expect(xsByLayer.get(0)?.size).toBe(1);
    expect(xsByLayer.get(1)?.size).toBe(1);
  });

  it("never overlaps rows within a layer of a landscape layout", () => {
    // Four columns of two rows: wide enough that the portrait wrap
    // stays out of the way and the stacked rows must keep their gaps.
    const clusters = grid(8, (i) => i % 4);
    assignCoordinates(clusters, []);
    for (const layer of [0, 1, 2, 3]) {
      const rows = clusters.filter((c) => c.layer === layer).toSorted((a, b) => a.cy - b.cy);
      for (let i = 1; i < rows.length; i++) {
        expect(rows[i].cy - rows[i - 1].cy).toBeGreaterThanOrEqual(rows[i].r + rows[i - 1].r);
      }
    }
  });
});

describe("graph view state", () => {
  const viewState = (): AppState => {
    const state = {
      view: "graph",
      lens: "overview",
      selected: null,
      selectedRoad: null,
      reducedMotion: true,
      requestRender: () => {},
      data: {
        files: [
          { path: "a/x.ts", status: "clean", unused_export_count: 0 },
          { path: "b/y.ts", status: "clean", unused_export_count: 0 },
          { path: "c/z.ts", status: "unused", unused_export_count: 0 },
        ],
        edges: [[0, 1, 0]],
      },
      index: {},
    } as unknown as AppState;
    const gvs = getGVS(state);
    gvs.clusters = [
      cluster({ key: "a", indices: [0], cx: 0, cy: 0 }),
      cluster({ key: "b", indices: [1], cx: 200, cy: 0 }),
      cluster({ key: "c", indices: [2], cx: 0, cy: 400, isolated: true }),
    ];
    gvs.clusterOf = [0, 1, 2];
    gvs.roads = [
      { src: 0, dst: 1, count: 1, violations: 0, cycleEdges: 0, bidi: false, back: false },
    ];
    gvs.transform = { x: 0, y: 0, k: 1 };
    gvs.initialized = true;
    return state;
  };

  const roadAB = (): NonNullable<AppState["selectedRoad"]> => ({
    srcKey: "a",
    dstKey: "b",
    count: 1,
    violations: 0,
    cycleEdges: 0,
    pairs: [],
  });

  it("derives the selected road index from the app state", () => {
    const state = viewState();
    const gvs = getGVS(state);
    expect(selectedRoadIndex(state, gvs)).toBeNull();
    state.selectedRoad = roadAB();
    expect(selectedRoadIndex(state, gvs)).toBe(0);
    state.selectedRoad = { ...roadAB(), srcKey: "b", dstKey: "a" };
    expect(selectedRoadIndex(state, gvs)).toBeNull();
  });

  it("standalone strip visibility", () => {
    const state = viewState();
    const gvs = getGVS(state);
    expect(standaloneVisible(state, gvs)).toBe(false);
    state.lens = "unused";
    expect(standaloneVisible(state, gvs)).toBe(true);
    setStandaloneOpen(state, gvs, false);
    expect(standaloneVisible(state, gvs)).toBe(false);
    expect(standaloneVisible(state, gvs)).toBe(false);
    state.lens = "overview";
    expect(standaloneVisible(state, gvs)).toBe(false);
    state.lens = "unused";
    expect(standaloneVisible(state, gvs)).toBe(true);

    const allIsolated = viewState();
    const isolatedGvs = getGVS(allIsolated);
    isolatedGvs.clusters = isolatedGvs.clusters.map((entry) => ({ ...entry, isolated: true }));
    expect(standaloneVisible(allIsolated, isolatedGvs)).toBe(true);
  });

  it("reconciles the strip after a lens change made outside the graph view", () => {
    const state = viewState();
    const gvs = getGVS(state);
    let cameraMoves = 0;
    const zoom = {
      scaleExtent: () => zoom,
      transform: () => {
        cameraMoves += 1;
      },
    };
    gvs.zoomBehavior = zoom as never;
    state.canvas = { parentElement: { clientWidth: 1000, clientHeight: 800 } } as never;
    expect(standaloneVisible(state, gvs)).toBe(false);
    state.view = "map";
    state.lens = "unused";
    reconcileGraphView(state);
    expect(cameraMoves).toBe(0);
    state.view = "graph";
    reconcileGraphView(state);
    expect(standaloneVisible(state, gvs)).toBe(true);
    // The strip opened, so the camera refits to include it.
    expect(cameraMoves).toBe(1);
  });

  it("drops a selected road that the new grouping does not have", () => {
    const regrouped = viewState();
    regrouped.selectedRoad = roadAB();
    getGVS(regrouped).clusters[1] = cluster({ key: "other", indices: [1], cx: 200, cy: 0 });
    reconcileGraphView(regrouped);
    expect(regrouped.selectedRoad).toBeNull();

    const rebuilt = viewState();
    rebuilt.selectedRoad = roadAB();
    rebuilt.data.edges.push([0, 1, 0]);
    reconcileGraphView(rebuilt);
    expect(rebuilt.selectedRoad?.pairs.length).toBe(2);
  });

  it("keeps the road panel and the road highlight in one transition", () => {
    const state = viewState();
    const gvs = getGVS(state);
    expect(graphHandleClick(state, 100, 0).kind).toBe("road");
    expect(state.selectedRoad?.srcKey).toBe("a");
    expect(state.selectedRoad?.pairs).toEqual([[0, 1]]);
    expect(selectedRoadIndex(state, gvs)).toBe(0);
    expect(graphHandleClick(state, 100, 300)).toEqual({ kind: "handled" });
    expect(state.selectedRoad).toBeNull();
    expect(selectedRoadIndex(state, gvs)).toBeNull();
    expect(graphHandleClick(state, 100, 300)).toEqual({ kind: "none" });
  });

  it("opens the standalone strip for a centered file", () => {
    const centerState = (): AppState => {
      const state = viewState();
      const gvs = getGVS(state);
      gvs.fileNodes[0] = { fileIndex: 0, radius: 4, cluster: 0, x: 0, y: 0 };
      gvs.fileNodes[2] = { fileIndex: 2, radius: 4, cluster: 2, x: 0, y: 400 };
      gvs.zoomBehavior = { transform: () => {} } as never;
      state.canvas = { parentElement: { clientWidth: 1000, clientHeight: 800 } } as never;
      return state;
    };

    const isolated = centerState();
    expect(standaloneVisible(isolated, getGVS(isolated))).toBe(false);
    centerOnFile(isolated, 2);
    expect(standaloneVisible(isolated, getGVS(isolated))).toBe(true);

    const connected = centerState();
    centerOnFile(connected, 0);
    expect(standaloneVisible(connected, getGVS(connected))).toBe(false);
  });

  it("selects the shortest other search match while a file is selected", () => {
    const state = viewState();
    state.selected = 0;
    state.data.files[1].path = "y.ts";
    state.searchMatches = new Set([2, 1]);
    expect(graphFocusSearch(state)).toBe(1);

    state.data.files[0].path = "x.ts";
    state.searchMatches = new Set([0, 2]);
    expect(graphFocusSearch(state)).toBe(2);

    state.searchMatches = new Set([0]);
    expect(graphFocusSearch(state)).toBeNull();
    state.searchMatches = new Set();
    expect(graphFocusSearch(state)).toBeNull();
  });
});
