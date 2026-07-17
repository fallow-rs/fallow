import { describe, expect, it } from "vitest";
import {
  clusterBounds,
  cubicPoint,
  fitTransform,
  isTestCluster,
  middleTruncate,
  roadWidth,
  tailTruncate,
  type ClusterInfo,
} from "./shared";
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
      const rows = clusters.filter((c) => c.layer === layer).sort((a, b) => a.cy - b.cy);
      for (let i = 1; i < rows.length; i++) {
        expect(rows[i].cy - rows[i - 1].cy).toBeGreaterThanOrEqual(rows[i].r + rows[i - 1].r);
      }
    }
  });
});
