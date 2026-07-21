/**
 * Minimap overlay for the graph overview: the corner navigator that shows
 * cluster footprints and the current viewport, plus its click-to-pan. Only
 * earns pixels once the camera has left the fit view.
 */
import { select } from "d3-selection";
// oxlint-disable-next-line import/no-unassigned-import -- augments selections with .transition()
import "d3-transition";
import { zoomIdentity } from "d3-zoom";
import type { AppState } from "../state";
import { type GraphViewState, type Pt, PANEL_WIDTH, clusterBounds, easeOut, getGVS } from "./shared";

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

const minimapFrameAt = (
  state: AppState,
  gvs: GraphViewState,
  width: number,
  height: number,
): MinimapFrame | null => {
  if (gvs.clusters.length < 2) return null;
  // Keep clear of the detail panel when a road drill-down is open.
  const panelW = state.selectedRoad !== null ? Math.min(PANEL_WIDTH, width * 0.9) : 0;
  const { minX, minY, maxX, maxY } = clusterBounds(gvs.clusters, () => true);
  const worldW = Math.max(1, maxX - minX);
  const worldH = Math.max(1, maxY - minY);
  const scale = Math.min((MINIMAP_W - 12) / worldW, (MINIMAP_H - 12) / worldH);
  return {
    x: width - panelW - MINIMAP_W - MINIMAP_MARGIN,
    y: height - MINIMAP_H - MINIMAP_MARGIN - 24,
    w: MINIMAP_W,
    h: MINIMAP_H,
    scale,
    worldX: minX - (MINIMAP_W / scale - worldW) / 2,
    worldY: minY - (MINIMAP_H / scale - worldH) / 2,
  };
};

/** The minimap frame at the stage's current size. */
const minimapFrame = (state: AppState, gvs: GraphViewState): MinimapFrame | null => {
  const stageEl = state.canvas.parentElement;
  const width = stageEl ? stageEl.clientWidth : window.innerWidth;
  const height = stageEl ? stageEl.clientHeight : window.innerHeight;
  return minimapFrameAt(state, gvs, width, height);
};

export const drawMinimap = (
  state: AppState,
  gvs: GraphViewState,
  width: number,
  height: number,
): void => {
  const { ctx, theme } = state;
  // Only earns pixels once the camera left the fit view.
  if (gvs.transform.k / gvs.fitK < 1.08) return;
  const frame = minimapFrameAt(state, gvs, width, height);
  if (!frame) return;

  ctx.fillStyle = theme.surface1;
  ctx.globalAlpha = 0.92;
  ctx.fillRect(frame.x, frame.y, frame.w, frame.h);
  ctx.globalAlpha = 1;
  ctx.strokeStyle = theme.borderDefault;
  ctx.lineWidth = 1;
  ctx.strokeRect(frame.x + 0.5, frame.y + 0.5, frame.w - 1, frame.h - 1);

  const toMini = (point: Pt): Pt => ({
    x: frame.x + (point.x - frame.worldX) * frame.scale,
    y: frame.y + (point.y - frame.worldY) * frame.scale,
  });

  // Cluster footprints, tangles in amber.
  for (const cluster of gvs.clusters) {
    const center = toMini({ x: cluster.cx, y: cluster.cy });
    const radius = Math.max(1.5, cluster.r * frame.scale);
    ctx.beginPath();
    ctx.arc(center.x, center.y, radius, 0, Math.PI * 2);
    ctx.fillStyle = cluster.tangle ? theme.amber : theme.borderStrong;
    ctx.globalAlpha = cluster.tangle ? 0.55 : 0.4;
    ctx.fill();
  }
  ctx.globalAlpha = 1;

  // Viewport rectangle (inverse of the active transform).
  const { transform } = gvs;
  const topLeft = toMini({ x: -transform.x / transform.k, y: -transform.y / transform.k });
  const bottomRight = toMini({
    x: (width - transform.x) / transform.k,
    y: (height - transform.y) / transform.k,
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
  const frame = minimapFrame(state, gvs);
  if (!frame) return false;
  return x >= frame.x && x <= frame.x + frame.w && y >= frame.y && y <= frame.y + frame.h;
};

/** Center the camera on the clicked minimap position. */
export const minimapPan = (state: AppState, x: number, y: number): void => {
  const gvs = getGVS(state);
  if (!gvs.initialized || !gvs.zoomBehavior) return;
  const frame = minimapFrame(state, gvs);
  if (!frame) return;
  const stageEl = state.canvas.parentElement;
  const width = stageEl ? stageEl.clientWidth : window.innerWidth;
  const height = stageEl ? stageEl.clientHeight : window.innerHeight;
  const worldX = frame.worldX + (x - frame.x) / frame.scale;
  const worldY = frame.worldY + (y - frame.y) / frame.scale;
  const zoomScale = gvs.transform.k;
  const target = zoomIdentity
    .translate(width / 2 - worldX * zoomScale, height / 2 - worldY * zoomScale)
    .scale(zoomScale);
  const sel = select(state.canvas);
  // Glide the recentre so a minimap click reads as a pan, not a jump.
  if (state.reducedMotion) {
    sel.call(gvs.zoomBehavior.transform, target);
  } else {
    sel.transition("camera").duration(250).ease(easeOut).call(gvs.zoomBehavior.transform, target);
  }
};
