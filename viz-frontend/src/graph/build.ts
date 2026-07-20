/**
 * Graph construction: clustering (by folder or import community), the
 * Sugiyama-lite rank layout with its wrap and stagger passes, frozen
 * seeded per-cluster force layouts, hulls, and the one-time init that
 * wires the zoom camera.
 */
import { forceCollide, forceLink, forceManyBody, forceSimulation, forceX, forceY } from "d3-force";
import { select } from "d3-selection";
import { zoom, zoomIdentity, type D3ZoomEvent } from "d3-zoom";
import Graph from "graphology";
import louvain from "graphology-communities-louvain";
import type { AppState } from "../state";
import type { VizFile } from "../types";
import {
  type ClusterInfo,
  type FileNode,
  type GraphViewState,
  type LocalLink,
  type Pt,
  LAYER_GAP,
  MAX_CLUSTERS,
  NODE_R_MAX,
  NODE_R_MIN,
  ROW_GAP,
  buildSpatialGrid,
  clusterBounds,
  fitTransform,
  getGVS,
  shouldShowIntro,
  stageSize,
} from "./shared";
import { renderGraph } from "./render";

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

// ── Clustering ──────────────────────────────────────────────────

/**
 * A folder at or below this many files is treated as one cohesive module and
 * kept whole; a bigger folder is a *container* (a repo's `src`, or
 * `src/features` / `src/components` that hold many modules) and splits into
 * one cluster per child. Tuned so individual features/components (tens of
 * files) stand on their own instead of collapsing into a top-level blob.
 */
const SPLIT_TARGET = 100;
/** Safety cap on how deep a container chain will keep subdividing. */
const MAX_SPLIT_DEPTH = 6;
/**
 * A child folder with fewer than this many files folds back into the
 * container's residual bucket instead of getting its own cluster. This keeps
 * substantial modules (feature domains, the big components) as their own
 * groups while the long tail of tiny uniform folders, e.g. a design system's
 * 3-file atomic components, collapses into one "src/components" group rather
 * than a confetti of dozens of near-identical blobs.
 */
const MIN_CHILD = 8;

