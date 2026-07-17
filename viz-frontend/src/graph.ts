/**
 * Import-graph view: a layered, data-driven map of the codebase.
 *
 * Cluster placement is computed from the dependency data itself
 * (Sugiyama-lite): the cluster meta-graph is condensed (Tarjan SCC),
 * layered sink-side (entry clusters left, widely-depended-on shared
 * clusters right), ordered within layers by weighted barycenter so
 * strongly coupled clusters sit adjacent, and finally each cluster's
 * files get a frozen, seeded local force layout. Cross-cluster edges
 * never enter a simulation and are drawn as aggregated tapered "roads"
 * (wide end = importer). Selecting a file opens a screen-space ego
 * stage: importers left, imports right, over a dimmed map ghost.
 * Deterministic by construction: same input, same map.
 */
import {
  forceCollide,
  forceLink,
  forceManyBody,
  forceSimulation,
  forceX,
  forceY,
  type SimulationLinkDatum,
  type SimulationNodeDatum,
} from "d3-force";
import { select } from "d3-selection";
import { zoom, zoomIdentity, type D3ZoomEvent } from "d3-zoom";
import Graph from "graphology";
import louvain from "graphology-communities-louvain";
import type { AppState } from "./state";
import type { RoadSelection, VizFile } from "./types";
import { basename, dirname, formatCount, legendText, lensColor } from "./data";

// ── Types ───────────────────────────────────────────────────────

interface FileNode extends SimulationNodeDatum {
  fileIndex: number;
  radius: number;
  cluster: number;
}

type LocalLink = SimulationLinkDatum<FileNode>;

interface ClusterInfo {
  key: string;
  indices: number[];
  /** Dependency layer, 0 = entry side (left). */
  layer: number;
  /** Row order within the layer. */
  order: number;
  cx: number;
  cy: number;
  r: number;
  /** Padded convex hull polygon (world coords). */
  hull: Array<{ x: number; y: number }>;
  /** Member of a cluster-level dependency tangle (meta-SCC > 1). */
  tangle: boolean;
  /** No imports in either direction: parked in the standalone strip. */
  isolated: boolean;
}

interface Road {
  src: number;
  dst: number;
  count: number;
  violations: number;
  cycleEdges: number;
  /** Reverse road exists (cluster-level 2-cycle). */
  bidi: boolean;
  /** Points against the dependency axis (target layer <= source layer). */
  back: boolean;
}

interface StageRect {
  x: number;
  y: number;
  w: number;
  h: number;
  kind: "file" | "group" | "crumb";
  fileIndex?: number;
  groupKey?: string;
}

export type ClusterMode = "directory" | "imports";

export type GraphHoverTarget =
  | { kind: "file"; fileIndex: number }
  | { kind: "road"; road: number }
  | { kind: "ui" }
  | null;

export type GraphClickResult =
  | { kind: "file"; fileIndex: number }
  | { kind: "road"; road: RoadSelection }
  | { kind: "handled" }
  | { kind: "none" };

interface GraphViewState {
  fileNodes: FileNode[];
  clusters: ClusterInfo[];
  clusterOf: number[];
  roads: Road[];
  /** Importer-count floor above which a node gets the hub badge. */
  hubFloor: number;
  transform: { x: number; y: number; k: number };
  fitK: number;
  initialized: boolean;
  clusterMode: ClusterMode;
  zoomBehavior: ReturnType<typeof zoom<HTMLCanvasElement, unknown>> | null;
  /** Ego stage state. */
  egoExpanded: Set<string>;
  crumbs: number[];
  stageRects: StageRect[];
  stageEnterAt: number;
  lastRoot: number | null;
  raf: number;
  /** Hovered road index (overview). */
  hoveredRoad: number | null;
  /** Selected road index (overview drill-down). */
  selectedRoad: number | null;
  /** Path-trace mode: pending start, and the traced path. */
  pathFrom: number | null;
  path: number[] | null;
  /** Search pulse: file index + start timestamp. */
  pulseFile: number | null;
  pulseAt: number;
  /** Transient HUD notice (e.g. "no dependency path found"). */
  notice: string;
  noticeAt: number;
  /** Standalone strip expanded state + chip hit rect (screen space). */
  standaloneOpen: boolean;
  standaloneChip: { x: number; y: number; w: number; h: number } | null;
  /** Ego-view "back to map" chip hit rect (screen space). */
  egoBackChip: { x: number; y: number; w: number; h: number } | null;
  /** True once the user pans/zooms; blocks auto-refit on window resize. */
  userMoved: boolean;
  /** First-run captions synced to the reveal. */
  showIntro: boolean;
  /** First-render reveal choreography start (0 = pending, -1 = skipped). */
  revealAt: number;
}

const FONT_SMALL = '10px "Martian Mono", "JetBrains Mono", ui-monospace, Menlo, monospace';
const FONT_MICRO = '9px "Martian Mono", "JetBrains Mono", ui-monospace, Menlo, monospace';
const FONT_CHIP = '11px "Martian Mono", "JetBrains Mono", ui-monospace, Menlo, monospace';
const FONT_LEGEND = '10px "Martian Mono", "JetBrains Mono", ui-monospace, Menlo, monospace';
const FONT_CARD = '700 13px "Martian Mono", "JetBrains Mono", ui-monospace, Menlo, monospace';

const NODE_R_MIN = 2.5;
const NODE_R_MAX = 10;
const MAX_CLUSTERS = 40;
const LAYER_GAP = 170;
const ROW_GAP = 56;
const STAGE_ENTER_MS = 220;
/** Relative-zoom LOD thresholds (k / fit-to-view k). */
const LOD_INTRA = 1.6;
const LOD_INTER = 3.0;
const LOD_SEVERITY = 0.9;

// ── Deterministic randomness ────────────────────────────────────

const fnv1a = (s: string): number => {
  let h = 0x811c9dc5;
  for (let i = 0; i < s.length; i++) {
    h ^= s.charCodeAt(i);
    h = Math.imul(h, 0x01000193);
  }
  return h >>> 0;
};

