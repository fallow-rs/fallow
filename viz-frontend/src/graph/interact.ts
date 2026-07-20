/**
 * Pointer and camera interaction for the graph view: hit testing, hover
 * and click resolution, the shift-click dependency trace, and every way
 * the camera moves (fit, reset, center, search zoom, resize refit).
 */
import { select } from "d3-selection";
// oxlint-disable-next-line import/no-unassigned-import -- augments selections with .transition()
import "d3-transition";
import { zoomIdentity, type ZoomTransform } from "d3-zoom";
import type { AppState } from "../state";
import type { RoadSelection } from "../types";
import {
  type ClusterMode,
  type GraphClickResult,
  type GraphHoverTarget,
  type StageRect,
  clusterBounds,
  cubicPoint,
  easeOut,
  fitTransform,
  getGVS,
  markIntroSeen,
  nodeHitTest,
  roadGeometry,
  stageSize,
  worldToScreen,
} from "./shared";
import { renderGraph } from "./render";
import { initGraphNodes } from "./build";

export const getClusterMode = (state: AppState): ClusterMode => getGVS(state).clusterMode;

/** Camera glide duration for programmatic moves (0-key, search, center). */
const CAMERA_MS = 450;

/**
 * Apply a zoom transform, gliding there over `duration` ms unless
 * reduced-motion is on or the duration is 0. Direct manipulation (wheel,
 * drag) stays on the instant path; only programmatic jumps tween, so the
 * eye can follow where the camera went. A named transition means a second
 * call interrupts the first rather than fighting it.
 */
const tweenCamera = (state: AppState, target: ZoomTransform, duration: number): void => {
  const gvs = getGVS(state);
  if (!gvs.zoomBehavior) return;
  const sel = select(state.canvas);
  if (state.reducedMotion || duration <= 0) {
    sel.call(gvs.zoomBehavior.transform, target);
  } else {
    sel
      .transition("camera")
      .duration(duration)
      .ease(easeOut)
      .call(gvs.zoomBehavior.transform, target);
  }
};

export const setClusterMode = (state: AppState, mode: ClusterMode): void => {
  const gvs = getGVS(state);
  if (gvs.clusterMode === mode) return;
  gvs.clusterMode = mode;
  gvs.initialized = false;
  initGraphNodes(state);
};
/** Seed the graph node lens-color crossfade with the pre-switch colors. */
export const startGraphLensFade = (state: AppState, prev: Map<number, string>): void => {
  if (state.reducedMotion) return;
  const gvs = getGVS(state);
  gvs.lensPrev = prev;
  gvs.lensFadeAt = performance.now();
};

/** Node position in canvas pixels (for tooltip docking in main). */
export const nodeScreenPos = (state: AppState, idx: number): { x: number; y: number } | null => {
  const gvs = getGVS(state);
  const node = gvs.fileNodes[idx];
  if (!node || node.x == null || node.y == null) return null;
  return worldToScreen(gvs, { x: node.x, y: node.y });
};