const directoryCluster = (files: VizFile[]): Map<string, number[]> => {
  const result = new Map<string, number[]>();
  const emit = (key: string, indices: number[]): void => {
    if (indices.length === 0) return;
    const existing = result.get(key);
    if (existing) existing.push(...indices);
    else result.set(key, indices);
  };
  // Split files sharing `prefix` by their folder segment at `depth`. Files
  // that live directly in `prefix` (no deeper segment) group under it.
  const split = (indices: number[], prefix: string, depth: number): void => {
    if (indices.length <= SPLIT_TARGET || depth >= MAX_SPLIT_DEPTH || result.size >= MAX_CLUSTERS) {
      emit(prefix, indices);
      return;
    }
    const groups = new Map<string, number[]>();
    for (const i of indices) {
      const parts = files[i].path.split("/");
      const key = depth < parts.length - 1 ? `${prefix}/${parts[depth]}` : prefix;
      const bucket = groups.get(key);
      if (bucket) bucket.push(i);
      else groups.set(key, [i]);
    }
    // Nothing to gain: one child, or everything sits directly in `prefix`.
    if (groups.size <= 1) {
      emit(prefix, indices);
      return;
    }
    // A substantial child earns its own cluster and recurses; direct files and
    // small children fold into one residual bucket under the container.
    const substantial: Array<[string, number[]]> = [];
    const residual: number[] = [];
    for (const [key, idx] of groups) {
      if (key !== prefix && idx.length >= MIN_CHILD) substantial.push([key, idx]);
      else residual.push(...idx);
    }
    if (substantial.length === 0) {
      emit(prefix, indices);
      return;
    }
    // Largest children first so the biggest containers split before the
    // cluster budget runs out; a child that is itself a container recurses.
    const ordered = substantial.toSorted(
      (a, b) => b[1].length - a[1].length || (a[0] < b[0] ? -1 : 1),
    );
    for (const [key, idx] of ordered) split(idx, key, depth + 1);
    emit(prefix, residual);
  };
  const top = new Map<string, number[]>();
  for (let i = 0; i < files.length; i++) {
    const seg = files[i].path.split("/")[0];
    const bucket = top.get(seg);
    if (bucket) bucket.push(i);
    else top.set(seg, [i]);
  }
  const orderedTop = [...top.entries()].toSorted(
    (a, b) => b[1].length - a[1].length || (a[0] < b[0] ? -1 : 1),
  );
  for (const [seg, idx] of orderedTop) split(idx, seg, 1);
  return new Map([...result.entries()].toSorted((a, b) => (a[0] < b[0] ? -1 : 1)));
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
  // Name each community by the folder that dominates it. A community is an
  // import group, not a folder, so the plurality folder only names it honestly
  // when it is a real MAJORITY (Charts, Calendar, email): then use that folder
  // directly. When no folder reaches a majority the community is genuinely
  // cross-cutting, so label it by its top-level area with a `· mixed` marker
  // (`src/components · mixed`) rather than let a 9%-of-the-files folder imply
  // it owns all 128. Two mixed areas that collide promote to their biggest
  // slice (`src/features/ai · mixed`) so the labels stay distinct.
  const comms = [...communityMap.values()];
  const MAJORITY = 0.5;
  const dominantFolder = (indices: number[], depth: number): { name: string; count: number } => {
    const counts = new Map<string, number>();
    for (const idx of indices) {
      const parts = files[idx].path.split("/");
      const dirs = parts.length > 1 ? parts.slice(0, -1) : parts;
      const key = dirs.slice(0, depth).join("/");
      counts.set(key, (counts.get(key) ?? 0) + 1);
    }
    const sorted = [...counts.entries()].toSorted((a, b) => b[1] - a[1] || (a[0] < b[0] ? -1 : 1));
    return { name: sorted[0]?.[0] ?? "misc", count: sorted[0]?.[1] ?? 0 };
  };
  // Each community wants a `preferred` label, falling back to a more specific
  // `fallback` only when the preferred one collides with another community.
  const labels = comms.map((indices) => {
    const total = indices.length || 1;
    const specific = dominantFolder(indices, 3);
    if (specific.count / total >= MAJORITY)
      return { preferred: specific.name, fallback: specific.name };
    const area = dominantFolder(indices, 2).name;
    return { preferred: `${area} · mixed`, fallback: `${specific.name} · mixed` };
  });
  const preferredCount = new Map<string, number>();
  for (const l of labels)
    preferredCount.set(l.preferred, (preferredCount.get(l.preferred) ?? 0) + 1);
  const result = new Map<string, number[]>();
  comms.forEach((indices, i) => {
    let name =
      (preferredCount.get(labels[i].preferred) ?? 0) > 1 ? labels[i].fallback : labels[i].preferred;
    while (result.has(name)) name = `${name}*`;
    result.set(name, indices);
  });
  return new Map([...result.entries()].toSorted((a, b) => (a[0] < b[0] ? -1 : 1)));
};
// ── Meta-graph, SCC condensation, layering, ordering ────────────

export interface MetaEdge {
  src: number;
  dst: number;
  count: number;
  violations: number;
  cycleEdges: number;
}

const buildMetaGraph = (state: AppState, clusterOf: number[], clusterCount: number): MetaEdge[] => {
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
  return [...buckets.values()].toSorted((a, b) => a.src - b.src || a.dst - b.dst);
};

/** Iterative Tarjan SCC over the cluster meta-graph. */
export const tarjanSCC = (count: number, adj: number[][]): number[] => {
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
export const assignLayers = (clusterCount: number, meta: MetaEdge[], sccOf: number[]): number[] => {
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

/** Flowing (non-isolated) clusters grouped by their layer index. */
const groupByLayer = (clusters: ClusterInfo[]): Map<number, ClusterInfo[]> => {
  const byLayer = new Map<number, ClusterInfo[]>();
  for (const c of clusters) {
    if (c.isolated) continue;
    if (!byLayer.has(c.layer)) byLayer.set(c.layer, []);
    byLayer.get(c.layer)?.push(c);
  }
  return byLayer;
};

/** 4 weighted barycenter sweeps for within-layer ordering. */
const orderWithinLayers = (clusters: ClusterInfo[], meta: MetaEdge[]): void => {
  const byLayer = groupByLayer(clusters);
  for (const list of byLayer.values()) {
    const sorted = list.toSorted((a, b) => (a.key < b.key ? -1 : 1));
    sorted.forEach((c, i) => {
      c.order = i;
    });
  }

  const neighbors = new Map<number, Array<{ other: number; w: number }>>();
  clusters.forEach((_, i) => neighbors.set(i, []));
  for (const e of meta) {
    neighbors.get(e.src)?.push({ other: e.dst, w: e.count });
    neighbors.get(e.dst)?.push({ other: e.src, w: e.count });
  }

  const layerKeys = [...byLayer.keys()].toSorted((a, b) => a - b);
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
      const orderedScored = scored.toSorted(
        (a, b) => a.bary - b.bary || (a.c.key < b.c.key ? -1 : 1),
      );
      orderedScored.forEach((s, i) => {
        s.c.order = i;
      });
    }
  };

  sweep(layerKeys);
  sweep(layerKeys.toReversed());
  sweep(layerKeys);
  sweep(layerKeys.toReversed());
};

