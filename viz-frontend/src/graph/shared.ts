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
  for (let i = 0; i < nodes.length; i++) {
    const node = nodes[i];
    if (!node || node.x == null || node.y == null) continue;
    const cx = Math.min(cols - 1, Math.max(0, Math.floor((node.x - minX) / cell)));
    const cy = Math.min(rows - 1, Math.max(0, Math.floor((node.y - minY) / cell)));
    buckets[cy * cols + cx].push(i);
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
      for (const idx of grid.buckets[cy * grid.cols + cx]) out.push(idx);
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

export const FONT_SMALL = '10px "Martian Mono", "JetBrains Mono", ui-monospace, Menlo, monospace';
export const FONT_MICRO = '9px "Martian Mono", "JetBrains Mono", ui-monospace, Menlo, monospace';
export const FONT_CHIP = '11px "Martian Mono", "JetBrains Mono", ui-monospace, Menlo, monospace';
export const FONT_LEGEND = '10px "Martian Mono", "JetBrains Mono", ui-monospace, Menlo, monospace';
export const FONT_CARD = '700 13px "Martian Mono", "JetBrains Mono", ui-monospace, Menlo, monospace';

export const NODE_R_MIN = 2.5;
export const NODE_R_MAX = 10;
export const MAX_CLUSTERS = 40;
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

export const cubicPoint = (p0: Pt, p1: Pt, p2: Pt, p3: Pt, t: number): Pt => {
  const u = 1 - t;
  return {
    x: u * u * u * p0.x + 3 * u * u * t * p1.x + 3 * u * t * t * p2.x + t * t * t * p3.x,
    y: u * u * u * p0.y + 3 * u * u * t * p1.y + 3 * u * t * t * p2.y + t * t * t * p3.y,
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

export const roadGeometry = (gvs: GraphViewState, road: Road): { p0: Pt; p1: Pt; p2: Pt; p3: Pt } => {
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
export const chipRect = (
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

export const roadWidth = (count: number): number =>
  Math.min(8, Math.max(1.5, 1 + Math.floor(Math.log2(count))));

/** Width the canvas can actually use: the right panel overlays the stage. */
export const usableStageWidth = (state: AppState, stageW: number): number => {
  const panelOpen =
    state.selected !== null ||
    state.selectedRoad !== null ||
    state.selectedClone !== null ||
    state.lens !== "overview";
  return panelOpen ? Math.max(420, stageW - 380) : stageW;
};

/** Folder keys whose imports carry little overview signal (test suites). */
export const isTestCluster = (key: string): boolean =>
  /(^|\/)(tests?|__tests__|e2e|spec)($|\/)/.test(key);
// ── Rendering ───────────────────────────────────────────────────

export const easeOut = (t: number): number => 1 - (1 - t) * (1 - t);
export const hullPath = (ctx: CanvasRenderingContext2D, hull: Pt[]): void => {
  ctx.moveTo(hull[0].x, hull[0].y);
  for (let i = 1; i < hull.length; i++) ctx.lineTo(hull[i].x, hull[i].y);
  ctx.closePath();
};

export const worldToScreen = (gvs: GraphViewState, p: Pt): Pt => ({
  x: p.x * gvs.transform.k + gvs.transform.x,
  y: p.y * gvs.transform.k + gvs.transform.y,
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
  const parts = dir.split("/").filter((p) => p !== "");
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
  for (const idx of gridQuery(grid, gx, gy, maxWorldRadius)) {
    const node = fileNodes[idx];
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

/** Bounding box of the clusters an include predicate keeps. */
export const clusterBounds = (
  clusters: ClusterInfo[],
  include: (c: ClusterInfo) => boolean,
): { minX: number; minY: number; maxX: number; maxY: number } => {
  let minX = Infinity;
  let minY = Infinity;
  let maxX = -Infinity;
  let maxY = -Infinity;
  for (const c of clusters) {
    if (!include(c)) continue;
    minX = Math.min(minX, c.cx - c.r);
    minY = Math.min(minY, c.cy - c.r);
    maxX = Math.max(maxX, c.cx + c.r);
    maxY = Math.max(maxY, c.cy + c.r);
  }
  return { minX, minY, maxX, maxY };
};

const FIT_PAD = 70;

/**
 * Fit-to-view camera transform for a cluster bounding box, reserving
 * horizontal room for labels that stick out of hulls.
 */
export const fitTransform = (
  w: number,
  h: number,
  b: { minX: number; minY: number; maxX: number; maxY: number },
): { x: number; y: number; k: number } => {
  // An empty include set yields infinite bounds; a NaN camera poisons
  // every later transform, so fall back to the identity view.
  if (!Number.isFinite(b.minX)) return { x: 0, y: 0, k: 1 };
  const bboxW = b.maxX - b.minX + FIT_PAD * 2;
  const bboxH = b.maxY - b.minY + FIT_PAD * 2;
  const k = Math.min((w - 200) / bboxW, (h - 60) / bboxH, 1.4);
  return {
    x: (w - bboxW * k) / 2 - b.minX * k + FIT_PAD * k,
    y: (h - bboxH * k) / 2 - b.minY * k + FIT_PAD * k,
    k,
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