/** Any real interaction ends the intro early. */
export const dismissIntro = (state: AppState): void => {
  const gvs = getGVS(state);
  if (!gvs.showIntro) return;
  gvs.showIntro = false;
  markIntroSeen();
};
// ── Hit testing / interaction ───────────────────────────────────

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
  const { transform } = gvs;
  const gx = (x - transform.x) / transform.k;
  const gy = (y - transform.y) / transform.k;
  const slop = 60 / transform.k;
  let best: number | null = null;
  let bestDist = threshold;
  for (let ri = 0; ri < gvs.roads.length; ri++) {
    const road = gvs.roads[ri];
    // Coarse circle prefilter: skip the 17-point sampling for roads
    // whose endpoint clusters are both far from the pointer.
    const src = gvs.clusters[road.src];
    const dst = gvs.clusters[road.dst];
    const nearSrc = Math.hypot(gx - src.cx, gy - src.cy) <= src.r + slop;
    const nearDst = Math.hypot(gx - dst.cx, gy - dst.cy) <= dst.r + slop;
    if (!nearSrc && !nearDst) continue;
    const { p0, p1, p2, p3 } = roadGeometry(gvs, road);
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
  const sortedPairs = pairs.toSorted(
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
    pairs: sortedPairs,
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
          return path.toReversed();
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
  return bfs(from, to, state.index.importsOf) ?? bfs(to, from, state.index.importsOf) ?? null;
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
  const had = gvs.selectedRoad !== null || gvs.path !== null || gvs.pathFrom !== null;
  gvs.selectedRoad = null;
  gvs.path = null;
  gvs.pathFrom = null;
  return had;
};

/** Re-fit after a window resize, but only while the camera is untouched. */
export const refitOnResize = (state: AppState): void => {
  const gvs = getGVS(state);
  if (!gvs.initialized || gvs.userMoved || state.selected !== null) return;
  // Keep-fitted, not a user gesture: snap instantly so a resize drag or a
  // panel open/close does not animate the camera every frame.
  resetGraphView(state, false);
};

/** Reset the camera to the fit-to-view transform (0 key / after wandering). */
export const resetGraphView = (state: AppState, animate = true): void => {
  const gvs = getGVS(state);
  if (!gvs.initialized || !gvs.zoomBehavior || state.selected !== null) return;
  // Recompute the fit transform from current cluster bounds.
  const { w, h } = stageSize(state);
  const anyConnected = gvs.clusters.some((c) => !c.isolated);
  const fit = fitTransform(
    w,
    h,
    clusterBounds(gvs.clusters, (c) => !(c.isolated && anyConnected && !gvs.standaloneOpen)),
  );
  // Re-anchor the zoom extent and LOD baseline to the refreshed fit,
  // so min/max zoom stay meaningful after large resizes.
  gvs.zoomBehavior.scaleExtent([fit.k * 0.4, fit.k * 12]);
  gvs.fitK = fit.k;
  tweenCamera(state, zoomIdentity.translate(fit.x, fit.y).scale(fit.k), animate ? CAMERA_MS : 0);
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
  // The camera glide is the reduced-motion feedback; skip the pulse rings.
  if (!state.reducedMotion) {
    gvs.pulseFile = best;
    gvs.pulseAt = performance.now();
  }
  renderGraph(state);
};

/** What the cursor is over (drives hover state, cursor, and tooltip). */
export const graphHoverTarget = (state: AppState, x: number, y: number): GraphHoverTarget => {
  const gvs = getGVS(state);
  if (state.selected !== null) {
    gvs.hoveredRoad = null;
    gvs.hoveredCluster = null;
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
    gvs.hoveredCluster = null;
    return { kind: "ui" };
  }
  const node = nodeHitTest(state, x, y);
  if (node !== null) {
    gvs.hoveredRoad = null;
    gvs.hoveredCluster = null;
    return { kind: "file", fileIndex: node };
  }
  // A cluster label lights up every road into/out of that cluster.
  const label = gvs.clusterLabels.find(
    (c) => x >= c.x && x <= c.x + c.w && y >= c.y && y <= c.y + c.h,
  );
  if (label) {
    gvs.hoveredRoad = null;
    gvs.hoveredCluster = label.cluster;
    return { kind: "cluster", cluster: label.cluster };
  }
  gvs.hoveredCluster = null;
  const road = roadHitTest(state, x, y);
  gvs.hoveredRoad = road;
  return road !== null ? { kind: "road", road } : null;
};

/** Drop road hover when the cursor leaves the canvas; true if state changed. */
export const clearRoadHover = (state: AppState): boolean => {
  const gvs = getGVS(state);
  if (gvs.hoveredRoad === null && gvs.hoveredCluster === null) return false;
  gvs.hoveredRoad = null;
  gvs.hoveredCluster = null;
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
  tweenCamera(state, target, CAMERA_MS);
};