/** Column x per layer, rows stacked by barycenter order. */
const stackLayers = (byLayer: Map<number, ClusterInfo[]>, layerKeys: number[]): void => {
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
    const list = (byLayer.get(layer) ?? []).toSorted((a, b) => a.order - b.order);
    let y = 0;
    list.forEach((c, i) => {
      if (i > 0) y += list[i - 1].r + c.r + ROW_GAP;
      c.cy = y;
    });
  }
};

/** Pull rows toward their neighbors' weighted mean, keeping row gaps. */
const relaxRows = (
  clusters: ClusterInfo[],
  meta: MetaEdge[],
  byLayer: Map<number, ClusterInfo[]>,
  layerKeys: number[],
): void => {
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
      const list = (byLayer.get(layer) ?? []).toSorted((a, b) => a.order - b.order);
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
};

/** Align every layer's midline to the global midline; returns it. */
const centerLayers = (
  flowing: ClusterInfo[],
  byLayer: Map<number, ClusterInfo[]>,
  layerKeys: number[],
): number => {
  const globalMid = flowing.reduce((sum, c) => sum + c.cy, 0) / Math.max(1, flowing.length);
  for (const layer of layerKeys) {
    const list = byLayer.get(layer) ?? [];
    const mid = list.reduce((s, c) => s + c.cy, 0) / Math.max(1, list.length);
    for (const c of list) c.cy += globalMid - mid;
  }
  return globalMid;
};

const bboxAspect = (flowing: ClusterInfo[]): number => {
  const b = clusterBounds(flowing, () => true);
  return (b.maxX - b.minX) / Math.max(1, b.maxY - b.minY);
};

/**
 * Re-wrap a portrait layout into rows targeting a wide aspect.
 * Import-community clustering collapses into few layers (the
 * communities import each other), which the rank layout would stack
 * as one tall column. Returns true when it applied.
 */
const wrapPortraitRows = (flowing: ClusterInfo[]): boolean => {
  if (bboxAspect(flowing) >= 1 || flowing.length <= 3) return false;
  const GRID_GAP = 150;
  const list = [...flowing].toSorted((a, b) => a.layer - b.layer || a.cy - b.cy);
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
  return true;
};

/**
 * Spread a flat ribbon (aspect 4:1+) vertically toward a presentable
 * aspect and stagger single-cluster layers off the midline; x gaps
 * between layers make the stagger collision-free.
 */
const spreadToAspect = (
  flowing: ClusterInfo[],
  byLayer: Map<number, ClusterInfo[]>,
  layerKeys: number[],
  globalMid: number,
): void => {
  if (flowing.length <= 1) return;
  const aspect = bboxAspect(flowing);
  const TARGET_ASPECT = 2.2;
  if (aspect <= TARGET_ASPECT) return;
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
};

/** Coordinate assignment: stack, relax, center, then shape the aspect. */
export const assignCoordinates = (clusters: ClusterInfo[], meta: MetaEdge[]): void => {
  const byLayer = groupByLayer(clusters);
  const layerKeys = [...byLayer.keys()].toSorted((a, b) => a - b);
  const flowing = clusters.filter((c) => !c.isolated);
  stackLayers(byLayer, layerKeys);
  relaxRows(clusters, meta, byLayer, layerKeys);
  const globalMid = centerLayers(flowing, byLayer, layerKeys);
  if (wrapPortraitRows(flowing)) return;
  spreadToAspect(flowing, byLayer, layerKeys, globalMid);
};

