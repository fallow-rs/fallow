import { describe, expect, it } from "vitest";
import {
  buildSpatialGrid,
  clusterBounds,
  cubicPoint,
  fitTransform,
  getGVS,
  gridQuery,
  isTestCluster,
  middleTruncate,
  nodeHitTest,
  roadWidth,
  tailTruncate,
  type ClusterInfo,
  type FileNode,
} from "./shared";
import type { AppState } from "../state";
import { assignCoordinates, assignLayers, partitionEdges, tarjanSCC, type MetaEdge } from "./build";

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

  it("interpolates a cubic bezier between its endpoints", () => {
    const p = { x: 0, y: 0 };
    const q = { x: 30, y: 0 };
    const mid = cubicPoint(p, { x: 10, y: 0 }, { x: 20, y: 0 }, q, 0.5);
    expect(mid.x).toBeCloseTo(15);
    expect(cubicPoint(p, p, q, q, 0).x).toBe(0);
    expect(cubicPoint(p, p, q, q, 1).x).toBe(30);
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
    gvs.standaloneOpen = true;
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
    for (const node of fileNodes) {
      if (!node || node.x == null || node.y == null) continue;
      if (clusters[node.cluster].isolated && !gvs.standaloneOpen) continue;
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
