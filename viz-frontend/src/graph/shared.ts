/**
 * Shared internals of the graph view: view-state, tokens, deterministic
 * geometry, and text measurement helpers. Every sibling module leans on
 * this file; it must not import from them.
 */
import type { SimulationLinkDatum, SimulationNodeDatum } from "d3-force";
import type { ZoomBehavior } from "d3-zoom";
import type { AppState } from "../state";
import type { RoadSelection } from "../types";

// ── Types ───────────────────────────────────────────────────────

export interface FileNode extends SimulationNodeDatum {
  fileIndex: number;
  radius: number;
  cluster: number;
}

export type LocalLink = SimulationLinkDatum<FileNode>;

export interface ClusterInfo {
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

export interface Road {
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

export interface StageRect {
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
  | { kind: "cluster"; cluster: number }
  | { kind: "ui" }
  | null;

export type GraphClickResult =
  | { kind: "file"; fileIndex: number }
  | { kind: "road"; road: RoadSelection }
  | { kind: "handled" }
  | { kind: "none" };

/** Uniform spatial grid over world coordinates for pointer hit-tests. */
export interface SpatialGrid {
  /** World units per cell. */
  cell: number;
  cols: number;
  rows: number;
  minX: number;
  minY: number;
  /** Node indices per cell, row-major. */
  buckets: number[][];
  /** Largest node radius in world units. */
  maxRadius: number;
}

/**
 * Index node positions into a uniform grid. Visibility (isolated
 * clusters, standalone toggle) is NOT baked in because it changes at
 * runtime; hit loops keep their own per-node visibility checks.
 */
export const buildSpatialGrid = (
  nodes: ReadonlyArray<FileNode | undefined>,
): SpatialGrid | null => {
  let minX = Infinity;
  let minY = Infinity;
  let maxX = -Infinity;
  let maxY = -Infinity;
  let maxRadius = 0;
  for (const node of nodes) {
    if (!node || node.x == null || node.y == null) continue;
    minX = Math.min(minX, node.x);
    minY = Math.min(minY, node.y);
    maxX = Math.max(maxX, node.x);
    maxY = Math.max(maxY, node.y);
    maxRadius = Math.max(maxRadius, node.radius);
  }
  if (!Number.isFinite(minX)) return null;
  const cell = Math.max(2 * maxRadius, 40);
  const cols = Math.max(1, Math.floor((maxX - minX) / cell) + 1);
  const rows = Math.max(1, Math.floor((maxY - minY) / cell) + 1);
  const buckets: number[][] = Array.from({ length: cols * rows }, () => []);
  for (let nodeIndex = 0; nodeIndex < nodes.length; nodeIndex++) {
    const node = nodes[nodeIndex];
    if (!node || node.x == null || node.y == null) continue;
    const cx = Math.min(cols - 1, Math.max(0, Math.floor((node.x - minX) / cell)));
    const cy = Math.min(rows - 1, Math.max(0, Math.floor((node.y - minY) / cell)));
    buckets[cy * cols + cx].push(nodeIndex);
  }
  return { cell, cols, rows, minX, minY, buckets, maxRadius };
};

/** Node indices from every cell overlapping the circle (gx, gy, worldRadius). */
export const gridQuery = (
  grid: SpatialGrid,
  gx: number,
  gy: number,
  worldRadius: number,
): number[] => {
  const minCx = Math.max(0, Math.floor((gx - worldRadius - grid.minX) / grid.cell));
  const maxCx = Math.min(grid.cols - 1, Math.floor((gx + worldRadius - grid.minX) / grid.cell));
  const minCy = Math.max(0, Math.floor((gy - worldRadius - grid.minY) / grid.cell));
  const maxCy = Math.min(grid.rows - 1, Math.floor((gy + worldRadius - grid.minY) / grid.cell));
  const out: number[] = [];
  for (let cy = minCy; cy <= maxCy; cy++) {
    for (let cx = minCx; cx <= maxCx; cx++) {
      for (const nodeIndex of grid.buckets[cy * grid.cols + cx]) out.push(nodeIndex);
    }
  }
  return out;
};

export interface GraphViewState {
  fileNodes: FileNode[];
  clusters: ClusterInfo[];
  clusterOf: number[];
  roads: Road[];
  /** Same-cluster [from, to] edges, precomputed once per clustering pass. */
  intraEdges: Array<[number, number]>;
  /** Cross-cluster [from, to] edges, precomputed once per clustering pass. */
  interEdges: Array<[number, number]>;
  /** Intra-cluster edges bucketed by cluster index, for local layouts. */
  linksByCluster: Array<Array<[number, number]>>;
  /** Spatial hit-test grid over the frozen node positions (null pre-init). */
  grid: SpatialGrid | null;
  /** Importer-count floor above which a node gets the hub badge. */
  hubFloor: number;
  transform: { x: number; y: number; k: number };
  fitK: number;
  initialized: boolean;
  clusterMode: ClusterMode;
  zoomBehavior: ZoomBehavior<HTMLCanvasElement, unknown> | null;
  /** Ego stage state. */
  egoExpanded: Set<string>;
  crumbs: number[];
  stageRects: StageRect[];
  stageEnterAt: number;
  lastRoot: number | null;
  raf: number;
  /** Hovered road index (overview). */
  hoveredRoad: number | null;
  /** Hovered cluster index (label hover lights up its roads). */
  hoveredCluster: number | null;
  /** Cluster-label hit rects (screen space), rebuilt each label pass. */
  clusterLabels: Array<{ cluster: number; x: number; y: number; w: number; h: number }>;
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
  /** True once the opening reveal has played; a re-arrange then skips it. */
  hasRevealed: boolean;
  /** Graph lens-color crossfade: prior per-file colors, and its start time. */
  lensPrev: Map<number, string> | null;
  lensFadeAt: number;
}

export const FONT_SMALL = '12px "Martian Mono", "JetBrains Mono", ui-monospace, Menlo, monospace';
export const FONT_MICRO = '11px "Martian Mono", "JetBrains Mono", ui-monospace, Menlo, monospace';
export const FONT_CHIP = '13px "Martian Mono", "JetBrains Mono", ui-monospace, Menlo, monospace';
export const FONT_LEGEND = '12px "Martian Mono", "JetBrains Mono", ui-monospace, Menlo, monospace';
export const FONT_CARD =
  '700 15px "Martian Mono", "JetBrains Mono", ui-monospace, Menlo, monospace';

export const NODE_R_MIN = 2.5;
export const NODE_R_MAX = 10;
export const MAX_CLUSTERS = 60;
export const LAYER_GAP = 170;
export const ROW_GAP = 56;
export const STAGE_ENTER_MS = 220;
/** Relative-zoom LOD thresholds (k / fit-to-view k). */
export const LOD_INTRA = 1.6;
export const LOD_INTER = 3.0;
export const LOD_SEVERITY = 0.9;
// ── State accessor ──────────────────────────────────────────────

export const getGVS = (state: AppState): GraphViewState => {
  const ext = state as AppState & { _gvs?: GraphViewState };
  if (!ext._gvs) {
    ext._gvs = {
      fileNodes: [],
      clusters: [],
      clusterOf: [],
      roads: [],
      intraEdges: [],
      interEdges: [],
      linksByCluster: [],
      grid: null,
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
      hoveredCluster: null,
      clusterLabels: [],
      selectedRoad: null,
      pathFrom: null,
      path: null,
      pulseFile: null,
      pulseAt: 0,
      notice: "",
      noticeAt: 0,
      revealAt: 0,
      hasRevealed: false,
      lensPrev: null,
      lensFadeAt: 0,
      standaloneOpen: false,
      standaloneChip: null,
      egoBackChip: null,
      userMoved: false,
      showIntro: false,
    };
  }
  return ext._gvs;
};
export interface Pt {
  x: number;
  y: number;
}

const INTRO_KEY = "fallow-viz-intro-seen";

export const shouldShowIntro = (): boolean => {
  try {
    if (new URLSearchParams(window.location.search).get("intro") === "1") return true;
    return window.localStorage.getItem(INTRO_KEY) === null;
  } catch {
    return true;
  }
};

export const markIntroSeen = (): void => {
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
  const alongA = ((b1.x - a1.x) * d2y - (b1.y - a1.y) * d2x) / denom;
  const alongB = ((b1.x - a1.x) * d1y - (b1.y - a1.y) * d1x) / denom;
  if (alongA < 0 || alongA > 1 || alongB < 0 || alongB > 1) return null;
  return { x: a1.x + alongA * d1x, y: a1.y + alongA * d1y };
};

/** Where the segment from a cluster's centre toward `toward` leaves its hull. */
const gatePoint = (cluster: ClusterInfo, toward: Pt): Pt => {
  const from = { x: cluster.cx, y: cluster.cy };
  const hull = cluster.hull;
  for (let index = 0; index < hull.length; index++) {
    const hit = segIntersect(from, toward, hull[index], hull[(index + 1) % hull.length]);
    if (hit) return hit;
  }
  return from;
};

export const cubicPoint = (p0: Pt, p1: Pt, p2: Pt, p3: Pt, progress: number): Pt => {
  const inverse = 1 - progress;
  return {
    x:
      inverse * inverse * inverse * p0.x +
      3 * inverse * inverse * progress * p1.x +
      3 * inverse * progress * progress * p2.x +
      progress * progress * progress * p3.x,
    y:
      inverse * inverse * inverse * p0.y +
      3 * inverse * inverse * progress * p1.y +
      3 * inverse * progress * progress * p2.y +
      progress * progress * progress * p3.y,
  };
};

/** Trace a tapered ribbon polygon along a cubic bezier into the current path. */
export const taperedRibbon = (
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
  for (let sampleIndex = 0; sampleIndex <= SAMPLES; sampleIndex++)
    centers.push(cubicPoint(p0, p1, p2, p3, sampleIndex / SAMPLES));
  const left: Pt[] = [];
  const right: Pt[] = [];
  for (let sampleIndex = 0; sampleIndex <= SAMPLES; sampleIndex++) {
    const progress = sampleIndex / SAMPLES;
    const prev = centers[Math.max(0, sampleIndex - 1)];
    const next = centers[Math.min(SAMPLES, sampleIndex + 1)];
    const dx = next.x - prev.x;
    const dy = next.y - prev.y;
    const len = Math.max(1e-6, Math.hypot(dx, dy));
    const nx = -dy / len;
    const ny = dx / len;
    const hw = (wSrc * (1 - progress) + wDst * progress) / 2;
    left.push({ x: centers[sampleIndex].x + nx * hw, y: centers[sampleIndex].y + ny * hw });
    right.push({ x: centers[sampleIndex].x - nx * hw, y: centers[sampleIndex].y - ny * hw });
  }
  ctx.moveTo(left[0].x, left[0].y);
  for (let sampleIndex = 1; sampleIndex <= SAMPLES; sampleIndex++)
    ctx.lineTo(left[sampleIndex].x, left[sampleIndex].y);
  for (let sampleIndex = SAMPLES; sampleIndex >= 0; sampleIndex--)
    ctx.lineTo(right[sampleIndex].x, right[sampleIndex].y);
  ctx.closePath();
};

export const roadGeometry = (
  gvs: GraphViewState,
  road: Road,
): { p0: Pt; p1: Pt; p2: Pt; p3: Pt } => {
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

  const chord = Math.hypot(p3.x - p0.x, p3.y - p0.y) || 1;
  const ux = (p3.x - p0.x) / chord;
  const uy = (p3.y - p0.y) / chord;
  // Perpendicular to the chord, biased to arc toward the top of the canvas.
  let px = -uy;
  let py = ux;
  if (py > 0) {
    px = -px;
    py = -py;
  }
  let bow = 0;
  if (road.back) {
    bow = 0.18 * chord; // back-edges arc off the fabric
  } else {
    const span = Math.abs(gvs.clusters[road.dst].layer - gvs.clusters[road.src].layer);
    // Long hops bow gently over intermediate layers instead of cutting
    // through their hulls.
    if (span >= 2) bow = 0.06 * chord;
  }
  // Handles run ALONG the chord so each road leaves its cluster pointing at
  // the other (a natural radiating fan), then the perpendicular bow lifts
  // long and back hops off the fabric.
  const handle = chord * 0.4;
  const p1 = { x: p0.x + ux * handle + px * bow, y: p0.y + uy * handle + py * bow };
  const p2 = { x: p3.x - ux * handle + px * bow, y: p3.y - uy * handle + py * bow };
  return { p0, p1, p2, p3 };
};

/** Rounded chip backing: fill plus 1px border, radius 4. */
export const chipRect = (
  ctx: CanvasRenderingContext2D,
  x: number,
  y: number,
  width: number,
  height: number,
  fill: string,
  fillAlpha: number,
  stroke: string | null,
): void => {
  ctx.beginPath();
  ctx.roundRect(x, y, width, height, 4);
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

export const roadWidth = (count: number): number =>
  Math.min(8, Math.max(1.5, 1 + Math.floor(Math.log2(count))));

/** Right panel width (CSS `--panel-width`). The panel overlays the stage. */
export const PANEL_WIDTH = 420;

/** Clearance kept between the stage content and the panel: just enough that the
 *  outermost treemap tiles' / graph nodes' right border renders in visible
 *  canvas instead of tucking under the opaque panel, but small enough that it
 *  reads as the tile's own edge, not an empty gap. */
const PANEL_CLEARANCE = 2;

/** Width the canvas can actually use: the right panel overlays the stage. */
export const usableStageWidth = (state: AppState, stageW: number): number => {
  // A panel is always present now: a selection, a road/clone drill-down,
  // or the per-lens ranked list (overview included). Reserve its width plus a
  // few px so tiles render up to it, edge visible, but never under it.
  void state;
  return Math.max(PANEL_WIDTH, stageW - PANEL_WIDTH - PANEL_CLEARANCE);
};

/** Folder keys whose imports carry little overview signal (test suites). */
export const isTestCluster = (key: string): boolean =>
  /(^|\/)(tests?|__tests__|e2e|spec)($|\/)/.test(key);
// ── Rendering ───────────────────────────────────────────────────

export const easeOut = (progress: number): number => 1 - (1 - progress) * (1 - progress);
export const hullPath = (ctx: CanvasRenderingContext2D, hull: Pt[]): void => {
  const pointCount = hull.length;
  if (pointCount < 3) {
    ctx.moveTo(hull[0].x, hull[0].y);
    for (let index = 1; index < pointCount; index++) ctx.lineTo(hull[index].x, hull[index].y);
    ctx.closePath();
    return;
  }
  // Rounded corners: run each edge to its midpoint, then a quadratic through
  // the vertex to the next edge's midpoint. The curve stays inside the convex
  // hull, so a cluster reads as a smooth blob rather than a hard polygon.
  const start = {
    x: (hull[pointCount - 1].x + hull[0].x) / 2,
    y: (hull[pointCount - 1].y + hull[0].y) / 2,
  };
  ctx.moveTo(start.x, start.y);
  for (let index = 0; index < pointCount; index++) {
    const curr = hull[index];
    const next = hull[(index + 1) % pointCount];
    ctx.quadraticCurveTo(curr.x, curr.y, (curr.x + next.x) / 2, (curr.y + next.y) / 2);
  }
  ctx.closePath();
};

export const worldToScreen = (gvs: GraphViewState, point: Pt): Pt => ({
  x: point.x * gvs.transform.k + gvs.transform.x,
  y: point.y * gvs.transform.k + gvs.transform.y,
});

/**
 * Truncate a directory prefix from the front, keeping whole trailing
 * segments ("…/mdx-components/"), so the informative tail survives.
 */
export const tailTruncate = (
  ctx: CanvasRenderingContext2D,
  dir: string,
  maxWidth: number,
): string => {
  if (dir === "" || ctx.measureText(dir).width <= maxWidth) return dir;
  const tail = dir.endsWith("/") ? "/" : "";
  const parts = dir.split("/").filter((part) => part !== "");
  for (let drop = 1; drop < parts.length; drop++) {
    const candidate = `…/${parts.slice(drop).join("/")}${tail}`;
    if (ctx.measureText(candidate).width <= maxWidth) return candidate;
  }
  return ctx.measureText("…/").width <= maxWidth ? "…/" : "";
};

export const middleTruncate = (
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

export const nodeHitTest = (state: AppState, canvasX: number, canvasY: number): number | null => {
  const gvs = getGVS(state);
  const { transform, fileNodes, clusters, grid } = gvs;
  if (!grid) return null;
  const gx = (canvasX - transform.x) / transform.k;
  const gy = (canvasY - transform.y) / transform.k;
  // Nearest-wins with a 9px screen-space floor so dots stay clickable
  // at fit zoom.
  const floor = 9 / transform.k;
  // The effective hit radius depends on the current zoom (screen-space
  // floor and slop), so the grid query radius is computed per call in
  // world units.
  const maxWorldRadius = Math.max(grid.maxRadius + 3 / transform.k, floor);
  let best: number | null = null;
  let bestD = Infinity;
  for (const nodeIndex of gridQuery(grid, gx, gy, maxWorldRadius)) {
    const node = fileNodes[nodeIndex];
    if (!node || node.x == null || node.y == null) continue;
    if (clusters[node.cluster].isolated && !gvs.standaloneOpen) continue;
    const dx = gx - node.x;
    const dy = gy - node.y;
    const distSq = dx * dx + dy * dy;
    const radius = Math.max(node.radius + 3 / transform.k, floor);
    if (distSq <= radius * radius && distSq < bestD) {
      bestD = distSq;
      best = node.fileIndex;
    }
  }
  return best;
};

export const roadHitTest = (state: AppState, x: number, y: number): number | null => {
  const gvs = getGVS(state);
  const threshold = 10;
  const { transform } = gvs;
  const gx = (x - transform.x) / transform.k;
  const gy = (y - transform.y) / transform.k;
  const pad = threshold / transform.k;
  let best: number | null = null;
  let bestDist = threshold;
  for (let ri = 0; ri < gvs.roads.length; ri++) {
    const road = gvs.roads[ri];
    const { p0, p1, p2, p3 } = roadGeometry(gvs, road);
    // Coarse bounding-box prefilter over the bezier's control points (the
    // curve stays within their hull), so the 17-point sampling is skipped
    // only for roads the pointer truly cannot be on. An endpoint-circle
    // prefilter would leave the middle of long roads unhittable.
    const minX = Math.min(p0.x, p1.x, p2.x, p3.x);
    const maxX = Math.max(p0.x, p1.x, p2.x, p3.x);
    const minY = Math.min(p0.y, p1.y, p2.y, p3.y);
    const maxY = Math.max(p0.y, p1.y, p2.y, p3.y);
    if (gx < minX - pad || gx > maxX + pad || gy < minY - pad || gy > maxY + pad) continue;
    for (let sampleIndex = 0; sampleIndex <= 16; sampleIndex++) {
      const point = worldToScreen(gvs, cubicPoint(p0, p1, p2, p3, sampleIndex / 16));
      const dist = Math.hypot(point.x - x, point.y - y);
      if (dist < bestDist) {
        bestDist = dist;
        best = ri;
      }
    }
  }
  return best;
};

/** Bounding box of the clusters an include predicate keeps. */
export const clusterBounds = (
  clusters: ClusterInfo[],
  include: (cluster: ClusterInfo) => boolean,
): { minX: number; minY: number; maxX: number; maxY: number } => {
  let minX = Infinity;
  let minY = Infinity;
  let maxX = -Infinity;
  let maxY = -Infinity;
  for (const cluster of clusters) {
    if (!include(cluster)) continue;
    minX = Math.min(minX, cluster.cx - cluster.r);
    minY = Math.min(minY, cluster.cy - cluster.r);
    maxX = Math.max(maxX, cluster.cx + cluster.r);
    maxY = Math.max(maxY, cluster.cy + cluster.r);
  }
  return { minX, minY, maxX, maxY };
};

const FIT_PAD = 70;

/**
 * Fit-to-view camera transform for a cluster bounding box, reserving
 * horizontal room for labels that stick out of hulls.
 */
export const fitTransform = (
  width: number,
  height: number,
  bounds: { minX: number; minY: number; maxX: number; maxY: number },
): { x: number; y: number; k: number } => {
  // An empty include set yields infinite bounds; a NaN camera poisons
  // every later transform, so fall back to the identity view.
  if (!Number.isFinite(bounds.minX)) return { x: 0, y: 0, k: 1 };
  const bboxW = bounds.maxX - bounds.minX + FIT_PAD * 2;
  const bboxH = bounds.maxY - bounds.minY + FIT_PAD * 2;
  const scale = Math.min((width - 200) / bboxW, (height - 60) / bboxH, 1.4);
  return {
    x: (width - bboxW * scale) / 2 - bounds.minX * scale + FIT_PAD * scale,
    y: (height - bboxH * scale) / 2 - bounds.minY * scale + FIT_PAD * scale,
    k: scale,
  };
};

/** The stage's usable pixel size for the graph camera. */
export const stageSize = (state: AppState): { w: number; h: number } => {
  const stageEl = state.canvas.parentElement;
  return {
    w: usableStageWidth(state, stageEl ? stageEl.clientWidth : window.innerWidth),
    h: stageEl ? stageEl.clientHeight : window.innerHeight,
  };
};