const mulberry32 = (seed: number): (() => number) => {
  let a = seed >>> 0;
  return () => {
    a |= 0;
    a = (a + 0x6d2b79f5) | 0;
    let t = Math.imul(a ^ (a >>> 15), 1 | a);
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
};

// ── State accessor ──────────────────────────────────────────────

const getGVS = (state: AppState): GraphViewState => {
  const ext = state as AppState & { _gvs?: GraphViewState };
  if (!ext._gvs) {
    ext._gvs = {
      fileNodes: [],
      clusters: [],
      clusterOf: [],
      roads: [],
      hubFloor: Infinity,
      transform: { x: 0, y: 0, k: 1 },
      fitK: 1,
      initialized: false,
      clusterMode: "directory",
      zoomBehavior: null,
      egoExpanded: new Set(),
      crumbs: [],
      stageRects: [],
      stageEnterAt: 0,
      lastRoot: null,
      raf: 0,
      hoveredRoad: null,
      selectedRoad: null,
      pathFrom: null,
      path: null,
      pulseFile: null,
      pulseAt: 0,
      notice: "",
      noticeAt: 0,
      revealAt: 0,
      standaloneOpen: false,
      standaloneChip: null,
      egoBackChip: null,
      userMoved: false,
      showIntro: false,
    };
  }
  return ext._gvs;
};

export const getClusterMode = (state: AppState): ClusterMode => getGVS(state).clusterMode;

export const setClusterMode = (state: AppState, mode: ClusterMode): void => {
  const gvs = getGVS(state);
  if (gvs.clusterMode === mode) return;
  gvs.clusterMode = mode;
  gvs.initialized = false;
  initGraphNodes(state);
};

// ── Clustering ──────────────────────────────────────────────────

const directoryCluster = (files: VizFile[]): Map<string, number[]> => {
  const clusters = new Map<string, { indices: number[]; depth: number }>();
  for (let i = 0; i < files.length; i++) {
    const key = files[i].path.split("/")[0];
    const existing = clusters.get(key);
    if (existing) existing.indices.push(i);
    else clusters.set(key, { indices: [i], depth: 0 });
  }

  for (let round = 0; round < 10; round++) {
    if (clusters.size >= MAX_CLUSTERS) break;
    let largestKey = "";
    let largestSize = 0;
    for (const [key, { indices }] of clusters) {
      if (indices.length > largestSize) {
        largestSize = indices.length;
        largestKey = key;
      }
    }
    if (largestSize <= Math.max(20, files.length / MAX_CLUSTERS)) break;
    const largest = clusters.get(largestKey);
    if (!largest) break;
    const nextDepth = largest.depth + 1;
    const subMap = new Map<string, number[]>();
    for (const idx of largest.indices) {
      const parts = files[idx].path.split("/");
      const key =
        parts.length > nextDepth + 1 ? parts.slice(0, nextDepth + 1).join("/") : parts.join("/");
      if (!subMap.has(key)) subMap.set(key, []);
      subMap.get(key)?.push(idx);
    }
    if (subMap.size <= 1 || clusters.size + subMap.size - 1 > MAX_CLUSTERS) break;
    clusters.delete(largestKey);
    for (const [key, indices] of subMap) clusters.set(key, { indices, depth: nextDepth });
  }

  const result = new Map<string, number[]>();
  for (const key of [...clusters.keys()].sort()) {
    const entry = clusters.get(key);
    if (entry) result.set(key, entry.indices);
  }
  return result;
};

const louvainCluster = (
  files: VizFile[],
  edges: [number, number, number][],
): Map<string, number[]> => {
  const g = new Graph({ type: "undirected" });
  for (let i = 0; i < files.length; i++) g.addNode(String(i));
  const seen = new Set<string>();
  for (const [src, tgt] of edges) {
    if (src >= files.length || tgt >= files.length || src === tgt) continue;
    const key = src < tgt ? `${src}-${tgt}` : `${tgt}-${src}`;
    if (seen.has(key)) continue;
    seen.add(key);
    g.addEdge(String(src), String(tgt));
  }
  const communities = louvain(g, { resolution: 1.2, rng: mulberry32(fnv1a("fallow-louvain")) });
  const communityMap = new Map<number, number[]>();
  for (let i = 0; i < files.length; i++) {
    const comm = communities[String(i)] ?? 0;
    if (!communityMap.has(comm)) communityMap.set(comm, []);
    communityMap.get(comm)?.push(i);
  }
  const result = new Map<string, number[]>();
  for (const [, indices] of communityMap) {
    const dirCounts = new Map<string, number>();
    for (const idx of indices) {
      const parts = files[idx].path.split("/");
      const dir = parts.length > 1 ? parts.slice(0, 2).join("/") : parts[0];
      dirCounts.set(dir, (dirCounts.get(dir) ?? 0) + 1);
    }
    const sorted = [...dirCounts.entries()].sort((a, b) => b[1] - a[1] || (a[0] < b[0] ? -1 : 1));
    let name = sorted[0]?.[0] ?? "misc";
    while (result.has(name)) name = `${name}*`;
    result.set(name, indices);
  }
  return new Map([...result.entries()].sort((a, b) => (a[0] < b[0] ? -1 : 1)));
};

// ── Meta-graph, SCC condensation, layering, ordering ────────────

interface MetaEdge {
  src: number;
  dst: number;
  count: number;
  violations: number;
  cycleEdges: number;
}

const buildMetaGraph = (
  state: AppState,
  clusterOf: number[],
  clusterCount: number,
): MetaEdge[] => {
  const n = state.data.files.length;
  const buckets = new Map<number, MetaEdge>();
  for (const [from, to] of state.data.edges) {
    const a = clusterOf[from];
    const b = clusterOf[to];
    if (a === undefined || b === undefined || a === b) continue;
    const key = a * clusterCount + b;
    let edge = buckets.get(key);
    if (!edge) {
      edge = { src: a, dst: b, count: 0, violations: 0, cycleEdges: 0 };
      buckets.set(key, edge);
    }
    edge.count++;
    const packed = from * n + to;
    if (state.index.violationEdges.has(packed)) edge.violations++;
    if (state.index.cycleEdges.has(packed)) edge.cycleEdges++;
  }
  return [...buckets.values()].sort((a, b) => a.src - b.src || a.dst - b.dst);
};

/** Iterative Tarjan SCC over the cluster meta-graph. */
const tarjanSCC = (count: number, adj: number[][]): number[] => {
  const sccOf = new Array<number>(count).fill(-1);
  const low = new Array<number>(count).fill(0);
  const disc = new Array<number>(count).fill(-1);
  const onStack = new Array<boolean>(count).fill(false);
  const stack: number[] = [];
  let time = 0;
  let sccCount = 0;

  for (let start = 0; start < count; start++) {
    if (disc[start] !== -1) continue;
    const work: Array<[number, number]> = [[start, 0]];
    while (work.length > 0) {
      const frame = work[work.length - 1];
      const v = frame[0];
      if (frame[1] === 0) {
        disc[v] = low[v] = time++;
        stack.push(v);
        onStack[v] = true;
      }
      let advanced = false;
      while (frame[1] < adj[v].length) {
        const w = adj[v][frame[1]];
        frame[1]++;
        if (disc[w] === -1) {
          work.push([w, 0]);
          advanced = true;
          break;
        }
        if (onStack[w]) low[v] = Math.min(low[v], disc[w]);
      }
      if (advanced) continue;
      if (low[v] === disc[v]) {
        for (;;) {
          const w = stack.pop();
          if (w === undefined) break;
          onStack[w] = false;
          sccOf[w] = sccCount;
          if (w === v) break;
        }
        sccCount++;
      }
      work.pop();
      if (work.length > 0) {
        const parent = work[work.length - 1][0];
        low[parent] = Math.min(low[parent], low[v]);
      }
    }
  }
  return sccOf;
};

/**
 * Sink-side longest-path layering on the SCC condensation, mirrored so
 * entry clusters (nothing imports them) sit at layer 0 (left) and the
 * most depended-on foundations sit at the highest layer (right).
 */
const assignLayers = (clusterCount: number, meta: MetaEdge[], sccOf: number[]): number[] => {
  const sccCount = sccOf.reduce((max, s) => Math.max(max, s), -1) + 1;
  const succ: Array<Set<number>> = Array.from({ length: sccCount }, () => new Set());
  for (const e of meta) {
    const a = sccOf[e.src];
    const b = sccOf[e.dst];
    if (a !== b) succ[a].add(b);
  }
  const memo = new Array<number>(sccCount).fill(-1);
  const depth = (s: number): number => {
    if (memo[s] !== -1) return memo[s];
    memo[s] = 0; // provisional (condensation is acyclic; guards reentry)
    let best = 0;
    for (const t of succ[s]) best = Math.max(best, 1 + depth(t));
    memo[s] = best;
    return best;
  };
  let maxLayer = 0;
  for (let s = 0; s < sccCount; s++) maxLayer = Math.max(maxLayer, depth(s));
  const layers = new Array<number>(clusterCount);
  for (let c = 0; c < clusterCount; c++) {
    layers[c] = maxLayer - memo[sccOf[c]];
  }
  return layers;
};

/** 4 weighted barycenter sweeps for within-layer ordering. */
const orderWithinLayers = (clusters: ClusterInfo[], meta: MetaEdge[]): void => {
  const byLayer = new Map<number, ClusterInfo[]>();
  for (const c of clusters) {
    if (c.isolated) continue;
    if (!byLayer.has(c.layer)) byLayer.set(c.layer, []);
    byLayer.get(c.layer)?.push(c);
  }
  for (const list of byLayer.values()) {
    list.sort((a, b) => (a.key < b.key ? -1 : 1));
    list.forEach((c, i) => {
      c.order = i;
    });
  }

  const neighbors = new Map<number, Array<{ other: number; w: number }>>();
  clusters.forEach((_, i) => neighbors.set(i, []));
  for (const e of meta) {
    neighbors.get(e.src)?.push({ other: e.dst, w: e.count });
    neighbors.get(e.dst)?.push({ other: e.src, w: e.count });
  }

  const layerKeys = [...byLayer.keys()].sort((a, b) => a - b);
  const indexOf = new Map<string, number>();
  clusters.forEach((c, i) => indexOf.set(c.key, i));

  const sweep = (keys: number[]): void => {
    for (const layer of keys) {
      const list = byLayer.get(layer);
      if (!list || list.length < 2) continue;
      const scored = list.map((c) => {
        const idx = indexOf.get(c.key) ?? 0;
        let num = 0;
        let den = 0;
        for (const nb of neighbors.get(idx) ?? []) {
          const other = clusters[nb.other];
          if (Math.abs(other.layer - layer) !== 1) continue;
          num += nb.w * other.order;
          den += nb.w;
        }
        return { c, bary: den > 0 ? num / den : c.order };
      });
      scored.sort((a, b) => a.bary - b.bary || (a.c.key < b.c.key ? -1 : 1));
      scored.forEach((s, i) => {
        s.c.order = i;
      });
    }
  };

  sweep(layerKeys);
  sweep([...layerKeys].reverse());
  sweep(layerKeys);
  sweep([...layerKeys].reverse());
};

/** Coordinate assignment: x per layer, y stacked by order + relaxation. */
const assignCoordinates = (clusters: ClusterInfo[], meta: MetaEdge[]): void => {
  const byLayer = new Map<number, ClusterInfo[]>();
  for (const c of clusters) {
    if (c.isolated) continue;
    if (!byLayer.has(c.layer)) byLayer.set(c.layer, []);
    byLayer.get(c.layer)?.push(c);
  }
  const layerKeys = [...byLayer.keys()].sort((a, b) => a - b);

  let x = 0;
  let prevMaxR = 0;
  layerKeys.forEach((layer, i) => {
    const list = byLayer.get(layer) ?? [];
    const maxR = list.reduce((max, c) => Math.max(max, c.r), 30);
    if (i > 0) x += prevMaxR + maxR + LAYER_GAP;
    for (const c of list) c.cx = x;
    prevMaxR = maxR;
  });

  for (const layer of layerKeys) {
    const list = (byLayer.get(layer) ?? []).sort((a, b) => a.order - b.order);
    let y = 0;
    list.forEach((c, i) => {
      if (i > 0) y += list[i - 1].r + c.r + ROW_GAP;
      c.cy = y;
    });
  }

  const indexOf = new Map<string, number>();
  clusters.forEach((c, i) => indexOf.set(c.key, i));
  const adjacency = new Map<number, Array<{ other: number; w: number }>>();
  clusters.forEach((_, i) => adjacency.set(i, []));
  for (const e of meta) {
    adjacency.get(e.src)?.push({ other: e.dst, w: e.count });
    adjacency.get(e.dst)?.push({ other: e.src, w: e.count });
  }
  for (let pass = 0; pass < 3; pass++) {
    for (const layer of layerKeys) {
      const list = (byLayer.get(layer) ?? []).sort((a, b) => a.order - b.order);
      for (const c of list) {
        const idx = indexOf.get(c.key) ?? 0;
        let num = 0;
        let den = 0;
        for (const nb of adjacency.get(idx) ?? []) {
          num += nb.w * clusters[nb.other].cy;
          den += nb.w;
        }
        if (den > 0) c.cy = (c.cy + num / den) / 2;
      }
      for (let i = 1; i < list.length; i++) {
        const minY = list[i - 1].cy + list[i - 1].r + list[i].r + ROW_GAP;
        if (list[i].cy < minY) list[i].cy = minY;
      }
    }
  }

  const flowing = clusters.filter((c) => !c.isolated);
  const globalMid = flowing.reduce((sum, c) => sum + c.cy, 0) / Math.max(1, flowing.length);
  for (const layer of layerKeys) {
    const list = byLayer.get(layer) ?? [];
    const mid = list.reduce((s, c) => s + c.cy, 0) / Math.max(1, list.length);
    for (const c of list) c.cy += globalMid - mid;
  }

  // Import-community clustering collapses into few layers (the
  // communities import each other), which the rank layout stacks as a
  // tall column. When the result is portrait, re-wrap the clusters
  // into rows targeting a wide aspect instead.
  const tallMinX = Math.min(...flowing.map((c) => c.cx - c.r));
  const tallMaxX = Math.max(...flowing.map((c) => c.cx + c.r));
  const tallMinY = Math.min(...flowing.map((c) => c.cy - c.r));
  const tallMaxY = Math.max(...flowing.map((c) => c.cy + c.r));
  const tallAspect = (tallMaxX - tallMinX) / Math.max(1, tallMaxY - tallMinY);
  if (tallAspect < 1 && flowing.length > 3) {
    const GRID_GAP = 150;
    const list = [...flowing].sort((a, b) => a.layer - b.layer || a.cy - b.cy);
    const totalW = list.reduce((sum, c) => sum + c.r * 2 + GRID_GAP, 0);
    const avgRowH = list.reduce((sum, c) => sum + c.r * 2, 0) / list.length + GRID_GAP;
    const rowW = Math.max(
      Math.sqrt(2 * totalW * avgRowH),
      Math.max(...list.map((c) => c.r * 2 + GRID_GAP)),
    );
    let x = 0;
    let rowTop = 0;
    let rowMaxR = 0;
    const flushRow = (row: ClusterInfo[]): void => {
      for (const c of row) c.cy = rowTop + rowMaxR;
      rowTop += rowMaxR * 2 + GRID_GAP;
    };
    let row: ClusterInfo[] = [];
    for (const c of list) {
      if (x + c.r * 2 > rowW && row.length > 0) {
        flushRow(row);
        row = [];
        x = 0;
        rowMaxR = 0;
      }
      c.cx = x + c.r;
      x += c.r * 2 + GRID_GAP;
      rowMaxR = Math.max(rowMaxR, c.r);
      row.push(c);
    }
    flushRow(row);
    return;
  }

  // The rank layout tends toward a flat ribbon (aspect 4:1+) that leaves
  // half the viewport empty. Spread rows vertically toward a presentable
  // aspect and stagger single-cluster layers off the midline; x gaps
  // between layers make the stagger collision-free.
  if (flowing.length > 1) {
    const minX = Math.min(...flowing.map((c) => c.cx - c.r));
    const maxX = Math.max(...flowing.map((c) => c.cx + c.r));
    const minY = Math.min(...flowing.map((c) => c.cy - c.r));
    const maxY = Math.max(...flowing.map((c) => c.cy + c.r));
    const aspect = (maxX - minX) / Math.max(1, maxY - minY);
    const TARGET_ASPECT = 2.2;
    if (aspect > TARGET_ASPECT) {
      const factor = Math.min(2.6, aspect / TARGET_ASPECT);
      for (const c of flowing) c.cy = globalMid + (c.cy - globalMid) * factor;
      let flip = -1;
      for (const layer of layerKeys) {
        const list = byLayer.get(layer) ?? [];
        if (list.length === 1) {
          list[0].cy += flip * Math.min(240, list[0].r + 100) * Math.min(1, factor - 0.6);
          flip = -flip;
        }
      }
    }
  }
};

/** Park isolated clusters in a compact strip below the dependency flow. */
const placeIsolated = (clusters: ClusterInfo[]): void => {
  const isolated = clusters.filter((c) => c.isolated).sort((a, b) => (a.key < b.key ? -1 : 1));
  if (isolated.length === 0) return;
  const flowing = clusters.filter((c) => !c.isolated);
  let minX = 0;
  let maxX = 800;
  let maxY = 0;
  if (flowing.length > 0) {
    minX = Math.min(...flowing.map((c) => c.cx - c.r));
    maxX = Math.max(...flowing.map((c) => c.cx + c.r));
    maxY = Math.max(...flowing.map((c) => c.cy + c.r));
  }
  let x = minX;
  let y = maxY + 200;
  let rowMax = 0;
  for (const c of isolated) {
    if (x + c.r * 2 > maxX && x > minX) {
      x = minX;
      y += rowMax + 90;
      rowMax = 0;
    }
    c.cx = x + c.r;
    c.cy = y + c.r;
    x += c.r * 2 + 120;
    rowMax = Math.max(rowMax, c.r * 2);
  }
};

// ── Local per-cluster layouts (frozen, seeded) ──────────────────

const runLocalLayouts = (state: AppState, gvs: GraphViewState): void => {
  const files = state.data.files;
  const maxSize = files.reduce((max, f) => Math.max(max, f.size), 1);

  for (let ci = 0; ci < gvs.clusters.length; ci++) {
    const cluster = gvs.clusters[ci];
    const rand = mulberry32(fnv1a(cluster.key));
    const nodes: FileNode[] = cluster.indices.map((fileIndex, i) => {
      // Phyllotaxis init in path-sorted member order: deterministic.
      const angle = i * 2.399963229728653;
      const radius = 6 * Math.sqrt(i + 0.5);
      const sizeRatio = Math.log(files[fileIndex].size + 1) / Math.log(maxSize + 1);
      return {
        fileIndex,
        cluster: ci,
        radius: NODE_R_MIN + sizeRatio * (NODE_R_MAX - NODE_R_MIN),
        x: cluster.cx + Math.cos(angle) * radius,
        y: cluster.cy + Math.sin(angle) * radius,
      };
    });
    const inCluster = new Map<number, FileNode>();
    for (const node of nodes) inCluster.set(node.fileIndex, node);

    const links: LocalLink[] = [];
    for (const [from, to] of state.data.edges) {
      const a = inCluster.get(from);
      const b = inCluster.get(to);
      if (a && b && a !== b) links.push({ source: a, target: b });
    }

    const sim = forceSimulation(nodes)
      .randomSource(rand)
      .force("link", forceLink<FileNode, LocalLink>(links).distance(24).strength(0.3))
      .force("charge", forceManyBody<FileNode>().strength(-30).theta(0.9).distanceMax(240))
      .force("collide", forceCollide<FileNode>((d) => d.radius + 2))
      .force("x", forceX<FileNode>(cluster.cx).strength(0.15))
      .force("y", forceY<FileNode>(cluster.cy).strength(0.15))
      .alphaDecay(0.028)
      .stop();
    const ticks = Math.min(300, 120 + cluster.indices.length * 2);
    for (let t = 0; t < ticks; t++) sim.tick();
    sim.stop();

    for (const node of nodes) gvs.fileNodes[node.fileIndex] = node;
  }
};

// ── Hull polygons ───────────────────────────────────────────────

interface Pt {
  x: number;
  y: number;
}

const convexHull = (pts: Pt[]): Pt[] => {
  const sorted = [...pts].sort((a, b) => a.x - b.x || a.y - b.y);
  if (sorted.length < 3) return sorted;
  const cross = (o: Pt, a: Pt, b: Pt): number =>
    (a.x - o.x) * (b.y - o.y) - (a.y - o.y) * (b.x - o.x);
  const lower: Pt[] = [];
  for (const p of sorted) {
    while (lower.length >= 2 && cross(lower[lower.length - 2], lower[lower.length - 1], p) <= 0) {
      lower.pop();
    }
    lower.push(p);
  }
  const upper: Pt[] = [];
  for (let i = sorted.length - 1; i >= 0; i--) {
    const p = sorted[i];
    while (upper.length >= 2 && cross(upper[upper.length - 2], upper[upper.length - 1], p) <= 0) {
      upper.pop();
    }
    upper.push(p);
  }
  lower.pop();
  upper.pop();
  return lower.concat(upper);
};

const buildHulls = (gvs: GraphViewState): void => {
  for (const cluster of gvs.clusters) {
    const pts = cluster.indices
      .map((i) => gvs.fileNodes[i])
      .filter((node) => node && node.x != null && node.y != null)
      .map((node) => ({ x: node.x ?? 0, y: node.y ?? 0 }));
    let cx = 0;
    let cy = 0;
    for (const p of pts) {
      cx += p.x;
      cy += p.y;
    }
    cx /= Math.max(1, pts.length);
    cy /= Math.max(1, pts.length);
    cluster.cx = cx;
    cluster.cy = cy;

    let hull: Pt[];
    if (pts.length < 3) {
      hull = [];
      const r = cluster.r * 0.5 + 20;
      for (let i = 0; i < 8; i++) {
        const a = (i / 8) * Math.PI * 2;
        hull.push({ x: cx + Math.cos(a) * r, y: cy + Math.sin(a) * r });
      }
    } else {
      hull = convexHull(pts).map((p) => {
        const dx = p.x - cx;
        const dy = p.y - cy;
        const d = Math.max(1, Math.hypot(dx, dy));
        const pad = 20;
        return { x: cx + dx * ((d + pad) / d), y: cy + dy * ((d + pad) / d) };
      });
    }
    cluster.hull = hull;
    let maxD = 0;
    for (const p of hull) maxD = Math.max(maxD, Math.hypot(p.x - cx, p.y - cy));
    cluster.r = maxD;
  }
};

// ── Init ────────────────────────────────────────────────────────

export const initGraphNodes = (state: AppState): void => {
  const { data, canvas } = state;
  const gvs = getGVS(state);
  if (gvs.initialized) {
    renderGraph(state);
    return;
  }

  const files = data.files;
  const groupMap =
    gvs.clusterMode === "imports" ? louvainCluster(files, data.edges) : directoryCluster(files);

  const clusterOf = new Array<number>(files.length).fill(0);
  gvs.clusterOf = clusterOf;
  const clusters: ClusterInfo[] = [];
  for (const [key, indices] of groupMap) {
    const ci = clusters.length;
    for (const idx of indices) clusterOf[idx] = ci;
    clusters.push({
      key,
      indices,
      layer: 0,
      order: 0,
      cx: 0,
      cy: 0,
      r: 24 + 9 * Math.sqrt(indices.length),
      hull: [],
      tangle: false,
      isolated: false,
    });
  }
  gvs.clusters = clusters;

  const meta = buildMetaGraph(state, clusterOf, clusters.length);
  const adj: number[][] = Array.from({ length: clusters.length }, () => []);
  for (const e of meta) adj[e.src].push(e.dst);
  const sccOf = tarjanSCC(clusters.length, adj);
  const sccSize = new Map<number, number>();
  for (const s of sccOf) sccSize.set(s, (sccSize.get(s) ?? 0) + 1);
  clusters.forEach((c, i) => {
    c.tangle = (sccSize.get(sccOf[i]) ?? 1) > 1;
  });
  // Clusters with no inter-cluster imports at all sit outside the flow:
  // park them in a standalone strip below the map instead of polluting
  // the entry/shared columns.
  const connected = new Set<number>();
  for (const e of meta) {
    connected.add(e.src);
    connected.add(e.dst);
  }
  clusters.forEach((c, i) => {
    c.isolated = !connected.has(i);
  });

  const layers = assignLayers(clusters.length, meta, sccOf);
  clusters.forEach((c, i) => {
    c.layer = layers[i];
  });
  orderWithinLayers(clusters, meta);
  assignCoordinates(clusters, meta);
  placeIsolated(clusters);

  gvs.fileNodes = new Array<FileNode>(files.length);
  runLocalLayouts(state, gvs);
  buildHulls(gvs);

  // Hub floor: p95 of importer counts, min 25 (spec: badge, never suppress).
  const importerCounts = files
    .map((f) => f.importer_count)
    .filter((c) => c > 0)
    .sort((a, b) => a - b);
  const p95 =
    importerCounts.length > 0
      ? importerCounts[Math.min(importerCounts.length - 1, Math.floor(importerCounts.length * 0.95))]
      : Infinity;
  gvs.hubFloor = Math.max(25, p95);

  const pairSet = new Set<number>();
  for (const e of meta) pairSet.add(e.src * clusters.length + e.dst);
  gvs.roads = meta.map((e) => ({
    src: e.src,
    dst: e.dst,
    count: e.count,
    violations: e.violations,
    cycleEdges: e.cycleEdges,
    bidi: pairSet.has(e.dst * clusters.length + e.src),
    back: clusters[e.dst].layer <= clusters[e.src].layer,
  }));

  // Fit-to-view.
  const stageEl = canvas.parentElement;
  const w = stageEl ? stageEl.clientWidth : window.innerWidth;
  const h = stageEl ? stageEl.clientHeight : window.innerHeight;
  let minX = Infinity;
  let minY = Infinity;
  let maxX = -Infinity;
  let maxY = -Infinity;
  const anyConnected = clusters.some((c) => !c.isolated);
  for (const c of clusters) {
    // Standalone clusters are hidden until toggled: keep them out of the fit.
    if (c.isolated && anyConnected) continue;
    minX = Math.min(minX, c.cx - c.r);
    minY = Math.min(minY, c.cy - c.r);
    maxX = Math.max(maxX, c.cx + c.r);
    maxY = Math.max(maxY, c.cy + c.r);
  }
  const pad = 70;
  const bboxW = maxX - minX + pad * 2;
  const bboxH = maxY - minY + pad * 2;
  // Reserve horizontal screen room for cluster labels that stick out of hulls.
  const fitScale = Math.min((w - 200) / bboxW, (h - 60) / bboxH, 1.4);
  const fitX = (w - bboxW * fitScale) / 2 - minX * fitScale + pad * fitScale;
  const fitY = (h - bboxH * fitScale) / 2 - minY * fitScale + pad * fitScale;
  gvs.transform = { x: fitX, y: fitY, k: fitScale };
  gvs.fitK = fitScale;

  const zoomBehavior = zoom<HTMLCanvasElement, unknown>()
    .scaleExtent([fitScale * 0.4, fitScale * 12])
    .filter((event: MouseEvent | WheelEvent) => {
      if (state.selected !== null) return false; // camera frozen in ego mode
      if (event.type === "wheel") return !event.ctrlKey;
      if ((event as MouseEvent).button !== 0) return true;
      const rect = canvas.getBoundingClientRect();
      const px = (event as MouseEvent).clientX - rect.left;
      const py = (event as MouseEvent).clientY - rect.top;
      return nodeHitTest(state, px, py) === null;
    })
    .on("zoom", (event: D3ZoomEvent<HTMLCanvasElement, unknown>) => {
      if (event.sourceEvent) gvs.userMoved = true;
      gvs.transform = { x: event.transform.x, y: event.transform.y, k: event.transform.k };
      renderGraph(state);
    });
  const initialTransform = zoomIdentity.translate(fitX, fitY).scale(fitScale);
  select(canvas).call(zoomBehavior).call(zoomBehavior.transform, initialTransform);
  gvs.zoomBehavior = zoomBehavior;

  gvs.initialized = true;
  gvs.revealAt = 0;
  gvs.showIntro = shouldShowIntro() && !state.reducedMotion;
  renderGraph(state);
};

const INTRO_KEY = "fallow-viz-intro-seen";

const shouldShowIntro = (): boolean => {
  try {
    if (new URLSearchParams(window.location.search).get("intro") === "1") return true;
    return window.localStorage.getItem(INTRO_KEY) === null;
  } catch {
    return true;
  }
};

const markIntroSeen = (): void => {
  try {
    window.localStorage.setItem(INTRO_KEY, "1");
  } catch {
    // Storage unavailable (some file:// contexts): show it again next time.
  }
};

// ── Geometry helpers ────────────────────────────────────────────

const segIntersect = (a1: Pt, a2: Pt, b1: Pt, b2: Pt): Pt | null => {
  const d1x = a2.x - a1.x;
  const d1y = a2.y - a1.y;
  const d2x = b2.x - b1.x;
  const d2y = b2.y - b1.y;
  const denom = d1x * d2y - d1y * d2x;
  if (Math.abs(denom) < 1e-9) return null;
  const t = ((b1.x - a1.x) * d2y - (b1.y - a1.y) * d2x) / denom;
  const u = ((b1.x - a1.x) * d1y - (b1.y - a1.y) * d1x) / denom;
  if (t < 0 || t > 1 || u < 0 || u > 1) return null;
  return { x: a1.x + t * d1x, y: a1.y + t * d1y };
};

/** Where the segment from a cluster's centre toward `toward` leaves its hull. */
const gatePoint = (cluster: ClusterInfo, toward: Pt): Pt => {
  const from = { x: cluster.cx, y: cluster.cy };
  const hull = cluster.hull;
  for (let i = 0; i < hull.length; i++) {
    const hit = segIntersect(from, toward, hull[i], hull[(i + 1) % hull.length]);
    if (hit) return hit;
  }
  return from;
};

const cubicPoint = (p0: Pt, p1: Pt, p2: Pt, p3: Pt, t: number): Pt => {
  const u = 1 - t;
  return {
    x: u * u * u * p0.x + 3 * u * u * t * p1.x + 3 * u * t * t * p2.x + t * t * t * p3.x,
    y: u * u * u * p0.y + 3 * u * u * t * p1.y + 3 * u * t * t * p2.y + t * t * t * p3.y,
  };
};

/** Trace a tapered ribbon polygon along a cubic bezier into the current path. */
const taperedRibbon = (
  ctx: CanvasRenderingContext2D,
  p0: Pt,
  p1: Pt,
  p2: Pt,
  p3: Pt,
  wSrc: number,
  wDst: number,
): void => {
  const SAMPLES = 20;
  const centers: Pt[] = [];
  for (let i = 0; i <= SAMPLES; i++) centers.push(cubicPoint(p0, p1, p2, p3, i / SAMPLES));
  const left: Pt[] = [];
  const right: Pt[] = [];
  for (let i = 0; i <= SAMPLES; i++) {
    const t = i / SAMPLES;
    const prev = centers[Math.max(0, i - 1)];
    const next = centers[Math.min(SAMPLES, i + 1)];
    const dx = next.x - prev.x;
    const dy = next.y - prev.y;
    const len = Math.max(1e-6, Math.hypot(dx, dy));
    const nx = -dy / len;
    const ny = dx / len;
    const hw = (wSrc * (1 - t) + wDst * t) / 2;
    left.push({ x: centers[i].x + nx * hw, y: centers[i].y + ny * hw });
    right.push({ x: centers[i].x - nx * hw, y: centers[i].y - ny * hw });
  }
  ctx.moveTo(left[0].x, left[0].y);
  for (let i = 1; i <= SAMPLES; i++) ctx.lineTo(left[i].x, left[i].y);
  for (let i = SAMPLES; i >= 0; i--) ctx.lineTo(right[i].x, right[i].y);
  ctx.closePath();
};

const roadGeometry = (gvs: GraphViewState, road: Road): { p0: Pt; p1: Pt; p2: Pt; p3: Pt } => {
  const src = gvs.clusters[road.src];
  const dst = gvs.clusters[road.dst];
  let p0 = gatePoint(src, { x: dst.cx, y: dst.cy });
  let p3 = gatePoint(dst, { x: src.cx, y: src.cy });

  if (road.bidi) {
    // Two one-way lanes, offset perpendicular to the chord.
    const dx = p3.x - p0.x;
    const dy = p3.y - p0.y;
    const len = Math.max(1e-6, Math.hypot(dx, dy));
    const nx = (-dy / len) * 6;
    const ny = (dx / len) * 6;
    p0 = { x: p0.x + nx, y: p0.y + ny };
    p3 = { x: p3.x + nx, y: p3.y + ny };
  }

  const dx = p3.x - p0.x;
  let bow = 0;
  const chord = Math.hypot(p3.x - p0.x, p3.y - p0.y);
  if (road.back) {
    bow = -0.18 * chord; // back-edges arc above the fabric
  } else {
    const span = Math.abs(gvs.clusters[road.dst].layer - gvs.clusters[road.src].layer);
    // Long hops bow gently over intermediate layers instead of cutting
    // through their hulls.
    if (span >= 2) bow = -0.06 * chord;
  }
  const p1 = { x: p0.x + dx * 0.45, y: p0.y + bow };
  const p2 = { x: p3.x - dx * 0.45, y: p3.y + bow };
  return { p0, p1, p2, p3 };
};

/** Rounded chip backing: fill plus 1px border, radius 4. */
const chipRect = (
  ctx: CanvasRenderingContext2D,
  x: number,
  y: number,
  w: number,
  h: number,
  fill: string,
  fillAlpha: number,
  stroke: string | null,
): void => {
  ctx.beginPath();
  ctx.roundRect(x, y, w, h, 4);
  ctx.fillStyle = fill;
  const prev = ctx.globalAlpha;
  ctx.globalAlpha = prev * fillAlpha;
  ctx.fill();
  ctx.globalAlpha = prev;
  if (stroke) {
    ctx.strokeStyle = stroke;
    ctx.lineWidth = 1;
    ctx.stroke();
  }
};

const roadWidth = (count: number): number =>
  Math.min(8, Math.max(1.5, 1 + Math.floor(Math.log2(count))));

/** Folder keys whose imports carry little overview signal (test suites). */
const isTestCluster = (key: string): boolean =>
  /(^|\/)(tests?|__tests__|e2e|spec)($|\/)/.test(key);

// ── Rendering ───────────────────────────────────────────────────

const easeOut = (t: number): number => 1 - (1 - t) * (1 - t);

export const renderGraph = (state: AppState): void => {
  const { canvas, ctx, theme, dpr } = state;
  const gvs = getGVS(state);
  if (!gvs.initialized) return;

  const stageEl = canvas.parentElement;
  const w = stageEl ? stageEl.clientWidth : window.innerWidth;
  const h = stageEl ? stageEl.clientHeight : window.innerHeight;
  const pw = Math.round(w * dpr);
  const ph = Math.round(h * dpr);
  if (canvas.width !== pw || canvas.height !== ph) {
    canvas.style.width = `${w}px`;
    canvas.style.height = `${h}px`;
    canvas.width = pw;
    canvas.height = ph;
  }

  ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
  ctx.fillStyle = theme.bg;
  ctx.fillRect(0, 0, w, h);

  if (state.selected !== null && gvs.fileNodes[state.selected]) {
    renderGhost(state, gvs);
    renderEgoStage(state, gvs, w, h);
  } else {
    gvs.stageRects = [];
    gvs.lastRoot = null;
    renderOverview(state, gvs, w, h);
  }
};

// ── Overview ────────────────────────────────────────────────────

/** Opening choreography: layers sweep in left to right, then the roads. */
const REVEAL_LAYER_MS = 110;
const REVEAL_FADE_MS = 380;

const revealProgress = (
  gvs: GraphViewState,
  reduced: boolean,
): { t: number; cluster: (c: ClusterInfo) => number; roads: number; labels: number } => {
  if (gvs.revealAt === 0) gvs.revealAt = reduced ? -1 : performance.now();
  if (gvs.revealAt < 0) {
    return { t: 1, cluster: () => 1, roads: 1, labels: 1 };
  }
  const elapsed = performance.now() - gvs.revealAt;
  const maxLayer = gvs.clusters.reduce((max, c) => Math.max(max, c.isolated ? 0 : c.layer), 0);
  const total = (maxLayer + 1) * REVEAL_LAYER_MS + REVEAL_FADE_MS + 420;
  const t = Math.min(1, elapsed / total);
  const clusterAlpha = (c: ClusterInfo): number => {
    const start = (c.isolated ? maxLayer + 1 : c.layer) * REVEAL_LAYER_MS;
    return easeOut(Math.min(1, Math.max(0, (elapsed - start) / REVEAL_FADE_MS)));
  };
  const roadsStart = (maxLayer + 1) * REVEAL_LAYER_MS * 0.6;
  const roads = easeOut(Math.min(1, Math.max(0, (elapsed - roadsStart) / (REVEAL_FADE_MS + 200))));
  const labelsStart = roadsStart + 180;
  const labels = easeOut(Math.min(1, Math.max(0, (elapsed - labelsStart) / REVEAL_FADE_MS)));
  return { t, cluster: clusterAlpha, roads, labels };
};

const renderOverview = (state: AppState, gvs: GraphViewState, w: number, h: number): void => {
  const { ctx, theme, data } = state;
  const { transform, clusters, roads, fileNodes } = gvs;
  const files = data.files;
  const searching = state.search.trim() !== "";
  const kRel = transform.k / gvs.fitK;
  const reveal = revealProgress(gvs, state.reducedMotion);

  ctx.save();
  ctx.translate(transform.x, transform.y);
  ctx.scale(transform.k, transform.k);

  // Hull fills.
  for (const cluster of clusters) {
    if (cluster.isolated && !gvs.standaloneOpen) continue;
    if (cluster.hull.length < 3) continue;
    ctx.beginPath();
    hullPath(ctx, cluster.hull);
    ctx.fillStyle = theme.surface2;
    ctx.globalAlpha = 0.9 * reveal.cluster(cluster);
    ctx.fill();
    ctx.globalAlpha = 1;
  }

  // Intra-cluster edges (LOD).
  if (kRel >= LOD_INTRA) {
    ctx.strokeStyle = theme.textMuted;
    ctx.globalAlpha = 0.12;
    ctx.lineWidth = 1 / transform.k;
    ctx.beginPath();
    for (const [from, to] of data.edges) {
      const a = fileNodes[from];
      const b = fileNodes[to];
      if (!a || !b || a.cluster !== b.cluster) continue;
      if (a.x == null || a.y == null || b.x == null || b.y == null) continue;
      ctx.moveTo(a.x, a.y);
      ctx.lineTo(b.x, b.y);
    }
    ctx.stroke();
    ctx.globalAlpha = 1;
  }
  // Individual inter-cluster edges only at deep zoom.
  if (kRel >= LOD_INTER) {
    ctx.strokeStyle = theme.textMuted;
    ctx.globalAlpha = 0.1;
    ctx.lineWidth = 0.8 / transform.k;
    ctx.beginPath();
    for (const [from, to] of data.edges) {
      const a = fileNodes[from];
      const b = fileNodes[to];
      if (!a || !b || a.cluster === b.cluster) continue;
      if (a.x == null || a.y == null || b.x == null || b.y == null) continue;
      ctx.moveTo(a.x, a.y);
      ctx.lineTo(b.x, b.y);
    }
    ctx.stroke();
    ctx.globalAlpha = 1;
  }

  // Hull borders.
  for (const cluster of clusters) {
    if (cluster.isolated && !gvs.standaloneOpen) continue;
    if (cluster.hull.length < 3) continue;
    ctx.beginPath();
    hullPath(ctx, cluster.hull);
    const showTangle = cluster.tangle && state.lens === "boundaries";
    ctx.strokeStyle = showTangle ? theme.amber : theme.borderDefault;
    ctx.globalAlpha = (showTangle ? 0.7 : 0.6) * reveal.cluster(cluster);
    ctx.lineWidth = 1 / transform.k;
    ctx.stroke();
    ctx.globalAlpha = 1;
  }

  // Roads: tapered ribbons, wide at importer, narrow at imported.
  // At fit zoom the ribbons carry the whole story, so hold a minimum
  // on-screen width and lift the alpha; both relax as the user zooms in.
  const roadBoost = Math.min(1, Math.max(0, 1.6 - kRel));
  const minRoadW = 1.8 / transform.k;
  // High-traffic roads get promoted a step so the trunk routes survive
  // a projector; the threshold is the 75th percentile of bundle sizes.
  const roadCounts = roads.map((r) => r.count).sort((a, b) => a - b);
  const trunkFloor =
    roadCounts.length > 0 ? roadCounts[Math.floor(roadCounts.length * 0.75)] : Infinity;
  for (const road of roads) {
    const { p0, p1, p2, p3 } = roadGeometry(gvs, road);
    const wSrc = Math.max(minRoadW, roadWidth(road.count));
    ctx.beginPath();
    const thinRatio = road.count >= trunkFloor ? 0.15 : 0.22;
    taperedRibbon(ctx, p0, p1, p2, p3, wSrc, Math.max(0.5, wSrc * thinRatio));
    ctx.fillStyle = theme.textLow;
    // Test-to-source imports are the least interesting overview signal
    // but the biggest bundles; keep them recessive so source roads lead.
    const testDim = isTestCluster(clusters[road.src].key) ? 0.4 : 1;
    const trunk = road.count >= trunkFloor && testDim === 1 ? 0.22 : 0;
    ctx.globalAlpha = (0.3 + 0.18 * roadBoost + trunk) * testDim * reveal.roads;
    ctx.fill();
    ctx.globalAlpha = 1;

    // Severity overdraw parallel to the road (boundaries lens only;
    // the overview stays neutral until the user asks a question).
    if (state.lens === "boundaries" && (road.violations > 0 || (road.bidi && road.cycleEdges > 0))) {
      ctx.beginPath();
      ctx.moveTo(p0.x, p0.y + 4);
      ctx.bezierCurveTo(p1.x, p1.y + 4, p2.x, p2.y + 4, p3.x, p3.y + 4);
      if (road.violations > 0) {
        ctx.strokeStyle = theme.red;
        ctx.setLineDash([]);
      } else {
        ctx.strokeStyle = theme.amber;
        ctx.setLineDash([4 / transform.k, 3 / transform.k]);
      }
      ctx.lineWidth = 1.2 / transform.k;
      ctx.globalAlpha = 0.9;
      ctx.stroke();
      ctx.setLineDash([]);
      ctx.globalAlpha = 1;
    }
  }

  // Individual severity edges from mid zoom (boundaries lens only).
  if (state.lens === "boundaries" && kRel >= LOD_SEVERITY) drawSeverityEdges(state, gvs);

  // Hovered / selected road highlight: bright centerline, marching when hovered.
  const focusRoad = gvs.hoveredRoad ?? gvs.selectedRoad;
  if (focusRoad !== null && gvs.roads[focusRoad]) {
    const { p0, p1, p2, p3 } = roadGeometry(gvs, gvs.roads[focusRoad]);
    ctx.beginPath();
    ctx.moveTo(p0.x, p0.y);
    ctx.bezierCurveTo(p1.x, p1.y, p2.x, p2.y, p3.x, p3.y);
    ctx.strokeStyle = theme.bg;
    ctx.lineWidth = 5 / transform.k;
    ctx.globalAlpha = 0.8;
    ctx.stroke();
    ctx.strokeStyle = theme.blue;
    ctx.lineWidth = 2 / transform.k;
    ctx.globalAlpha = 1;
    if (gvs.hoveredRoad !== null && !state.reducedMotion) {
      ctx.setLineDash([8 / transform.k, 6 / transform.k]);
      ctx.lineDashOffset = -((performance.now() / 40) % 14) / transform.k;
    }
    ctx.stroke();
    ctx.setLineDash([]);
    ctx.lineDashOffset = 0;
    // Direction stamp: a filled dot marks the importer end, so the
    // taper's meaning is confirmable the moment a road is focused.
    ctx.beginPath();
    ctx.arc(p0.x, p0.y, 4 / transform.k, 0, Math.PI * 2);
    ctx.fillStyle = theme.blue;
    ctx.fill();
  }

  // Hover neighborhood.
  const hovered = state.graphHovered;
  let neighbors: Set<number> | null = null;
  if (hovered !== null) {
    neighbors = new Set([hovered]);
    for (const [from, to] of data.edges) {
      if (from !== hovered && to !== hovered) continue;
      neighbors.add(from);
      neighbors.add(to);
      const a = fileNodes[from];
      const b = fileNodes[to];
      if (!a || !b || a.x == null || a.y == null || b.x == null || b.y == null) continue;
      ctx.beginPath();
      ctx.moveTo(a.x, a.y);
      ctx.lineTo(b.x, b.y);
      ctx.strokeStyle = theme.bg;
      ctx.globalAlpha = 0.9;
      ctx.lineWidth = 3 / transform.k;
      ctx.stroke();
      ctx.strokeStyle = from === hovered ? theme.blue : theme.textLow;
      ctx.globalAlpha = 0.85;
      ctx.lineWidth = 1.2 / transform.k;
      ctx.stroke();
    }
    ctx.globalAlpha = 1;
  }

  // Nodes.
  for (const node of fileNodes) {
    if (!node || node.x == null || node.y == null) continue;
    if (clusters[node.cluster].isolated && !gvs.standaloneOpen) continue;
    const file = files[node.fileIndex];
    const color = lensColor(state.lens, theme, state.index, file);
    const recessive = color === theme.cellNeutral || color === theme.cellEntry;
    const matched = !searching || state.searchMatches.has(node.fileIndex);
    const isNeighbor = neighbors?.has(node.fileIndex) ?? false;
    const dimmed = neighbors !== null && !isNeighbor;

    let alpha = recessive ? 0.82 : 0.95;
    if (dimmed) alpha = 0.12;
    if (searching && !matched) alpha = Math.min(alpha, 0.1);
    if (isNeighbor) alpha = 1;
    alpha *= reveal.cluster(clusters[node.cluster]);
    if (alpha <= 0.01) continue;

    ctx.globalAlpha = alpha;
    ctx.fillStyle = color;
    ctx.beginPath();
    ctx.arc(
      node.x,
      node.y,
      node.radius * (state.graphHovered === node.fileIndex ? 1.3 : 1),
      0,
      Math.PI * 2,
    );
    ctx.fill();

    if (!dimmed) {
      if (state.lens === "deadcode" && file.status === "unused") {
        ctx.setLineDash([3 / transform.k, 3 / transform.k]);
        ctx.strokeStyle = theme.redText;
        ctx.lineWidth = 1.4 / transform.k;
        ctx.stroke();
        ctx.setLineDash([]);
      } else if (state.lens === "boundaries" && state.index.violationSources.has(node.fileIndex)) {
        ctx.setLineDash([3 / transform.k, 3 / transform.k]);
        ctx.strokeStyle = theme.red;
        ctx.lineWidth = 1.4 / transform.k;
        ctx.stroke();
        ctx.setLineDash([]);
      }
      if (searching && matched) {
        ctx.strokeStyle = theme.amberText;
        ctx.lineWidth = 1.4 / transform.k;
        ctx.stroke();
      }
      // Hub ring from mid zoom (a bare ring at fit zoom reads as an
      // artifact); the xN count joins once there is room.
      if (
        file.importer_count >= gvs.hubFloor &&
        (kRel >= 1.2 || state.graphHovered === node.fileIndex)
      ) {
        ctx.globalAlpha = Math.max(alpha, 0.85);
        ctx.strokeStyle = theme.textLow;
        ctx.lineWidth = 1 / transform.k;
        ctx.beginPath();
        ctx.arc(node.x, node.y, node.radius + 3 / transform.k, 0, Math.PI * 2);
        ctx.stroke();
        if (kRel >= 1.5 || state.graphHovered === node.fileIndex) {
          ctx.font = FONT_MICRO;
          ctx.textAlign = "left";
          ctx.textBaseline = "middle";
          ctx.fillStyle = theme.textLow;
          ctx.fillText(
            `×${formatCount(file.importer_count)}`,
            node.x + node.radius + 6 / transform.k,
            node.y,
          );
        }
      }
    }
    ctx.globalAlpha = 1;
  }

  // Deep-zoom file labels: name the important dots once there is room.
  if (kRel >= 2 && state.graphHovered === null) {
    drawZoomLabels(state, gvs, w, h);
  }

  // Neighbor labels on hover.
  if (neighbors !== null) {
    ctx.font = FONT_SMALL;
    ctx.textAlign = "center";
    ctx.textBaseline = "top";
    for (const idx of neighbors) {
      const node = fileNodes[idx];
      if (!node || node.x == null || node.y == null) continue;
      const name = basename(files[idx].path);
      const textW = ctx.measureText(name).width;
      ctx.fillStyle = theme.bg;
      ctx.globalAlpha = 0.85;
      ctx.fillRect(node.x - textW / 2 - 2, node.y + node.radius + 1, textW + 4, 12);
      ctx.globalAlpha = 1;
      ctx.fillStyle = idx === hovered ? theme.textHigh : theme.textLow;
      ctx.fillText(name, node.x, node.y + node.radius + 2);
    }
  }

  // Search pulse rings.
  if (gvs.pulseFile !== null) {
    const node = fileNodes[gvs.pulseFile];
    const age = performance.now() - gvs.pulseAt;
    if (node && node.x != null && node.y != null && age < 1200) {
      for (const phase of [0, 400]) {
        const t = (age - phase) / 800;
        if (t < 0 || t > 1) continue;
        ctx.beginPath();
        ctx.arc(node.x, node.y, node.radius + 4 + t * 26, 0, Math.PI * 2);
        ctx.strokeStyle = theme.blue;
        ctx.globalAlpha = 0.8 * (1 - t);
        ctx.lineWidth = 2 / transform.k;
        ctx.stroke();
      }
      ctx.globalAlpha = 1;
    } else {
      gvs.pulseFile = null;
    }
  }

  ctx.restore();

  // Labels join once the roads have flowed in (their internal alpha
  // handling would fight a global fade).
  if (reveal.labels > 0.35) {
    drawRoadLabels(state, gvs);
    drawClusterLabels(state, gvs);
  }
  drawAxisMarkers(state, gvs, w, h);
  drawCanvasLegend(state, w, h);
  drawPathTrace(state, gvs, w, h);

  drawMinimap(state, gvs, w, h);

  // Transient notice (fades after 1.8s).
  if (gvs.notice !== "") {
    const age = performance.now() - gvs.noticeAt;
    if (age < 1800) {
      ctx.font = FONT_SMALL;
      ctx.textAlign = "center";
      ctx.textBaseline = "top";
      ctx.fillStyle = theme.amberText;
      ctx.globalAlpha = age > 1400 ? 1 - (age - 1400) / 400 : 1;
      ctx.fillText(gvs.notice, w / 2, 28);
      ctx.globalAlpha = 1;
      cancelAnimationFrame(gvs.raf);
      gvs.raf = requestAnimationFrame(() => {
        if (state.view === "graph") renderGraph(state);
      });
    } else {
      gvs.notice = "";
    }
  }

  drawIntroCaptions(state, gvs, w);

  // Motion frames while something animates.
  const animating =
    (gvs.hoveredRoad !== null && !state.reducedMotion) ||
    gvs.pulseFile !== null ||
    reveal.t < 1 ||
    gvs.showIntro;
  if (animating) {
    cancelAnimationFrame(gvs.raf);
    gvs.raf = requestAnimationFrame(() => {
      if (state.view === "graph") renderGraph(state);
    });
  }
};

/** Greedy screen-space labels for the highest-degree files in view. */
const drawZoomLabels = (state: AppState, gvs: GraphViewState, w: number, h: number): void => {
  const { ctx, theme, data } = state;
  const { transform } = gvs;
  const candidates: Array<{ node: FileNode; degree: number }> = [];
  for (const node of gvs.fileNodes) {
    if (!node || node.x == null || node.y == null) continue;
    const sx = node.x * transform.k + transform.x;
    const sy = node.y * transform.k + transform.y;
    if (sx < -20 || sx > w + 20 || sy < -20 || sy > h + 20) continue;
    const file = data.files[node.fileIndex];
    candidates.push({ node, degree: file.importer_count + file.import_count });
  }
  candidates.sort((a, b) => b.degree - a.degree);

  ctx.font = FONT_SMALL;
  ctx.textAlign = "center";
  ctx.textBaseline = "top";
  const placed: Array<{ x: number; y: number; w: number; h: number }> = [];
  let drawn = 0;
  for (const { node } of candidates) {
    if (drawn >= 40) break;
    if (node.x == null || node.y == null) continue;
    const name = basename(data.files[node.fileIndex].path);
    const textW = ctx.measureText(name).width;
    const x = node.x;
    const y = node.y + node.radius + 2 / transform.k;
    // Occupancy check in screen space.
    const sx = x * transform.k + transform.x;
    const sy = y * transform.k + transform.y;
    const rect = { x: sx - textW / 2 - 2, y: sy, w: textW + 4, h: 13 };
    const overlaps = placed.some(
      (r) => rect.x < r.x + r.w && rect.x + rect.w > r.x && rect.y < r.y + r.h && rect.y + rect.h > r.y,
    );
    if (overlaps) continue;
    placed.push(rect);
    drawn++;
    // Draw in world space (crisper under the active transform).
    const worldFont = 10 / transform.k;
    ctx.font = `${worldFont}px "Martian Mono", "JetBrains Mono", ui-monospace, Menlo, monospace`;
    const wTextW = ctx.measureText(name).width;
    ctx.fillStyle = theme.bg;
    ctx.globalAlpha = 0.82;
    ctx.fillRect(x - wTextW / 2 - 2 / transform.k, y, wTextW + 4 / transform.k, 12 / transform.k);
    ctx.globalAlpha = 0.9;
    ctx.fillStyle = theme.textLow;
    ctx.fillText(name, x, y + 1 / transform.k);
    ctx.globalAlpha = 1;
    ctx.font = FONT_SMALL;
  }
};

/** Three staged captions that teach the map during the opening reveal. */
const drawIntroCaptions = (state: AppState, gvs: GraphViewState, w: number): void => {
  if (!gvs.showIntro || gvs.revealAt <= 0) return;
  const { ctx, theme } = state;
  const elapsed = performance.now() - gvs.revealAt;
  // Three beats: the nouns, the lines, then the verbs (what to do next).
  const captions: Array<[number, number, string]> = [
    [0, 2600, "Every dot is a file · the shapes are folders"],
    [2600, 5200, "Lines are imports · the thick end is the importer"],
    [5200, 8600, "Click any dot for its story · keys 1-5 switch lenses"],
  ];
  const total = captions[captions.length - 1][1];
  if (elapsed >= total) {
    gvs.showIntro = false;
    markIntroSeen();
    return;
  }
  for (const [from, to, text] of captions) {
    if (elapsed < from || elapsed >= to) continue;
    const local = (elapsed - from) / (to - from);
    const alpha = local < 0.12 ? local / 0.12 : local > 0.85 ? (1 - local) / 0.15 : 1;
    ctx.font = FONT_CARD;
    ctx.textAlign = "center";
    ctx.textBaseline = "middle";
    const textW = ctx.measureText(text).width;
    ctx.globalAlpha = Math.max(0, alpha);
    // Backed chip so the caption reads over cluster labels behind it.
    chipRect(ctx, w / 2 - textW / 2 - 16, 12, textW + 32, 32, theme.bg, 1, theme.borderSubtle);
    ctx.fillStyle = theme.textHigh;
    ctx.fillText(text, w / 2, 28.5);
    ctx.globalAlpha = 1;
  }
};

/** Any real interaction ends the intro early. */
export const dismissIntro = (state: AppState): void => {
  const gvs = getGVS(state);
  if (!gvs.showIntro) return;
  gvs.showIntro = false;
  markIntroSeen();
};

// ── Minimap ─────────────────────────────────────────────────────

const MINIMAP_W = 172;
const MINIMAP_H = 112;
const MINIMAP_MARGIN = 14;

interface MinimapFrame {
  x: number;
  y: number;
  w: number;
  h: number;
  scale: number;
  worldX: number;
  worldY: number;
}

const minimapFrame = (
  state: AppState,
  gvs: GraphViewState,
  w: number,
  h: number,
): MinimapFrame | null => {
  if (gvs.clusters.length < 2) return null;
  // Keep clear of the detail panel when a road drill-down is open.
  const panelW = state.selectedRoad !== null ? Math.min(380, w * 0.9) : 0;
  let minX = Infinity;
  let minY = Infinity;
  let maxX = -Infinity;
  let maxY = -Infinity;
  for (const c of gvs.clusters) {
    minX = Math.min(minX, c.cx - c.r);
    minY = Math.min(minY, c.cy - c.r);
    maxX = Math.max(maxX, c.cx + c.r);
    maxY = Math.max(maxY, c.cy + c.r);
  }
  const worldW = Math.max(1, maxX - minX);
  const worldH = Math.max(1, maxY - minY);
  const scale = Math.min((MINIMAP_W - 12) / worldW, (MINIMAP_H - 12) / worldH);
  return {
    x: w - panelW - MINIMAP_W - MINIMAP_MARGIN,
    y: h - MINIMAP_H - MINIMAP_MARGIN - 24,
    w: MINIMAP_W,
    h: MINIMAP_H,
    scale,
    worldX: minX - (MINIMAP_W / scale - worldW) / 2,
    worldY: minY - (MINIMAP_H / scale - worldH) / 2,
  };
};

const drawMinimap = (state: AppState, gvs: GraphViewState, w: number, h: number): void => {
  const { ctx, theme } = state;
  // Only earns pixels once the camera left the fit view.
  if (gvs.transform.k / gvs.fitK < 1.08) return;
  const frame = minimapFrame(state, gvs, w, h);
  if (!frame) return;

  ctx.fillStyle = theme.surface1;
  ctx.globalAlpha = 0.92;
  ctx.fillRect(frame.x, frame.y, frame.w, frame.h);
  ctx.globalAlpha = 1;
  ctx.strokeStyle = theme.borderDefault;
  ctx.lineWidth = 1;
  ctx.strokeRect(frame.x + 0.5, frame.y + 0.5, frame.w - 1, frame.h - 1);

  const toMini = (p: Pt): Pt => ({
    x: frame.x + (p.x - frame.worldX) * frame.scale,
    y: frame.y + (p.y - frame.worldY) * frame.scale,
  });

  // Cluster footprints, tangles in amber.
  for (const cluster of gvs.clusters) {
    const c = toMini({ x: cluster.cx, y: cluster.cy });
    const r = Math.max(1.5, cluster.r * frame.scale);
    ctx.beginPath();
    ctx.arc(c.x, c.y, r, 0, Math.PI * 2);
    ctx.fillStyle = cluster.tangle ? theme.amber : theme.borderStrong;
    ctx.globalAlpha = cluster.tangle ? 0.55 : 0.4;
    ctx.fill();
  }
  ctx.globalAlpha = 1;

  // Viewport rectangle (inverse of the active transform).
  const { transform } = gvs;
  const topLeft = toMini({ x: -transform.x / transform.k, y: -transform.y / transform.k });
  const bottomRight = toMini({
    x: (w - transform.x) / transform.k,
    y: (h - transform.y) / transform.k,
  });
  ctx.strokeStyle = theme.blue;
  ctx.lineWidth = 1;
  ctx.strokeRect(
    Math.max(frame.x, topLeft.x) + 0.5,
    Math.max(frame.y, topLeft.y) + 0.5,
    Math.min(frame.w, bottomRight.x - topLeft.x) - 1,
    Math.min(frame.h, bottomRight.y - topLeft.y) - 1,
  );
};

/** True when the point sits inside the minimap (graph overview only). */
export const minimapHit = (state: AppState, x: number, y: number): boolean => {
  const gvs = getGVS(state);
  if (!gvs.initialized || state.selected !== null) return false;
  if (gvs.transform.k / gvs.fitK < 1.08) return false;
  const stageEl = state.canvas.parentElement;
  const w = stageEl ? stageEl.clientWidth : window.innerWidth;
  const h = stageEl ? stageEl.clientHeight : window.innerHeight;
  const frame = minimapFrame(state, gvs, w, h);
  if (!frame) return false;
  return x >= frame.x && x <= frame.x + frame.w && y >= frame.y && y <= frame.y + frame.h;
};

/** Center the camera on the clicked minimap position. */
export const minimapPan = (state: AppState, x: number, y: number): void => {
  const gvs = getGVS(state);
  if (!gvs.initialized || !gvs.zoomBehavior) return;
  const stageEl = state.canvas.parentElement;
  const w = stageEl ? stageEl.clientWidth : window.innerWidth;
  const h = stageEl ? stageEl.clientHeight : window.innerHeight;
  const frame = minimapFrame(state, gvs, w, h);
  if (!frame) return;
  const worldX = frame.worldX + (x - frame.x) / frame.scale;
  const worldY = frame.worldY + (y - frame.y) / frame.scale;
  const k = gvs.transform.k;
  select(state.canvas).call(
    gvs.zoomBehavior.transform,
    zoomIdentity.translate(w / 2 - worldX * k, h / 2 - worldY * k).scale(k),
  );
};

/** Path-trace overlay: dim the map, draw the dependency chain on top. */
const drawPathTrace = (state: AppState, gvs: GraphViewState, w: number, h: number): void => {
  const { ctx, theme, data } = state;

  if (gvs.pathFrom !== null && gvs.path === null) {
    const node = gvs.fileNodes[gvs.pathFrom];
    if (node && node.x != null && node.y != null) {
      const s = worldToScreen(gvs, { x: node.x, y: node.y });
      ctx.beginPath();
      ctx.arc(s.x, s.y, 10, 0, Math.PI * 2);
      ctx.strokeStyle = theme.blue;
      ctx.lineWidth = 2;
      ctx.setLineDash([4, 3]);
      ctx.stroke();
      ctx.setLineDash([]);
      ctx.font = FONT_MICRO;
      ctx.textAlign = "center";
      ctx.textBaseline = "top";
      ctx.fillStyle = theme.blueText;
      ctx.fillText("trace from here · shift-click a target", s.x, s.y + 16);
    }
    return;
  }

  const path = gvs.path;
  if (!path || path.length < 2) return;

  // Dim everything under the trace.
  ctx.fillStyle = theme.bg;
  ctx.globalAlpha = 0.62;
  ctx.fillRect(0, 0, w, h);
  ctx.globalAlpha = 1;

  const pts = path
    .map((idx) => gvs.fileNodes[idx])
    .filter((n) => n && n.x != null && n.y != null)
    .map((n) => worldToScreen(gvs, { x: n.x ?? 0, y: n.y ?? 0 }));
  if (pts.length < 2) return;

  ctx.beginPath();
  ctx.moveTo(pts[0].x, pts[0].y);
  for (let i = 1; i < pts.length; i++) ctx.lineTo(pts[i].x, pts[i].y);
  ctx.strokeStyle = theme.bg;
  ctx.lineWidth = 6;
  ctx.stroke();
  ctx.strokeStyle = theme.blue;
  ctx.lineWidth = 2;
  ctx.stroke();

  ctx.font = FONT_SMALL;
  ctx.textAlign = "center";
  ctx.textBaseline = "bottom";
  path.forEach((idx, i) => {
    const p = pts[i];
    if (!p) return;
    ctx.beginPath();
    ctx.arc(p.x, p.y, 5, 0, Math.PI * 2);
    ctx.fillStyle = i === 0 || i === path.length - 1 ? theme.blue : theme.textHigh;
    ctx.fill();
    const name = basename(data.files[idx].path);
    const textW = ctx.measureText(name).width;
    ctx.fillStyle = theme.bg;
    ctx.globalAlpha = 0.9;
    ctx.fillRect(p.x - textW / 2 - 3, p.y - 24, textW + 6, 14);
    ctx.globalAlpha = 1;
    ctx.fillStyle = theme.textHigh;
    ctx.fillText(name, p.x, p.y - 11);
  });

  ctx.font = FONT_MICRO;
  ctx.textAlign = "left";
  ctx.textBaseline = "top";
  ctx.fillStyle = theme.blueText;
  ctx.fillText(
    `dependency trace · ${path.length - 1} hop${path.length === 2 ? "" : "s"} · esc to clear`,
    14,
    28,
  );
};

const hullPath = (ctx: CanvasRenderingContext2D, hull: Pt[]): void => {
  ctx.moveTo(hull[0].x, hull[0].y);
  for (let i = 1; i < hull.length; i++) ctx.lineTo(hull[i].x, hull[i].y);
  ctx.closePath();
};

const worldToScreen = (gvs: GraphViewState, p: Pt): Pt => ({
  x: p.x * gvs.transform.k + gvs.transform.x,
  y: p.y * gvs.transform.k + gvs.transform.y,
});

const drawRoadLabels = (state: AppState, gvs: GraphViewState): void => {
  const { ctx, theme } = state;
  ctx.font = FONT_MICRO;
  ctx.textAlign = "center";
  ctx.textBaseline = "middle";
  const kRel = gvs.transform.k / gvs.fitK;
  for (let ri = 0; ri < gvs.roads.length; ri++) {
    const road = gvs.roads[ri];
    const focused = gvs.hoveredRoad === ri || gvs.selectedRoad === ri;
    // Quiet by default: numbers appear on zoom or on intent (hover/click).
    if (!focused && kRel < 1.5) continue;
    if (road.count < 2 && !focused) continue;
    const { p0, p1, p2, p3 } = roadGeometry(gvs, road);
    const mid = worldToScreen(gvs, cubicPoint(p0, p1, p2, p3, 0.5));
    const label = formatCount(road.count);
    const textW = ctx.measureText(label).width;
    ctx.fillStyle = theme.bg;
    ctx.globalAlpha = 0.92;
    ctx.fillRect(mid.x - textW / 2 - 3, mid.y - 7, textW + 6, 14);
    ctx.globalAlpha = 1;
    ctx.strokeStyle = theme.borderSubtle;
    ctx.lineWidth = 1;
    ctx.strokeRect(mid.x - textW / 2 - 3.5, mid.y - 7.5, textW + 7, 15);
    if (state.lens === "boundaries" && road.violations > 0) ctx.fillStyle = theme.redText;
    else if (state.lens === "boundaries" && road.bidi && road.cycleEdges > 0) ctx.fillStyle = theme.amberText;
    else ctx.fillStyle = theme.textLow;
    ctx.fillText(label, mid.x, mid.y + 0.5);
  }
};

const drawClusterLabels = (state: AppState, gvs: GraphViewState): void => {
  const { ctx, theme } = state;
  ctx.font = FONT_CHIP;
  ctx.textAlign = "left";
  ctx.textBaseline = "middle";
  const placed: Array<{ x: number; y: number; w: number; h: number }> = [];
  // Bigger clusters claim their spot first; smaller ones move below on overlap.
  const ordered = [...gvs.clusters].sort(
    (a, b) => b.indices.length - a.indices.length || (a.key < b.key ? -1 : 1),
  );
  const kRel = gvs.transform.k / gvs.fitK;
  for (const cluster of ordered) {
    if (cluster.isolated && !getGVS(state).standaloneOpen) continue;
    // Small multi-file clusters wait for mid zoom (their chips only add
    // collisions at fit); singletons keep their quiet borderless label
    // so no connected dot floats unexplained.
    if (cluster.indices.length >= 2 && cluster.indices.length < 6 && kRel < 1.5) continue;
    let topLeft = cluster.hull[0] ?? { x: cluster.cx, y: cluster.cy };
    for (const p of cluster.hull) {
      if (p.y < topLeft.y || (p.y === topLeft.y && p.x < topLeft.x)) topLeft = p;
    }
    const s = worldToScreen(gvs, topLeft);
    // Single-file clusters: just the filename, borderless dim text. The
    // full path lives in the tooltip; quiet labels collide far less.
    const single = cluster.indices.length === 1;
    const raw = single
      ? basename(state.data.files[cluster.indices[0]].path)
      : cluster.key;
    const label = middleTruncate(ctx, raw.toUpperCase(), 210);
    const sub = single ? "" : `${formatCount(cluster.indices.length)} files`;
    const labelW = ctx.measureText(label).width;
    const subW = sub ? ctx.measureText(sub).width : -8;
    const boxW = labelW + subW + 17;
    // Clamp inside the viewport so edge clusters keep readable chips.
    const x = Math.min(Math.max(6, s.x - 4), state.canvas.clientWidth - boxW - 8);
    let y = s.y - 12;
    for (let tries = 0; tries < 6; tries++) {
      const overlaps = placed.some(
        (r) => x < r.x + r.w && x + boxW > r.x && y - 9 < r.y + r.h && y + 9 > r.y,
      );
      if (!overlaps) break;
      y += 19;
    }
    placed.push({ x: x - 4, y: y - 10, w: boxW + 3, h: 20 });
    chipRect(
      ctx,
      x - 4,
      y - 10,
      labelW + subW + 18,
      20,
      theme.bg,
      single ? 0.75 : 0.92,
      single ? null : cluster.tangle && state.lens === "boundaries" ? theme.amber : theme.borderSubtle,
    );
    ctx.fillStyle = cluster.isolated || single ? theme.textMuted : theme.textLow;
    ctx.fillText(label, x + 2, y + 0.5);
    if (sub) {
      ctx.fillStyle = theme.textMuted;
      ctx.fillText(sub, x + labelW + 8, y + 0.5);
    }
  }

  // Standalone toggle: a fixed chip docked above the canvas legend so it
  // never floats orphaned in world space. When open, a caption sits by
  // the revealed strip itself.
  const isolated = gvs.clusters.filter((c) => c.isolated);
  gvs.standaloneChip = null;
  if (isolated.length > 0) {
    const h = state.canvas.clientHeight;
    const fileCount = isolated.reduce((sum, c) => sum + c.indices.length, 0);
    ctx.font = FONT_MICRO;
    ctx.textAlign = "left";
    ctx.textBaseline = "middle";
    const label = gvs.standaloneOpen
      ? "hide standalone files"
      : `standalone · ${formatCount(fileCount)} files · nothing imports them`;
    const textW = ctx.measureText(label).width;
    const cx0 = 12;
    const cy0 = h - 56;
    chipRect(ctx, cx0, cy0, textW + 16, 22, theme.bg, 0.9, theme.borderSubtle);
    ctx.fillStyle = theme.textMuted;
    ctx.fillText(label, cx0 + 8, cy0 + 11.5);
    gvs.standaloneChip = { x: cx0, y: cy0, w: textW + 16, h: 22 };
    if (gvs.standaloneOpen) {
      const minX = Math.min(...isolated.map((c) => c.cx - c.r));
      const minY = Math.min(...isolated.map((c) => c.cy - c.r));
      const s = worldToScreen(gvs, { x: minX, y: minY });
      ctx.fillStyle = theme.textMuted;
      ctx.globalAlpha = 0.7;
      ctx.fillText("STANDALONE · configs & CI · nothing imports them", s.x, s.y - 26);
      ctx.globalAlpha = 1;
    }
  }
};

/** Screen-space axis endpoints: the one annotation that explains the x layout. */
const drawAxisMarkers = (state: AppState, gvs: GraphViewState, w: number, h: number): void => {
  // The entry-to-shared axis only describes the folder rank layout;
  // import communities carry no left-right meaning.
  if (gvs.clusterMode === "imports") return;
  const kRel = gvs.transform.k / gvs.fitK;
  if (kRel > 1.3 || state.selected !== null) return;
  const { ctx, theme } = state;
  ctx.font = FONT_SMALL;
  ctx.textBaseline = "middle";
  ctx.fillStyle = theme.textLow;
  ctx.globalAlpha = 0.85;
  ctx.textAlign = "left";
  ctx.fillText("ENTRY CODE", 14, h / 2);
  ctx.textAlign = "right";
  ctx.fillText("SHARED FOUNDATIONS", w - 14, h / 2);
  ctx.globalAlpha = 1;
};

const drawCanvasLegend = (state: AppState, w: number, h: number): void => {
  const { ctx, theme } = state;
  const text = legendText(state.lens, state.data, "graph");
  if (text === "") return;
  ctx.font = FONT_LEGEND;
  ctx.textAlign = "left";
  ctx.textBaseline = "middle";
  const textW = ctx.measureText(text).width;
  ctx.fillStyle = theme.bg;
  ctx.globalAlpha = 0.85;
  ctx.fillRect(10, h - 27, textW + 12, 19);
  ctx.globalAlpha = 0.9;
  ctx.fillStyle = theme.textLow;
  ctx.fillText(text, 16, h - 17.5);
  ctx.globalAlpha = 1;
  void w;
};

const drawSeverityEdges = (state: AppState, gvs: GraphViewState): void => {
  const { ctx, theme, data } = state;
  const n = data.files.length;
  const k = gvs.transform.k;
  for (const [from, to] of data.edges) {
    const packed = from * n + to;
    const isViolation = state.index.violationEdges.has(packed);
    const isCycle = state.index.cycleEdges.has(packed);
    if (!isViolation && !isCycle) continue;
    const a = gvs.fileNodes[from];
    const b = gvs.fileNodes[to];
    if (!a || !b || a.x == null || a.y == null || b.x == null || b.y == null) continue;
    ctx.beginPath();
    ctx.moveTo(a.x, a.y);
    ctx.lineTo(b.x, b.y);
    ctx.strokeStyle = theme.bg;
    ctx.lineWidth = 3 / k;
    ctx.globalAlpha = 0.9;
    ctx.setLineDash([]);
    ctx.stroke();
    ctx.strokeStyle = isViolation ? theme.red : theme.amber;
    ctx.lineWidth = 1.4 / k;
    if (isCycle && !isViolation) ctx.setLineDash([4 / k, 3 / k]);
    ctx.stroke();
    ctx.setLineDash([]);
    ctx.globalAlpha = 1;
  }
};

// ── Ghost layer (ego mode background) ───────────────────────────

const renderGhost = (state: AppState, gvs: GraphViewState): void => {
  const { ctx, theme } = state;
  const { transform } = gvs;
  ctx.save();
  ctx.translate(transform.x, transform.y);
  ctx.scale(transform.k, transform.k);
  for (const cluster of gvs.clusters) {
    if (cluster.hull.length < 3) continue;
    ctx.beginPath();
    hullPath(ctx, cluster.hull);
    ctx.strokeStyle = theme.borderSubtle;
    ctx.globalAlpha = 0.25;
    ctx.lineWidth = 1 / transform.k;
    ctx.stroke();
  }
  ctx.globalAlpha = 0.12;
  for (const node of gvs.fileNodes) {
    if (!node || node.x == null || node.y == null) continue;
    ctx.fillStyle = lensColor(state.lens, theme, state.index, state.data.files[node.fileIndex]);
    ctx.beginPath();
    ctx.arc(node.x, node.y, node.radius, 0, Math.PI * 2);
    ctx.fill();
  }
  ctx.globalAlpha = 1;
  ctx.restore();
};

// ── Ego stage ───────────────────────────────────────────────────

interface StageRow {
  kind: "file" | "group" | "header" | "more";
  fileIndex?: number;
  groupKey?: string;
  label: string;
  dim?: string;
  count?: number;
  violation?: boolean;
  cycle?: boolean;
}

const buildColumn = (
  state: AppState,
  gvs: GraphViewState,
  rootIdx: number,
  indices: number[],
  side: "left" | "right",
  maxRows: number,
): StageRow[] => {
  const files = state.data.files;
  const n = files.length;
  const isViolation = (other: number): boolean =>
    side === "left"
      ? state.index.violationEdges.has(other * n + rootIdx)
      : state.index.violationEdges.has(rootIdx * n + other);
  const isCycle = (other: number): boolean =>
    state.index.cycleEdges.has(rootIdx * n + other) ||
    state.index.cycleEdges.has(other * n + rootIdx);

  const groups = new Map<string, number[]>();
  for (const idx of indices) {
    const top = files[idx].path.split("/")[0];
    if (!groups.has(top)) groups.set(top, []);
    groups.get(top)?.push(idx);
  }
  const layerOf = (dir: string): number => {
    const cluster = gvs.clusters.find((c) => c.key === dir || c.key.startsWith(`${dir}/`));
    return cluster ? cluster.layer * 1000 + cluster.order : 999999;
  };
  const groupKeys = [...groups.keys()].sort((a, b) => layerOf(a) - layerOf(b) || (a < b ? -1 : 1));

  const fileRow = (idx: number): StageRow => ({
    kind: "file",
    fileIndex: idx,
    label: basename(files[idx].path),
    dim: dirname(files[idx].path),
    violation: isViolation(idx),
    cycle: isCycle(idx),
  });

  const sortIndices = (list: number[]): number[] =>
    [...list].sort((a, b) => {
      const sevA = (isViolation(a) ? 2 : 0) + (isCycle(a) ? 1 : 0);
      const sevB = (isViolation(b) ? 2 : 0) + (isCycle(b) ? 1 : 0);
      if (sevA !== sevB) return sevB - sevA;
      return files[a].path < files[b].path ? -1 : 1;
    });

  // Decide collapsed vs expanded BEFORE layout.
  const totalExpanded = indices.length + (groupKeys.length > 1 ? groupKeys.length : 0);
  const collapse = totalExpanded > maxRows && groupKeys.length > 1;

  const rows: StageRow[] = [];
  for (const key of groupKeys) {
    const members = sortIndices(groups.get(key) ?? []);
    const expandKey = `${side}:${key}`;
    const expanded = !collapse || gvs.egoExpanded.has(expandKey);
    if (groupKeys.length > 1) {
      if (collapse && !expanded) {
        rows.push({
          kind: "group",
          groupKey: expandKey,
          label: `${key}/`,
          count: members.length,
          violation: members.some(isViolation),
          cycle: members.some(isCycle),
        });
        continue;
      }
      rows.push({ kind: "header", label: `${key}/`, groupKey: expandKey });
    }
    for (const idx of members) rows.push(fileRow(idx));
  }
  if (rows.length > maxRows) {
    const kept = rows.slice(0, maxRows - 1);
    const hidden = rows.length - (maxRows - 1);
    kept.push({ kind: "more", label: `… ${hidden} more (see panel)` });
    return kept;
  }
  return rows;
};

const renderEgoStage = (state: AppState, gvs: GraphViewState, w: number, h: number): void => {
  const { ctx, theme, data } = state;
  const rootIdx = state.selected;
  if (rootIdx === null) return;
  const rootFile = data.files[rootIdx];
  const rootNode = gvs.fileNodes[rootIdx];

  if (gvs.lastRoot !== rootIdx) {
    gvs.stageEnterAt = state.reducedMotion ? 0 : performance.now();
    if (gvs.crumbs[gvs.crumbs.length - 1] !== rootIdx) {
      gvs.crumbs.push(rootIdx);
      if (gvs.crumbs.length > 12) gvs.crumbs.shift();
    }
    gvs.lastRoot = rootIdx;
  }
  const t = state.reducedMotion
    ? 1
    : Math.min(1, (performance.now() - gvs.stageEnterAt) / STAGE_ENTER_MS);
  const ease = easeOut(t);

  // Stage area: keep clear of the detail panel (380px when open).
  const panelW = Math.min(380, w * 0.9);
  const stageW = Math.max(420, w - panelW);
  const cx = stageW / 2;
  const cy = h / 2;

  gvs.stageRects = [];

  const importers = state.index.importersOf[rootIdx];
  const imports = state.index.importsOf[rootIdx];
  const availH = h - 170;
  const maxRows = Math.max(6, Math.floor(availH / 19));
  const leftRows = buildColumn(state, gvs, rootIdx, importers, "left", maxRows);
  const rightRows = buildColumn(state, gvs, rootIdx, imports, "right", maxRows);
  const colOffset = Math.min(Math.max(0.3 * stageW, 230), 430);
  const leftX = cx - colOffset;
  const rightX = cx + colOffset;

  ctx.save();
  ctx.globalAlpha = ease;

  // Column headers, anchored just above each column's own rows (or the
  // card when a side is empty) instead of floating at the viewport top.
  const headerY = (rows: StageRow[]): number => {
    if (rows.length === 0) return cy - 33 - 18;
    const rowH = Math.min(24, Math.max(18, availH / rows.length));
    return cy - (rows.length * rowH) / 2 - 18;
  };
  ctx.font = FONT_MICRO;
  ctx.textBaseline = "middle";
  ctx.fillStyle = theme.textMuted;
  ctx.textAlign = "right";
  ctx.fillText(`◂ IMPORTED BY ${formatCount(importers.length)}`, leftX, headerY(leftRows));
  ctx.textAlign = "left";
  ctx.fillText(`IMPORTS ${formatCount(imports.length)} ▸`, rightX, headerY(rightRows));
  if (importers.length === 0) {
    ctx.textAlign = "right";
    ctx.fillText("nothing imports this file", leftX, cy);
  }
  if (imports.length === 0) {
    ctx.textAlign = "left";
    ctx.fillText("no imports", rightX, cy);
  }

  drawStageColumn(state, gvs, leftRows, "left", leftX, cy, availH, cx, ease, stageW);
  drawStageColumn(state, gvs, rightRows, "right", rightX, cy, availH, cx, ease, stageW);

  // Escape hatch at the point of attention, not only in the statusbar.
  ctx.font = FONT_MICRO;
  ctx.textAlign = "left";
  const backLabel = "◂ back to map · esc";
  const backW = ctx.measureText(backLabel).width;
  ctx.globalAlpha = 0.9 * ease;
  chipRect(ctx, 12, 12, backW + 20, 24, theme.bg, 1, theme.borderSubtle);
  ctx.globalAlpha = ease;
  ctx.fillStyle = theme.textLow;
  ctx.fillText(backLabel, 22, 24.5);
  gvs.egoBackChip = { x: 12, y: 12, w: backW + 20, h: 24 };

  // Center card.
  const cardW = 250;
  const cardH = 66;
  ctx.fillStyle = theme.surface1;
  ctx.fillRect(cx - cardW / 2, cy - cardH / 2, cardW, cardH);
  ctx.strokeStyle = theme.blue;
  ctx.lineWidth = 1;
  ctx.strokeRect(cx - cardW / 2 + 0.5, cy - cardH / 2 + 0.5, cardW - 1, cardH - 1);
  ctx.textAlign = "center";
  ctx.font = FONT_MICRO;
  ctx.fillStyle = theme.textMuted;
  const dir = dirname(rootFile.path);
  ctx.fillText(middleTruncate(ctx, dir ? `${dir}/` : "", cardW - 20), cx, cy - 18);
  ctx.font = FONT_CARD;
  ctx.fillStyle = theme.textHigh;
  ctx.fillText(middleTruncate(ctx, basename(rootFile.path), cardW - 20), cx, cy + 1);
  ctx.font = FONT_MICRO;
  ctx.fillStyle = theme.textLow;
  ctx.fillText(
    `imported by ${formatCount(importers.length)} · imports ${formatCount(imports.length)}`,
    cx,
    cy + 19,
  );

  // Crosshair at the true map position.
  if (rootNode && rootNode.x != null && rootNode.y != null) {
    const s = worldToScreen(gvs, { x: rootNode.x, y: rootNode.y });
    ctx.strokeStyle = theme.blue;
    ctx.globalAlpha = 0.5 * ease;
    ctx.lineWidth = 1;
    ctx.beginPath();
    ctx.moveTo(s.x - 6, s.y);
    ctx.lineTo(s.x + 6, s.y);
    ctx.moveTo(s.x, s.y - 6);
    ctx.lineTo(s.x, s.y + 6);
    ctx.stroke();
    ctx.globalAlpha = ease;
  }

  drawCrumbs(state, gvs, stageW);

  ctx.restore();

  const rowMarching =
    !state.reducedMotion &&
    state.graphHovered !== null &&
    gvs.stageRects.some((r) => r.kind === "file" && r.fileIndex === state.graphHovered);
  if (t < 1 || rowMarching) {
    cancelAnimationFrame(gvs.raf);
    gvs.raf = requestAnimationFrame(() => {
      if (state.view === "graph") renderGraph(state);
    });
  }
};

const drawStageColumn = (
  state: AppState,
  gvs: GraphViewState,
  rows: StageRow[],
  side: "left" | "right",
  colX: number,
  cy: number,
  availH: number,
  centerX: number,
  ease: number,
  stageW: number,
): void => {
  const { ctx, theme } = state;
  if (rows.length === 0) return;
  const rowH = Math.min(24, Math.max(18, availH / rows.length));
  const totalH = rows.length * rowH;
  let y = cy - totalH / 2 + rowH / 2;
  const dirSign = side === "left" ? -1 : 1;
  const slide = 14 * (1 - ease) * dirSign;
  const cardEdgeX = centerX + dirSign * 128;

  for (const row of rows) {
    const rowY = y;
    y += rowH;
    const textX = colX + dirSign * 14 + slide;

    if (row.kind === "file" || row.kind === "group") {
      const endX = colX - dirSign * 6 + slide;
      const hoveredRow =
        row.kind === "file" &&
        row.fileIndex !== undefined &&
        state.graphHovered === row.fileIndex;
      ctx.beginPath();
      ctx.moveTo(cardEdgeX, cy);
      const dx = endX - cardEdgeX;
      ctx.bezierCurveTo(cardEdgeX + dx * 0.45, cy, cardEdgeX + dx * 0.55, rowY, endX, rowY);
      if (row.violation) {
        ctx.strokeStyle = theme.red;
        ctx.lineWidth = hoveredRow ? 2 : 1.4;
        ctx.setLineDash([]);
      } else if (row.cycle) {
        ctx.strokeStyle = theme.amber;
        ctx.lineWidth = hoveredRow ? 1.8 : 1.1;
        ctx.setLineDash([4, 3]);
      } else {
        ctx.strokeStyle = theme.blue;
        ctx.lineWidth = hoveredRow ? 2 : 1;
        ctx.setLineDash([]);
      }
      if (hoveredRow && !state.reducedMotion) {
        ctx.setLineDash([8, 6]);
        ctx.lineDashOffset = -((performance.now() / 40) % 14);
      }
      ctx.globalAlpha = hoveredRow ? ease : 0.7 * ease;
      ctx.stroke();
      ctx.setLineDash([]);
      ctx.lineDashOffset = 0;
      ctx.globalAlpha = ease;
    }

    ctx.textBaseline = "middle";
    ctx.textAlign = side === "left" ? "right" : "left";

    if (row.kind === "header" || row.kind === "more") {
      ctx.font = FONT_MICRO;
      ctx.fillStyle = theme.textMuted;
      ctx.fillText(row.kind === "header" ? row.label.toUpperCase() : row.label, textX, rowY);
      continue;
    }

    const dotX = colX - dirSign * 6 + slide;
    if (row.kind === "file" && row.fileIndex !== undefined) {
      ctx.fillStyle = lensColor(state.lens, theme, state.index, state.data.files[row.fileIndex]);
    } else {
      ctx.fillStyle = theme.borderStrong;
    }
    ctx.beginPath();
    ctx.arc(dotX, rowY, 4, 0, Math.PI * 2);
    ctx.fill();

    ctx.font = FONT_SMALL;
    const maxTextW = side === "left" ? colX - 44 : stageW - colX - 44;
    if (row.kind === "group") {
      const label = `${row.label} (${row.count ?? 0})`;
      ctx.fillStyle = row.violation ? theme.redText : row.cycle ? theme.amberText : theme.textHigh;
      ctx.fillText(middleTruncate(ctx, label, Math.min(maxTextW, 320)), textX, rowY);
    } else {
      const dim = row.dim ? `${row.dim}/` : "";
      const nameColor = row.violation ? theme.redText : row.cycle ? theme.amberText : theme.textHigh;
      const name = row.cycle ? `${row.label} ~` : row.label;
      const nameW = ctx.measureText(name).width;
      let drawDim = dim;
      if (ctx.measureText(dim).width + nameW > maxTextW) {
        drawDim = tailTruncate(ctx, dim, Math.max(0, maxTextW - nameW));
      }
      const totalW = nameW + ctx.measureText(drawDim).width;
      ctx.fillStyle = theme.bg;
      const prevAlpha = ctx.globalAlpha;
      ctx.globalAlpha = 0.85 * ease;
      if (side === "left") {
        ctx.fillRect(textX - totalW - 2, rowY - 7, totalW + 4, 14);
      } else {
        ctx.fillRect(textX - 2, rowY - 7, totalW + 4, 14);
      }
      ctx.globalAlpha = prevAlpha;
      if (side === "left") {
        ctx.fillStyle = nameColor;
        ctx.fillText(name, textX, rowY);
        ctx.fillStyle = theme.textMuted;
        ctx.fillText(drawDim, textX - nameW, rowY);
      } else {
        ctx.fillStyle = theme.textMuted;
        ctx.fillText(drawDim, textX, rowY);
        ctx.fillStyle = nameColor;
        ctx.fillText(name, textX + ctx.measureText(drawDim).width, rowY);
      }
    }

    // Leader line to the true map position (spatial identity).
    if (row.kind === "file" && row.fileIndex !== undefined) {
      const node = gvs.fileNodes[row.fileIndex];
      if (node && node.x != null && node.y != null) {
        const s = worldToScreen(gvs, { x: node.x, y: node.y });
        ctx.beginPath();
        ctx.moveTo(dotX, rowY);
        ctx.lineTo(s.x, s.y);
        ctx.strokeStyle = theme.textLow;
        ctx.globalAlpha = 0.08 * ease;
        ctx.lineWidth = 1;
        ctx.stroke();
        ctx.globalAlpha = ease;
      }
    }

    const rectW = Math.min(maxTextW + 40, 460);
    gvs.stageRects.push({
      x: side === "left" ? colX - rectW : colX - 8,
      y: rowY - rowH / 2,
      w: rectW + 8,
      h: rowH,
      kind: row.kind === "group" ? "group" : "file",
      fileIndex: row.fileIndex,
      groupKey: row.groupKey,
    });
  }
};

const drawCrumbs = (state: AppState, gvs: GraphViewState, stageW: number): void => {
  const { ctx, theme, data } = state;
  if (gvs.crumbs.length < 2) return;
  const shown = gvs.crumbs.slice(-6);
  ctx.font = FONT_MICRO;
  ctx.textAlign = "left";
  ctx.textBaseline = "middle";
  let x = 14;
  const y = 14;
  shown.forEach((idx, i) => {
    const name = basename(data.files[idx].path);
    const isLast = i === shown.length - 1;
    const textW = ctx.measureText(name).width;
    if (x + textW > stageW - 40) return;
    ctx.fillStyle = isLast ? theme.textHigh : theme.textLow;
    ctx.fillText(name, x, y);
    if (!isLast) {
      gvs.stageRects.push({
        x: x - 2,
        y: y - 8,
        w: textW + 4,
        h: 16,
        kind: "crumb",
        fileIndex: idx,
      });
    }
    x += textW;
    if (!isLast) {
      ctx.fillStyle = theme.textMuted;
      ctx.fillText(" ▸ ", x, y);
      x += ctx.measureText(" ▸ ").width;
    }
  });
};

/**
 * Truncate a directory prefix from the front, keeping whole trailing
 * segments ("…/mdx-components/"), so the informative tail survives.
 */
const tailTruncate = (
  ctx: CanvasRenderingContext2D,
  dir: string,
  maxWidth: number,
): string => {
  if (dir === "" || ctx.measureText(dir).width <= maxWidth) return dir;
  const parts = dir.split("/").filter((p) => p !== "");
  for (let drop = 1; drop < parts.length; drop++) {
    const candidate = `…/${parts.slice(drop).join("/")}`;
    if (ctx.measureText(candidate).width <= maxWidth) return candidate;
  }
  return ctx.measureText("…/").width <= maxWidth ? "…/" : "";
};

const middleTruncate = (
  ctx: CanvasRenderingContext2D,
  text: string,
  maxWidth: number,
): string => {
  if (text === "") return "";
  if (maxWidth <= 10) return "…";
  if (ctx.measureText(text).width <= maxWidth) return text;
  let lo = 1;
  let hi = text.length;
  while (lo < hi) {
    const mid = (lo + hi + 1) >>> 1;
    const keep = Math.floor(mid / 2);
    const candidate = `${text.slice(0, keep)}…${text.slice(text.length - (mid - keep))}`;
    if (ctx.measureText(candidate).width <= maxWidth) lo = mid;
    else hi = mid - 1;
  }
  const keep = Math.floor(lo / 2);
  return `${text.slice(0, keep)}…${text.slice(text.length - (lo - keep))}`;
};

// ── Hit testing / interaction ───────────────────────────────────

const nodeHitTest = (state: AppState, canvasX: number, canvasY: number): number | null => {
  const gvs = getGVS(state);
  const { transform, fileNodes, clusters } = gvs;
  const gx = (canvasX - transform.x) / transform.k;
  const gy = (canvasY - transform.y) / transform.k;
  // Nearest-wins with a 9px screen-space floor so dots stay clickable
  // at fit zoom.
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

const stageHitTest = (state: AppState, x: number, y: number): StageRect | null => {
  const gvs = getGVS(state);
  for (const rect of gvs.stageRects) {
    if (x >= rect.x && x <= rect.x + rect.w && y >= rect.y && y <= rect.y + rect.h) return rect;
  }
  return null;
};

/** Distance-to-bezier road hit test in screen space (overview only). */
const roadHitTest = (state: AppState, x: number, y: number): number | null => {
  const gvs = getGVS(state);
  const threshold = 8;
  let best: number | null = null;
  let bestDist = threshold;
  for (let ri = 0; ri < gvs.roads.length; ri++) {
    const { p0, p1, p2, p3 } = roadGeometry(gvs, gvs.roads[ri]);
    for (let i = 0; i <= 16; i++) {
      const p = worldToScreen(gvs, cubicPoint(p0, p1, p2, p3, i / 16));
      const d = Math.hypot(p.x - x, p.y - y);
      if (d < bestDist) {
        bestDist = d;
        best = ri;
      }
    }
  }
  return best;
};

/** Resolve a road to its contributing file pairs for the panel. */
const buildRoadSelection = (state: AppState, roadIndex: number): RoadSelection => {
  const gvs = getGVS(state);
  const road = gvs.roads[roadIndex];
  const pairs: Array<[number, number]> = [];
  for (const [from, to] of state.data.edges) {
    if (gvs.clusterOf[from] === road.src && gvs.clusterOf[to] === road.dst) {
      pairs.push([from, to]);
    }
  }
  pairs.sort(
    (a, b) =>
      (state.data.files[a[0]].path < state.data.files[b[0]].path ? -1 : 1) ||
      (state.data.files[a[1]].path < state.data.files[b[1]].path ? -1 : 1),
  );
  return {
    srcKey: gvs.clusters[road.src].key,
    dstKey: gvs.clusters[road.dst].key,
    count: road.count,
    violations: road.violations,
    cycleEdges: road.cycleEdges,
    pairs,
  };
};

/** BFS shortest path over directed imports; falls back to the reverse
 *  direction so a trace works whichever node was clicked first. */
const shortestPath = (state: AppState, from: number, to: number): number[] | null => {
  const bfs = (start: number, goal: number, adj: number[][]): number[] | null => {
    const prev = new Map<number, number>();
    prev.set(start, -1);
    let frontier = [start];
    while (frontier.length > 0) {
      const next: number[] = [];
      for (const v of frontier) {
        if (v === goal) {
          const path: number[] = [];
          let cur = goal;
          while (cur !== -1) {
            path.push(cur);
            cur = prev.get(cur) ?? -1;
          }
          return path.reverse();
        }
        for (const w of adj[v]) {
          if (!prev.has(w)) {
            prev.set(w, v);
            next.push(w);
          }
        }
      }
      frontier = next;
    }
    return null;
  };
  return (
    bfs(from, to, state.index.importsOf) ??
    bfs(to, from, state.index.importsOf) ??
    null
  );
};

/** Start or complete a shift-click dependency trace. Returns true when handled. */
export const graphPathTrace = (state: AppState, x: number, y: number): boolean => {
  if (state.selected !== null) return false;
  const gvs = getGVS(state);
  const node = nodeHitTest(state, x, y);
  if (node === null) {
    // Shift-click is trace-only: a miss never falls through to selection.
    gvs.notice = "shift-click a file dot to trace";
    gvs.noticeAt = performance.now();
    renderGraph(state);
    return true;
  }
  if (gvs.pathFrom === null || gvs.pathFrom === node) {
    gvs.pathFrom = node;
    gvs.path = null;
  } else {
    gvs.path = shortestPath(state, gvs.pathFrom, node);
    if (gvs.path) {
      gvs.pathFrom = null;
    } else {
      gvs.notice = "no dependency path between these files";
      gvs.noticeAt = performance.now();
    }
  }
  renderGraph(state);
  return true;
};

/** Clear road selection / path trace (esc, click-away, view switches). */
export const clearGraphFocus = (state: AppState): boolean => {
  const gvs = getGVS(state);
  const had =
    gvs.selectedRoad !== null || gvs.path !== null || gvs.pathFrom !== null;
  gvs.selectedRoad = null;
  gvs.path = null;
  gvs.pathFrom = null;
  return had;
};

/** Re-fit after a window resize, but only while the camera is untouched. */
export const refitOnResize = (state: AppState): void => {
  const gvs = getGVS(state);
  if (!gvs.initialized || gvs.userMoved || state.selected !== null) return;
  resetGraphView(state);
};

/** Reset the camera to the fit-to-view transform (0 key / after wandering). */
export const resetGraphView = (state: AppState): void => {
  const gvs = getGVS(state);
  if (!gvs.initialized || !gvs.zoomBehavior || state.selected !== null) return;
  // Recompute the fit transform from current cluster bounds.
  const stageEl = state.canvas.parentElement;
  const w = stageEl ? stageEl.clientWidth : window.innerWidth;
  const h = stageEl ? stageEl.clientHeight : window.innerHeight;
  let minX = Infinity;
  let minY = Infinity;
  let maxX = -Infinity;
  let maxY = -Infinity;
  const anyConnected = gvs.clusters.some((c) => !c.isolated);
  for (const c of gvs.clusters) {
    if (c.isolated && anyConnected && !gvs.standaloneOpen) continue;
    minX = Math.min(minX, c.cx - c.r);
    minY = Math.min(minY, c.cy - c.r);
    maxX = Math.max(maxX, c.cx + c.r);
    maxY = Math.max(maxY, c.cy + c.r);
  }
  const pad = 70;
  const bboxW = maxX - minX + pad * 2;
  const bboxH = maxY - minY + pad * 2;
  const fitScale = Math.min((w - 200) / bboxW, (h - 60) / bboxH, 1.4);
  const fitX = (w - bboxW * fitScale) / 2 - minX * fitScale + pad * fitScale;
  const fitY = (h - bboxH * fitScale) / 2 - minY * fitScale + pad * fitScale;
  select(state.canvas).call(
    gvs.zoomBehavior.transform,
    zoomIdentity.translate(fitX, fitY).scale(fitScale),
  );
};

/** Zoom to the first search match and pulse it (Enter in the search box). */
export const graphFocusSearch = (state: AppState): void => {
  const gvs = getGVS(state);
  if (!gvs.initialized || state.searchMatches.size === 0) return;
  let best: number | null = null;
  for (const idx of state.searchMatches) {
    if (best === null || state.data.files[idx].path.length < state.data.files[best].path.length) {
      best = idx;
    }
  }
  if (best === null) return;
  centerOnFile(state, best);
  gvs.pulseFile = best;
  gvs.pulseAt = performance.now();
  renderGraph(state);
};

/** What the cursor is over (drives hover state, cursor, and tooltip). */
export const graphHoverTarget = (state: AppState, x: number, y: number): GraphHoverTarget => {
  const gvs = getGVS(state);
  if (state.selected !== null) {
    gvs.hoveredRoad = null;
    const back = gvs.egoBackChip;
    if (back && x >= back.x && x <= back.x + back.w && y >= back.y && y <= back.y + back.h) {
      return { kind: "ui" };
    }
    const rect = stageHitTest(state, x, y);
    if (rect) {
      if (rect.kind !== "group" && rect.fileIndex !== undefined) {
        return { kind: "file", fileIndex: rect.fileIndex };
      }
      return { kind: "ui" };
    }
    const node = nodeHitTest(state, x, y);
    return node !== null ? { kind: "file", fileIndex: node } : null;
  }
  const chip = gvs.standaloneChip;
  if (chip && x >= chip.x && x <= chip.x + chip.w && y >= chip.y && y <= chip.y + chip.h) {
    gvs.hoveredRoad = null;
    return { kind: "ui" };
  }
  const node = nodeHitTest(state, x, y);
  if (node !== null) {
    gvs.hoveredRoad = null;
    return { kind: "file", fileIndex: node };
  }
  const road = roadHitTest(state, x, y);
  gvs.hoveredRoad = road;
  return road !== null ? { kind: "road", road } : null;
};

/** Drop road hover when the cursor leaves the canvas; true if state changed. */
export const clearRoadHover = (state: AppState): boolean => {
  const gvs = getGVS(state);
  if (gvs.hoveredRoad === null) return false;
  gvs.hoveredRoad = null;
  return true;
};

/** Road facts for the tooltip (hover). */
export const roadFacts = (
  state: AppState,
  roadIndex: number,
): { srcKey: string; dstKey: string; count: number; violations: number; cycleEdges: number } => {
  const gvs = getGVS(state);
  const road = gvs.roads[roadIndex];
  return {
    srcKey: gvs.clusters[road.src].key,
    dstKey: gvs.clusters[road.dst].key,
    count: road.count,
    violations: road.violations,
    cycleEdges: road.cycleEdges,
  };
};

/** Handle a primary click; the caller applies file selection. */
export const graphHandleClick = (state: AppState, x: number, y: number): GraphClickResult => {
  const gvs = getGVS(state);
  if (state.selected !== null) {
    const back = gvs.egoBackChip;
    if (back && x >= back.x && x <= back.x + back.w && y >= back.y && y <= back.y + back.h) {
      return { kind: "none" };
    }
    const rect = stageHitTest(state, x, y);
    if (rect) {
      if (rect.kind !== "group" && rect.fileIndex !== undefined) {
        return { kind: "file", fileIndex: rect.fileIndex };
      }
      if (rect.kind === "group" && rect.groupKey) {
        if (gvs.egoExpanded.has(rect.groupKey)) gvs.egoExpanded.delete(rect.groupKey);
        else gvs.egoExpanded.add(rect.groupKey);
        return { kind: "handled" };
      }
      return { kind: "handled" };
    }
    const node = nodeHitTest(state, x, y);
    if (node !== null) return { kind: "file", fileIndex: node };
    return { kind: "none" };
  }
  const chip = gvs.standaloneChip;
  if (chip && x >= chip.x && x <= chip.x + chip.w && y >= chip.y && y <= chip.y + chip.h) {
    gvs.standaloneOpen = !gvs.standaloneOpen;
    renderGraph(state);
    return { kind: "handled" };
  }
  const node = nodeHitTest(state, x, y);
  if (node !== null) return { kind: "file", fileIndex: node };
  const road = roadHitTest(state, x, y);
  if (road !== null) {
    gvs.selectedRoad = road;
    return { kind: "road", road: buildRoadSelection(state, road) };
  }
  if (clearGraphFocus(state)) return { kind: "handled" };
  return { kind: "none" };
};

/** Reset ego navigation history (call when selection is cleared). */
export const resetEgoTrail = (state: AppState): void => {
  const gvs = getGVS(state);
  gvs.crumbs = [];
  gvs.egoExpanded.clear();
  gvs.lastRoot = null;
};

/** Pan/zoom so a file's node is centered (overview only; ego centers itself). */
export const centerOnFile = (state: AppState, fileIndex: number): void => {
  const gvs = getGVS(state);
  if (!gvs.initialized || !gvs.zoomBehavior || state.selected !== null) return;
  const node = gvs.fileNodes[fileIndex];
  if (!node || node.x == null || node.y == null) return;
  const stageEl = state.canvas.parentElement;
  const w = stageEl ? stageEl.clientWidth : window.innerWidth;
  const h = stageEl ? stageEl.clientHeight : window.innerHeight;
  const k = Math.max(gvs.transform.k, gvs.fitK * 1.5);
  const target = zoomIdentity.translate(w / 2 - node.x * k, h / 2 - node.y * k).scale(k);
  select(state.canvas).call(gvs.zoomBehavior.transform, target);
};
