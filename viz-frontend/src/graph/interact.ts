/**
 * Pointer and camera interaction for the graph view: hit testing, hover
 * and click resolution, the shift-click dependency trace, and every way
 * the camera moves (fit, reset, center, search zoom, resize refit).
 */
import { select } from "d3-selection";
import { zoomIdentity } from "d3-zoom";
import type { AppState } from "../state";
import type { RoadSelection } from "../types";
import {
  type ClusterMode,
  type GraphClickResult,
  type GraphHoverTarget,
  type StageRect,
  cubicPoint,
  getGVS,
  nodeHitTest,
  markIntroSeen,
  roadGeometry,
  usableStageWidth,
  worldToScreen,
} from "./shared";
import { renderGraph } from "./render";
import { initGraphNodes } from "./build";

export const getClusterMode = (state: AppState): ClusterMode => getGVS(state).clusterMode;

export const setClusterMode = (state: AppState, mode: ClusterMode): void => {
  const gvs = getGVS(state);
  if (gvs.clusterMode === mode) return;
  gvs.clusterMode = mode;
  gvs.initialized = false;
  initGraphNodes(state);
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
  const w = usableStageWidth(state, stageEl ? stageEl.clientWidth : window.innerWidth);
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