/** Park isolated clusters in a compact strip below the dependency flow. */
const placeIsolated = (clusters: ClusterInfo[]): void => {
  const isolated = clusters.filter((c) => c.isolated).toSorted((a, b) => (a.key < b.key ? -1 : 1));
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
// ── Edge partitions ─────────────────────────────────────────────

/**
 * Split the raw edge list into same-cluster and cross-cluster subsets in
 * one pass, plus a per-cluster bucket for the local layouts. Rebuilt
 * whenever clustering reruns (initGraphNodes), so the per-frame edge
 * passes and per-cluster layouts stop rescanning every edge.
 */
export const partitionEdges = (
  edges: ReadonlyArray<[number, number, number]>,
  clusterOf: number[],
  clusterCount: number,
): {
  intra: Array<[number, number]>;
  inter: Array<[number, number]>;
  byCluster: Array<Array<[number, number]>>;
} => {
  const intra: Array<[number, number]> = [];
  const inter: Array<[number, number]> = [];
  const byCluster: Array<Array<[number, number]>> = Array.from({ length: clusterCount }, () => []);
  for (const [from, to] of edges) {
    const a = clusterOf[from];
    const b = clusterOf[to];
    if (a === undefined || b === undefined) continue;
    if (a === b) {
      intra.push([from, to]);
      byCluster[a].push([from, to]);
    } else {
      inter.push([from, to]);
    }
  }
  return { intra, inter, byCluster };
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
    for (const [from, to] of gvs.linksByCluster[ci]) {
      const a = inCluster.get(from);
      const b = inCluster.get(to);
      if (a && b && a !== b) links.push({ source: a, target: b });
    }

    const sim = forceSimulation(nodes)
      .randomSource(rand)
      .force("link", forceLink<FileNode, LocalLink>(links).distance(24).strength(0.3))
      .force("charge", forceManyBody<FileNode>().strength(-30).theta(0.9).distanceMax(240))
      .force(
        "collide",
        forceCollide<FileNode>((d) => d.radius + 2),
      )
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
const convexHull = (pts: Pt[]): Pt[] => {
  const sorted = [...pts].toSorted((a, b) => a.x - b.x || a.y - b.y);
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

  const partitions = partitionEdges(data.edges, clusterOf, clusters.length);
  gvs.intraEdges = partitions.intra;
  gvs.interEdges = partitions.inter;
  gvs.linksByCluster = partitions.byCluster;

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
  // An edge-free project marks every cluster isolated; the standalone
  // strip must then open by default or the map renders as an empty canvas.
  if (!clusters.some((c) => !c.isolated)) gvs.standaloneOpen = true;

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
  // Positions are frozen from here on; index them for pointer hit-tests.
  gvs.grid = buildSpatialGrid(gvs.fileNodes);

  // Hub floor: p95 of importer counts, min 25 (spec: badge, never suppress).
  const importerCounts = files
    .map((f) => f.importer_count)
    .filter((c) => c > 0)
    .toSorted((a, b) => a - b);
  const p95 =
    importerCounts.length > 0
      ? importerCounts[
          Math.min(importerCounts.length - 1, Math.floor(importerCounts.length * 0.95))
        ]
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

  // Fit-to-view. Standalone clusters are hidden until toggled: keep
  // them out of the fit.
  const { w, h } = stageSize(state);
  const anyConnected = clusters.some((c) => !c.isolated);
  const fit = fitTransform(
    w,
    h,
    clusterBounds(clusters, (c) => !(c.isolated && anyConnected)),
  );
  gvs.transform = fit;
  gvs.fitK = fit.k;

  const zoomBehavior = zoom<HTMLCanvasElement, unknown>()
    .scaleExtent([fit.k * 0.4, fit.k * 12])
    // A drag pans from ANYWHERE (including a node or road); selection runs on
    // the native `click` in main.ts. clickDistance lets a genuine click's
    // `click` event through while d3 suppresses it after a real drag, so d3
    // owning the gesture no longer swallows clicks.
    .clickDistance(4)
    .filter((event: MouseEvent | WheelEvent) => {
      // Only the graph view pans/zooms via d3; the treemap has its own drill,
      // and the ego camera is frozen.
      if (state.view !== "graph") return false;
      if (state.selected !== null) return false;
      if (event.type === "wheel") return !event.ctrlKey;
      return !(event as MouseEvent).button;
    })
    .on("zoom", (event: D3ZoomEvent<HTMLCanvasElement, unknown>) => {
      if (event.sourceEvent) gvs.userMoved = true;
      gvs.transform = { x: event.transform.x, y: event.transform.y, k: event.transform.k };
      renderGraph(state);
    });
  const initialTransform = zoomIdentity.translate(fit.x, fit.y).scale(fit.k);
  select(canvas).call(zoomBehavior).call(zoomBehavior.transform, initialTransform);
  gvs.zoomBehavior = zoomBehavior;

  gvs.initialized = true;
  if (gvs.hasRevealed) {
    // A re-arrange (by folder / by imports) is a compare gesture: paint
    // the new layout immediately instead of replaying the opening sweep.
    gvs.revealAt = -1;
  } else {
    gvs.revealAt = 0;
    gvs.showIntro = shouldShowIntro() && !state.reducedMotion;
    gvs.hasRevealed = true;
  }
  renderGraph(state);
};
