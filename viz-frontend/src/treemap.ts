import type { AppState } from "./state";
import type { LayoutCell, TreeNode } from "./types";
import { DISPLAY_FACE, TEXT_FACE, contrastText, mix } from "./theme";
import { formatCount, legendText, lensColor, lensFindingLevel } from "./data";
import { usableStageWidth } from "./graph";

// ── Constants ───────────────────────────────────────────────────

const DIR_HEADER = 18;
const DIR_PAD = 3;
const MIN_LABEL_W = 40;
const MIN_LABEL_H = 13;
const FONT_CELL = `600 13px ${DISPLAY_FACE}`;
const FONT_LEGEND = `13px ${TEXT_FACE}`;
const FONT_DIR = `600 13px ${DISPLAY_FACE}`;
const ZOOM_MS = 480;
const LENS_MS = 640;
/** Reduced motion keeps the color change legible but drops the sweep. */
const LENS_REDUCED_MS = 180;
/** Share of the lens fade spent staggering the sweep from left to right. */
const LENS_SWEEP = 0.55;
const REVEAL_MS = 560;
/** Footer gutter under the tiles that holds the legend chips. */
const FOOTER_H = 22;

interface Rect {
  x: number;
  y: number;
  w: number;
  h: number;
}

// ── Squarify ────────────────────────────────────────────────────

const squarify = (nodes: TreeNode[], rect: Rect): LayoutCell[] => {
  const total = nodes.reduce((sum, node) => sum + node.size, 0);
  const result: LayoutCell[] = [];
  if (total > 0) layoutStrip(nodes, rect, total, result);
  return result;
};

const layoutStrip = (
  nodes: TreeNode[],
  rect: Rect,
  totalSize: number,
  result: LayoutCell[],
): void => {
  if (nodes.length === 0 || totalSize === 0) return;
  if (nodes.length === 1) {
    result.push({ ...rect, node: nodes[0], depth: 0 });
    return;
  }

  const isWide = rect.w >= rect.h;
  const side = isWide ? rect.h : rect.w;

  let row: TreeNode[] = [];
  let rowSize = 0;
  let bestAspect = Infinity;
  let index = 0;

  while (index < nodes.length) {
    const testSize = rowSize + nodes[index].size;
    const testAspect = worstAspect(row.concat(nodes[index]), testSize, side, totalSize, rect);
    if (testAspect <= bestAspect || row.length === 0) {
      row.push(nodes[index]);
      rowSize = testSize;
      bestAspect = testAspect;
      index++;
    } else {
      break;
    }
  }

  const rowFraction = rowSize / totalSize;
  const rowRect: Rect = isWide
    ? { x: rect.x, y: rect.y, w: rect.w * rowFraction, h: rect.h }
    : { x: rect.x, y: rect.y, w: rect.w, h: rect.h * rowFraction };

  let offset = 0;
  for (const node of row) {
    const fraction = node.size / rowSize;
    if (isWide) {
      const height = rowRect.h * fraction;
      result.push({ x: rowRect.x, y: rowRect.y + offset, w: rowRect.w, h: height, node, depth: 0 });
      offset += height;
    } else {
      const width = rowRect.w * fraction;
      result.push({ x: rowRect.x + offset, y: rowRect.y, w: width, h: rowRect.h, node, depth: 0 });
      offset += width;
    }
  }

  const remaining = nodes.slice(index);
  if (remaining.length > 0) {
    const remainRect: Rect = isWide
      ? { x: rect.x + rowRect.w, y: rect.y, w: rect.w - rowRect.w, h: rect.h }
      : { x: rect.x, y: rect.y + rowRect.h, w: rect.w, h: rect.h - rowRect.h };
    layoutStrip(remaining, remainRect, totalSize - rowSize, result);
  }
};

const worstAspect = (
  row: TreeNode[],
  rowSize: number,
  side: number,
  totalSize: number,
  rect: Rect,
): number => {
  const isWide = rect.w >= rect.h;
  const rowLength = isWide ? (rowSize / totalSize) * rect.w : (rowSize / totalSize) * rect.h;
  if (rowLength === 0) return Infinity;

  let worst = 0;
  for (const node of row) {
    const nodeLength = side * (node.size / rowSize);
    const aspect = Math.max(rowLength / nodeLength, nodeLength / rowLength);
    if (aspect > worst) worst = aspect;
  }
  return worst;
};

// ── Animation state (module-local) ──────────────────────────────

interface TreemapAnim {
  kind: "zoom-in" | "zoom-out" | "lens" | "reveal";
  start: number;
  /**
   * Zoom: where the child directory sits inside the parent layout. A
   * zoom-in flies the camera from the parent into this rect; a zoom-out
   * flies it back out.
   */
  rect?: Rect;
  /** Zoom: a bitmap of the outgoing view, so both views share one camera. */
  snapshot?: HTMLCanvasElement;
  /** Lens crossfade: previous fill colors keyed by file index. */
  prevColors?: Map<number, string>;
}

interface TreemapState {
  anim: TreemapAnim | null;
  hatch: CanvasPattern | null;
  /** Lighter hatch variant for mild (level 1) findings. */
  hatchMild: CanvasPattern | null;
  hatchKey: string;
  raf: number;
  revealed: boolean;
  /** Geometry key the cached `state.layout` was squarified for ("" = stale). */
  layoutKey: string;
  /** Transparent layer over the map that holds only the hover marks. */
  hoverCanvas: HTMLCanvasElement | null;
  hoverCtx: CanvasRenderingContext2D | null;
  /** Reused backing store for the zoom snapshot. */
  snapshot: HTMLCanvasElement | null;
}

const getTM = (state: AppState): TreemapState => {
  const ext = state as AppState & { _tm?: TreemapState };
  if (!ext._tm) {
    ext._tm = {
      anim: null,
      hatch: null,
      hatchMild: null,
      hatchKey: "",
      raf: 0,
      revealed: false,
      layoutKey: "",
      hoverCanvas: null,
      hoverCtx: null,
      snapshot: null,
    };
  }
  return ext._tm;
};

/**
 * Geometry inputs of the treemap layout. While this key is unchanged and
 * no animation is in flight, the cached `state.layout` cells repaint
 * as-is: pure hover repaints skip the squarify recursion entirely.
 */
export const treemapLayoutKey = (
  drillPath: string,
  width: number,
  height: number,
  usableW: number,
  dpr: number,
): string => [drillPath, width, height, usableW, dpr].join("|");

/** Quartic ease-out: a fast, confident start that settles without bounce. */
const easeOut = (progress: number): number => 1 - (1 - progress) ** 4;

/** Cubic ease-out for the drill camera: gentler than quartic, so the eye
 *  can follow the flight instead of seeing it land almost at once. */
const easeCamera = (progress: number): number => 1 - (1 - progress) ** 3;

const clamp01 = (value: number): number => Math.min(1, Math.max(0, value));

/** Hermite ramp of `value` between two edges, 0 below and 1 above. */
const smoothstep = (from: number, to: number, value: number): number => {
  const local = clamp01((value - from) / (to - from));
  return local * local * (3 - 2 * local);
};

/** Copy the visible map into a reusable bitmap before the view changes. */
const snapshotCanvas = (state: AppState): HTMLCanvasElement | undefined => {
  const tm = getTM(state);
  const { canvas } = state;
  if (canvas.width === 0 || canvas.height === 0) return undefined;
  const snap = tm.snapshot ?? document.createElement("canvas");
  tm.snapshot = snap;
  if (snap.width !== canvas.width) snap.width = canvas.width;
  if (snap.height !== canvas.height) snap.height = canvas.height;
  const sctx = snap.getContext("2d");
  if (!sctx) return undefined;
  sctx.setTransform(1, 0, 0, 1, 0, 0);
  sctx.clearRect(0, 0, snap.width, snap.height);
  sctx.drawImage(canvas, 0, 0);
  return snap;
};

/**
 * Kick a camera zoom. `rect` is the child directory's rect in the parent
 * layout: the clicked cell on the way in, the located cell on the way out.
 */
const startZoom = (state: AppState, rect: Rect | null, dir: "in" | "out"): void => {
  if (state.reducedMotion || state.view !== "map") return;
  const tm = getTM(state);
  if (!rect || rect.w < 1 || rect.h < 1) {
    tm.anim = null;
    return;
  }
  const snapshot = snapshotCanvas(state);
  if (!snapshot) return;
  tm.anim = {
    kind: dir === "in" ? "zoom-in" : "zoom-out",
    start: performance.now(),
    rect,
    snapshot,
  };
};

/**
 * Kick a lens crossfade from the current cell colors. The new colors
 * sweep across the map from left to right; reduced motion keeps a short
 * uniform fade, so the change still reads as a change.
 */
export const startLensFade = (state: AppState, prevColors: Map<number, string>): void => {
  const tm = getTM(state);
  tm.anim = { kind: "lens", start: performance.now(), prevColors };
};

/** Capture the current lens colors of all files (for the crossfade). */
export const captureLensColors = (state: AppState): Map<number, string> => {
  const colors = new Map<number, string>();
  for (let index = 0; index < state.data.files.length; index++) {
    colors.set(index, lensColor(state.lens, state.theme, state.index, state.data.files[index]));
  }
  return colors;
};

// ── Hatch texture (secondary encoding for findings) ─────────────

const buildHatch = (color: string, alpha = 0.5): CanvasPattern | null => {
  const canvas = document.createElement("canvas");
  canvas.width = 6;
  canvas.height = 6;
  const pctx = canvas.getContext("2d");
  if (!pctx) return null;
  pctx.strokeStyle = color;
  pctx.globalAlpha = alpha;
  pctx.lineWidth = 1;
  pctx.beginPath();
  pctx.moveTo(-1, 5);
  pctx.lineTo(7, -3);
  pctx.moveTo(-1, 11);
  pctx.lineTo(7, 3);
  pctx.stroke();
  const ctx2 = document.createElement("canvas").getContext("2d");
  return ctx2 ? ctx2.createPattern(canvas, "repeat") : null;
};

// ── Rendering ───────────────────────────────────────────────────

interface RenderCtx {
  state: AppState;
  now: number;
  /** 0..1 linear lens crossfade progress (1 = no fade active). */
  lensT: number;
  /** Whether the lens fade sweeps across the map or fades uniformly. */
  lensSweep: boolean;
  prevColors: Map<number, string> | null;
  /** 0..1 reveal progress (1 = fully revealed). */
  revealT: number;
  /** Opacity of the whole layer being painted (zoom crossfade). */
  layerAlpha: number;
  /** Stage rect the sweep and reveal positions are measured against. */
  bounds: Rect;
  hitTest: boolean;
  labels: boolean;
}

/** A backed footer chip: a translucent bg rect behind muted legend text. */
const footerChip = (
  ctx: CanvasRenderingContext2D,
  theme: AppState["theme"],
  text: string,
  align: "left" | "right",
  edgeX: number,
  y: number,
): void => {
  ctx.font = FONT_LEGEND;
  ctx.textAlign = align;
  ctx.textBaseline = "middle";
  const tw = ctx.measureText(text).width;
  const boxX = align === "left" ? edgeX - 6 : edgeX - tw - 6;
  ctx.fillStyle = theme.bg;
  ctx.globalAlpha = 0.85;
  ctx.fillRect(boxX, y - 9, tw + 12, 18);
  ctx.globalAlpha = 0.8;
  ctx.fillStyle = theme.textMuted;
  ctx.fillText(text, edgeX, y);
  ctx.globalAlpha = 1;
};

/** Footer strip: lens legend on the left, and, at the root, the drill hint. */
const drawTreemapFooter = (state: AppState, width: number, height: number): void => {
  const { ctx, theme } = state;
  const y = height - 17;
  const usableW = usableStageWidth(state, width);
  const hint = "Click a folder to zoom in";
  ctx.font = FONT_LEGEND;
  const hintW = ctx.measureText(hint).width + 32;
  // The legend gets the room left of the hint; on a narrow stage the hint
  // gives way and the legend ellipsizes instead of running under it.
  const showHint = state.drillPath === "" && usableW > 640;
  const legendRoom = usableW - 32 - (showHint ? hintW : 0);
  let legend = legendText(state.lens, state.data, "map");
  while (legend.length > 1 && ctx.measureText(legend).width > legendRoom) {
    legend = `${legend.slice(0, -2)}…`;
  }
  if (legend !== "") footerChip(ctx, theme, legend, "left", 16, y);
  // The treemap's one non-obvious gesture is drilling; teach it at the
  // root (once drilled, the breadcrumb already shows how to navigate).
  if (showHint) {
    footerChip(ctx, theme, hint, "right", usableW - 16, y);
  }
};

/** The stage size and the rect the tiles fill (clear of the footer and panel). */
const stageGeometry = (state: AppState): { width: number; height: number; root: Rect } => {
  const stage = state.canvas.parentElement;
  const width = stage ? stage.clientWidth : window.innerWidth;
  const height = stage ? stage.clientHeight : window.innerHeight;
  return {
    width,
    height,
    root: { x: 0, y: 0, w: usableStageWidth(state, width), h: height - FOOTER_H },
  };
};

/**
 * Camera transform that shows `view` (a rect in layout space) across
 * `frame`, as [scaleX, scaleY, translateX, translateY].
 */
const cameraOnto = (view: Rect, frame: Rect): [number, number, number, number] => {
  const sx = frame.w / view.w;
  const sy = frame.h / view.h;
  return [sx, sy, frame.x - view.x * sx, frame.y - view.y * sy];
};

/**
 * The camera's view at zoom progress `t`, from the whole `frame` (t = 0)
 * down to `target` (t = 1). Size moves geometrically so the zoom speed
 * feels constant, and the position follows the size, so `target` grows
 * from its own spot instead of sliding across the stage.
 */
const cameraView = (frame: Rect, target: Rect, t: number): Rect => {
  const w = frame.w * (target.w / frame.w) ** t;
  const h = frame.h * (target.h / frame.h) ** t;
  const along = (
    from: number,
    to: number,
    size: number,
    toSize: number,
    fromSize: number,
  ): number =>
    Math.abs(fromSize - toSize) < 0.5
      ? from + (to - from) * t
      : from + ((to - from) * (fromSize - size)) / (fromSize - toSize);
  return {
    x: along(frame.x, target.x, w, target.w, frame.w),
    y: along(frame.y, target.y, h, target.h, frame.h),
    w,
    h,
  };
};

/** Paint the tiles of `rootNode` into `rootRect`, or repaint the cached layout. */
const paintTiles = (
  rctx: RenderCtx,
  rootNode: TreeNode,
  rootRect: Rect,
  layoutKey: string,
): void => {
  const { state } = rctx;
  const tm = getTM(state);
  // Layout cache: geometry only changes with drill, stage size, panel
  // state, or DPR. On a paint-only render (hover, selection ring) the
  // cached cells repaint without re-running squarify. Any in-flight
  // animation bypasses the cache; the reveal populates `state.layout`
  // incrementally and a zoom repaints scaled geometry.
  if (tm.anim === null && tm.layoutKey === layoutKey && state.layout.length > 0) {
    repaintFromLayout(rctx);
    return;
  }
  state.layout = [];
  const cells = squarify(rootNode.children, insetRect(rootRect, 1));
  const total = cells.length;
  let cellSeq = 0;
  for (const cell of cells) {
    cellSeq = renderCell(rctx, cell, 0, cellSeq, total);
  }
  // Only an animation-free hit-test frame produces a complete layout
  // list; every other frame leaves the cache stale.
  tm.layoutKey = rctx.hitTest && tm.anim === null ? layoutKey : "";
};

export const renderTreemap = (state: AppState): void => {
  const { canvas, ctx } = state;
  // Re-read per render: the window can move to a display with a
  // different pixel ratio mid-session. Keep state.dpr in sync for
  // any consumer that sizes against the backing store.
  const dpr = window.devicePixelRatio || 1;
  state.dpr = dpr;
  const tm = getTM(state);
  const { width, height, root: rootRect } = stageGeometry(state);

  if (canvas.width !== Math.round(width * dpr) || canvas.height !== Math.round(height * dpr)) {
    canvas.style.width = `${width}px`;
    canvas.style.height = `${height}px`;
    canvas.width = Math.round(width * dpr);
    canvas.height = Math.round(height * dpr);
  }

  // First render: start the staggered reveal.
  if (!tm.revealed) {
    tm.revealed = true;
    if (!state.reducedMotion) {
      tm.anim = { kind: "reveal", start: performance.now() };
    }
  }

  if (tm.hatchKey !== state.theme.red) {
    tm.hatch = buildHatch(state.theme.textHigh, 0.34);
    tm.hatchMild = buildHatch(state.theme.textHigh, 0.16);
    tm.hatchKey = state.theme.red;
  }

  const now = performance.now();
  const anim = tm.anim;
  let animT = 1;
  if (anim) {
    const lensMs = state.reducedMotion ? LENS_REDUCED_MS : LENS_MS;
    const dur = anim.kind === "lens" ? lensMs : anim.kind === "reveal" ? REVEAL_MS : ZOOM_MS;
    animT = Math.min(1, (now - anim.start) / dur);
    if (animT >= 1) tm.anim = null;
  }

  ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
  ctx.globalAlpha = 1;
  ctx.fillStyle = state.theme.bg;
  ctx.fillRect(0, 0, width, height);

  const rootNode = state.index.nodesByPath.get(state.drillPath) ?? state.index.tree;
  const zoom =
    anim && (anim.kind === "zoom-in" || anim.kind === "zoom-out") && animT < 1 ? anim : null;

  const rctx: RenderCtx = {
    state,
    now,
    lensT: anim?.kind === "lens" ? animT : 1,
    lensSweep: !state.reducedMotion,
    prevColors: anim?.kind === "lens" ? (anim.prevColors ?? null) : null,
    revealT: anim?.kind === "reveal" ? animT : 1,
    layerAlpha: 1,
    bounds: rootRect,
    hitTest: zoom === null,
    labels: true,
  };
  const layoutKey = treemapLayoutKey(state.drillPath, width, height, rootRect.w, dpr);

  if (zoom?.rect && zoom.snapshot) {
    paintZoomFrame(rctx, zoom, easeCamera(animT), rootNode, rootRect, width, height);
  } else {
    paintTiles(rctx, rootNode, rootRect, layoutKey);
    paintSelectionMarker(state, rootRect);
  }

  ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
  ctx.globalAlpha = zoom ? smoothstep(0.5, 1, easeCamera(animT)) : 1;
  drawTreemapFooter(state, width, height);
  ctx.globalAlpha = 1;
  paintTreemapHover(state);

  if (tm.anim !== null) scheduleFrame(state);
};

/**
 * Make the selected file findable. A small file is a sliver of a tile, so
 * the rest of the map dims, the tile gets a bright ring, and a name tag
 * points at it.
 */
const paintSelectionMarker = (state: AppState, bounds: Rect): void => {
  if (state.selected === null) return;
  const cell = state.layout.find((entry) => entry.node.fileIndex === state.selected);
  if (!cell) return;
  const { ctx, theme } = state;
  ctx.save();
  ctx.beginPath();
  ctx.rect(bounds.x, bounds.y, bounds.w, bounds.h);
  ctx.rect(cell.x + cell.w, cell.y, -cell.w, cell.h);
  ctx.fillStyle = theme.bg;
  ctx.globalAlpha = 0.5;
  ctx.fill("evenodd");
  ctx.globalAlpha = 1;
  ctx.strokeStyle = theme.textHigh;
  ctx.lineWidth = 2;
  ctx.strokeRect(cell.x - 1, cell.y - 1, cell.w + 2, cell.h + 2);
  ctx.font = FONT_CELL;
  ctx.textBaseline = "middle";
  ctx.textAlign = "left";
  const name = cell.node.name;
  const tagW = ctx.measureText(name).width + 14;
  const tagH = 20;
  const above = cell.y - tagH - 6 >= bounds.y;
  const tagY = above ? cell.y - tagH - 6 : cell.y + cell.h + 6;
  const tagX = Math.min(
    Math.max(bounds.x + 4, cell.x + cell.w / 2 - tagW / 2),
    bounds.x + bounds.w - tagW - 4,
  );
  ctx.fillStyle = theme.textHigh;
  ctx.beginPath();
  ctx.roundRect(tagX, tagY, tagW, tagH, 4);
  ctx.fill();
  ctx.fillStyle = theme.bg;
  ctx.fillText(name, tagX + 7, tagY + tagH / 2 + 0.5);
  ctx.restore();
};

/**
 * One frame of the drill camera. The parent layout and the child layout
 * share a single camera: drilling in flies from the whole parent into the
 * child's rect while the child's own tiles resolve inside it; drilling out
 * runs the same flight backwards. The outgoing view is a bitmap, so the
 * flight costs one image draw plus the incoming tiles.
 */
const paintZoomFrame = (
  rctx: RenderCtx,
  zoom: TreemapAnim,
  progress: number,
  rootNode: TreeNode,
  rootRect: Rect,
  width: number,
  height: number,
): void => {
  const { state } = rctx;
  const { ctx, dpr } = state;
  const rect = zoom.rect as Rect;
  const snapshot = zoom.snapshot as HTMLCanvasElement;
  const zoomIn = zoom.kind === "zoom-in";
  // Camera position in parent-layout space: 0 = whole parent, 1 = child rect.
  const depth = zoomIn ? progress : 1 - progress;
  const [sx, sy, tx, ty] = cameraOnto(cameraView(rootRect, rect, depth), rootRect);
  // The child layout fills rootRect in its own space; this maps it onto
  // its rect inside the parent's space.
  const childSx = rect.w / rootRect.w;
  const childSy = rect.h / rootRect.h;
  const childTx = rect.x - rootRect.x * childSx;
  const childTy = rect.y - rootRect.y * childSy;

  const applyCamera = (intoChild: boolean): void => {
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.transform(sx, 0, 0, sy, tx, ty);
    if (intoChild) ctx.transform(childSx, 0, 0, childSy, childTx, childTy);
  };
  const paintSnapshot = (alpha: number, intoChild: boolean): void => {
    if (alpha <= 0.01) return;
    applyCamera(intoChild);
    ctx.globalAlpha = alpha;
    ctx.imageSmoothingQuality = "high";
    ctx.drawImage(snapshot, 0, 0, width, height);
    ctx.globalAlpha = 1;
  };
  const paintLive = (alpha: number, intoChild: boolean): void => {
    if (alpha <= 0.01) return;
    applyCamera(intoChild);
    ctx.save();
    ctx.beginPath();
    ctx.rect(rootRect.x, rootRect.y, rootRect.w, rootRect.h);
    ctx.clip();
    rctx.layerAlpha = alpha;
    paintTiles(rctx, rootNode, rootRect, "");
    rctx.layerAlpha = 1;
    ctx.restore();
  };

  if (zoomIn) {
    // Outgoing parent underneath; the child resolves on top inside its
    // rect while the parent's surroundings fly out of frame.
    paintSnapshot(1 - smoothstep(0.55, 1, progress), false);
    paintLive(smoothstep(0.05, 0.6, progress), true);
  } else {
    // Incoming parent underneath; the outgoing child shrinks back into
    // its own tile and dissolves there.
    paintLive(smoothstep(0, 0.45, progress), false);
    paintSnapshot(1 - smoothstep(0.35, 0.9, progress), true);
  }
};

/** Give the treemap its hover layer: a canvas stacked over the map canvas. */
export const attachHoverLayer = (state: AppState, canvas: HTMLCanvasElement): void => {
  const tm = getTM(state);
  tm.hoverCanvas = canvas;
  tm.hoverCtx = canvas.getContext("2d");
};

/** Keep the hover layer the same size as the map canvas. */
const syncHoverSize = (state: AppState, hover: HTMLCanvasElement): void => {
  const { canvas } = state;
  if (hover.width !== canvas.width) hover.width = canvas.width;
  if (hover.height !== canvas.height) hover.height = canvas.height;
  if (hover.style.width !== canvas.style.width) hover.style.width = canvas.style.width;
  if (hover.style.height !== canvas.style.height) hover.style.height = canvas.style.height;
};

/**
 * Paint the hover marks of the hovered cell on the hover layer. A hover
 * change calls only this, so the map canvas with its thousands of tiles
 * does not repaint. The wash sits above the tile label and rings, which
 * is the only visible difference from painting it into the tile.
 */
export const paintTreemapHover = (state: AppState): void => {
  const tm = getTM(state);
  const hover = tm.hoverCanvas;
  const ctx = tm.hoverCtx;
  if (!hover || !ctx) return;
  syncHoverSize(state, hover);
  ctx.setTransform(1, 0, 0, 1, 0, 0);
  ctx.clearRect(0, 0, hover.width, hover.height);
  const cell = state.hoveredCell === null ? undefined : state.layout[state.hoveredCell];
  if (state.view !== "map" || !cell) return;

  const { theme } = state;
  ctx.setTransform(state.dpr, 0, 0, state.dpr, 0, 0);
  ctx.globalAlpha = 1;
  const fileIndex = cell.node.fileIndex;
  if (fileIndex !== null) {
    const dimmed = state.search.trim() !== "" && !state.searchMatches.has(fileIndex);
    const rect = fileRect(cell);
    ctx.fillStyle = theme.textHigh;
    ctx.globalAlpha = (dimmed ? 0.18 : 1) * 0.18;
    ctx.fillRect(rect.x, rect.y, rect.w, rect.h);
    ctx.globalAlpha = 1;
    ctx.strokeStyle = theme.textHigh;
    ctx.lineWidth = 1;
    ctx.strokeRect(rect.x + 0.5, rect.y + 0.5, rect.w - 1, rect.h - 1);
    return;
  }
  const { tooSmall, showHeader } = dirShape(cell);
  if (tooSmall) {
    ctx.fillStyle = theme.textHigh;
    ctx.globalAlpha = 0.18;
    ctx.fillRect(cell.x + 0.5, cell.y + 0.5, cell.w - 1, cell.h - 1);
    ctx.globalAlpha = 1;
  } else if (showHeader) {
    paintDirHeader(state, ctx, cell, true);
  }
};

const scheduleFrame = (state: AppState): void => {
  const tm = getTM(state);
  cancelAnimationFrame(tm.raf);
  tm.raf = requestAnimationFrame(() => {
    if (state.view === "map") renderTreemap(state);
  });
};

const insetRect = (rect: Rect, by: number): Rect => ({
  x: rect.x + by,
  y: rect.y + by,
  w: Math.max(0, rect.w - by * 2),
  h: Math.max(0, rect.h - by * 2),
});

/**
 * Staggered reveal: top-level folders print in like terminal output
 * lines, each settling from a slight inset so the map assembles rather
 * than flashes in.
 */
const cellReveal = (rctx: RenderCtx, depth: number, seq: number, totalTop: number): number => {
  if (rctx.revealT >= 1 || depth !== 0) return 1;
  const slot = totalTop <= 1 ? 0 : (seq / totalTop) * 0.55;
  return easeOut(clamp01((rctx.revealT - slot) / 0.45));
};

/** Scale a settling cell in from a slight inset around its center. */
const beginSettle = (ctx: CanvasRenderingContext2D, cell: LayoutCell, reveal: number): void => {
  const scale = 0.97 + 0.03 * reveal;
  const cx = cell.x + cell.w / 2;
  const cy = cell.y + cell.h / 2;
  ctx.save();
  ctx.translate(cx * (1 - scale), cy * (1 - scale));
  ctx.scale(scale, scale);
};

/** Recursively render one cell; returns the running sequence counter. */
const renderCell = (
  rctx: RenderCtx,
  cell: LayoutCell,
  depth: number,
  seq: number,
  totalTop: number,
  parentAlpha = rctx.layerAlpha,
): number => {
  const { state } = rctx;
  const { ctx } = state;
  const isFile = cell.node.fileIndex !== null;

  const reveal = cellReveal(rctx, depth, seq, totalTop);
  const nextSeq = seq + 1;
  const alpha = reveal * parentAlpha;
  if (alpha <= 0.01) return nextSeq;

  cell.depth = depth;
  if (rctx.hitTest) state.layout.push(cell);

  const searching = state.search.trim() !== "";
  const settling = reveal < 1;
  if (settling) beginSettle(ctx, cell, reveal);

  ctx.globalAlpha = alpha;
  const result = isFile ? nextSeq : renderDirCell(rctx, cell, depth, nextSeq, totalTop, alpha);
  if (isFile) renderFileCell(rctx, cell, alpha, searching);
  if (settling) ctx.restore();
  ctx.globalAlpha = 1;
  return result;
};

/** A file tile: lens fill, finding hatch, rings, and its label. */
const renderFileCell = (
  rctx: RenderCtx,
  cell: LayoutCell,
  alpha: number,
  searching: boolean,
): void => {
  const { state } = rctx;
  const { ctx, theme, data, index } = state;
  const fi = cell.node.fileIndex as number;
  const file = data.files[fi];
  // Overview colors findings only. Entry points stay neutral in the
  // treemap: test-heavy repos make most tiles entry points, and a tint or
  // outline on all of them hides the findings.
  let fill = lensColor(state.lens, theme, index, file);
  if (state.lens === "overview" && fill === theme.cellEntry) fill = theme.cellNeutral;
  if (rctx.prevColors && rctx.lensT < 1) {
    const prev = rctx.prevColors.get(fi);
    if (prev && prev !== fill) fill = mix(prev, fill, lensProgress(rctx, cell));
  }

  const matched = !searching || state.searchMatches.has(fi);
  if (searching && !matched) ctx.globalAlpha = alpha * 0.18;

  const rect = fileRect(cell);
  ctx.fillStyle = fill;
  ctx.fillRect(rect.x, rect.y, rect.w, rect.h);

  // Texture channel: hatch marks findings so color is never the only
  // signal; severe findings get the dense hatch, mild ones a light one.
  const tm = getTM(state);
  const level = rect.w > 4 && rect.h > 4 ? lensFindingLevel(state.lens, index, file, fi) : 0;
  if (level === 2 && tm.hatch) {
    ctx.fillStyle = tm.hatch;
    ctx.fillRect(rect.x, rect.y, rect.w, rect.h);
  } else if (level === 1 && tm.hatchMild) {
    ctx.fillStyle = tm.hatchMild;
    ctx.fillRect(rect.x, rect.y, rect.w, rect.h);
  }

  // Selection ring (blue = interactive, never a severity color).
  if (state.selected === fi) {
    ctx.strokeStyle = theme.blue;
    ctx.lineWidth = 2;
    ctx.strokeRect(rect.x + 1, rect.y + 1, rect.w - 2, rect.h - 2);
  }

  // Search match ring.
  if (searching && matched) {
    ctx.strokeStyle = theme.amberText;
    ctx.lineWidth = 1.5;
    ctx.strokeRect(rect.x + 0.75, rect.y + 0.75, rect.w - 1.5, rect.h - 1.5);
  }

  if (rctx.labels && cell.w > MIN_LABEL_W && cell.h > MIN_LABEL_H) {
    ctx.fillStyle = contrastText(fill);
    ctx.font = FONT_CELL;
    ctx.textBaseline = "top";
    ctx.textAlign = "left";
    const label = cellLabel(ctx, cell.node.name, cell.w - 8);
    ctx.globalAlpha = ctx.globalAlpha * 0.92;
    ctx.fillText(label, cell.x + 4, cell.y + 3);
    ctx.globalAlpha = alpha;
  }
};

/**
 * Lens fade progress of one tile. The new colors sweep across the map
 * left to right with a slight downward lean, so a lens switch reads as one
 * pass of a scanner instead of a global blink.
 */
const lensProgress = (rctx: RenderCtx, cell: LayoutCell): number => {
  if (!rctx.lensSweep) return easeOut(rctx.lensT);
  const { bounds } = rctx;
  const along =
    ((cell.x + cell.w / 2 - bounds.x) / Math.max(1, bounds.w)) * 0.8 +
    ((cell.y + cell.h / 2 - bounds.y) / Math.max(1, bounds.h)) * 0.2;
  const delay = clamp01(along) * LENS_SWEEP;
  return easeOut(clamp01((rctx.lensT - delay) / (1 - LENS_SWEEP)));
};

/** The painted area of a file tile, inset half a pixel on each side. */
const fileRect = (cell: LayoutCell): Rect => ({
  x: cell.x + 0.5,
  y: cell.y + 0.5,
  w: cell.w - 1,
  h: cell.h - 1,
});

/** How a directory cell paints: a summary tile, a header band, or neither. */
const dirShape = (cell: LayoutCell): { tooSmall: boolean; showHeader: boolean } => {
  const tooSmall = cell.w < 34 || cell.h < 30;
  return { tooSmall, showHeader: !tooSmall && cell.h > DIR_HEADER + 12 && cell.w > 46 };
};

/**
 * The finding note of a directory header in a finding lens: how many
 * files it flags, led by the severe count when there is one.
 */
const dirFlag = (state: AppState, cell: LayoutCell): { flag: string; severe: number } => {
  if (state.lens === "overview") return { flag: "", severe: 0 };
  const { flagged, severe } = dirFlagCounts(state, cell.node);
  if (flagged === 0) return { flag: "", severe };
  if (severe > 0) return { flag: `${formatCount(severe)} high`, severe };
  return { flag: `${formatCount(flagged)}${cell.w > 200 ? " flagged" : ""}`, severe };
};

/** Width of a header text, zero when the text is empty. */
const textWidth = (ctx: CanvasRenderingContext2D, text: string): number =>
  text ? ctx.measureText(text).width : 0;

/** The right-aligned file count and finding note of a directory header. */
const paintDirHeaderSuffix = (
  state: AppState,
  ctx: CanvasRenderingContext2D,
  cell: LayoutCell,
  suffix: { total: string; flagText: string; severe: number; gap: number },
): void => {
  const { theme } = state;
  const { total, flagText, severe, gap } = suffix;
  ctx.textAlign = "right";
  let right = cell.x + cell.w - 5;
  if (total) {
    ctx.fillStyle = theme.textMuted;
    ctx.fillText(total, right, cell.y + 4);
    right -= ctx.measureText(total).width + gap;
  }
  if (flagText) {
    ctx.fillStyle = severe > 0 ? theme.redText : theme.amberText;
    ctx.fillText(flagText, right, cell.y + 4);
  }
  ctx.textAlign = "left";
};

/**
 * The header band of a directory: name on the left; on the right the file
 * count, led in a finding lens by how many of those files it flags.
 */
const paintDirHeader = (
  state: AppState,
  ctx: CanvasRenderingContext2D,
  cell: LayoutCell,
  hovered: boolean,
): void => {
  const { theme } = state;
  ctx.fillStyle = hovered ? theme.surface3 : theme.dirHeader;
  ctx.fillRect(cell.x + 1, cell.y + 1, cell.w - 2, DIR_HEADER - 1);
  ctx.fillStyle = hovered ? theme.textHigh : theme.textLow;
  ctx.font = FONT_DIR;
  ctx.textBaseline = "top";
  ctx.textAlign = "left";
  const count = countFiles(cell.node);
  const { flag, severe } = dirFlag(state, cell);
  const total = cell.w > 150 ? formatCount(count) : "";
  const flagText = flag && cell.w > 90 ? flag : "";
  const gap = flagText && total ? 10 : 0;
  const suffixW = textWidth(ctx, total) + textWidth(ctx, flagText) + gap;
  const label = truncate(ctx, `${cell.node.name}/`, cell.w - 14 - suffixW);
  ctx.fillText(label, cell.x + 5, cell.y + 4);
  paintDirHeaderSuffix(state, ctx, cell, { total, flagText, severe, gap });
};

/**
 * A directory container: summary tile when tiny, otherwise a header
 * band plus recursively squarified children. Returns the running
 * reveal sequence.
 */
const renderDirCell = (
  rctx: RenderCtx,
  cell: LayoutCell,
  depth: number,
  nextSeq: number,
  totalTop: number,
  alpha: number,
): number => {
  const inner = paintDirChrome(rctx, cell, depth);
  if (inner) {
    const children = squarify(cell.node.children, inner);
    let childSeq = nextSeq;
    for (const child of children) {
      childSeq = renderCell(rctx, child, depth + 1, childSeq, totalTop, alpha);
    }
    rctx.state.ctx.globalAlpha = alpha;
    return childSeq;
  }
  return nextSeq;
};

/**
 * Paint a directory cell's own chrome (summary tile, or fill + border +
 * header). Returns the inner child area when the cell nests children,
 * null otherwise; the cached repaint path ignores the return value
 * because child cells already sit in `state.layout`.
 */
const paintDirChrome = (rctx: RenderCtx, cell: LayoutCell, depth: number): Rect | null => {
  const { state } = rctx;
  const { ctx, theme } = state;
  const { tooSmall, showHeader } = dirShape(cell);

  if (tooSmall) {
    ctx.fillStyle = dirSummaryColor(rctx, cell.node);
    ctx.fillRect(cell.x + 0.5, cell.y + 0.5, cell.w - 1, cell.h - 1);
    return null;
  }
  ctx.fillStyle = theme.dirFill;
  ctx.fillRect(cell.x, cell.y, cell.w, cell.h);
  ctx.strokeStyle = depth === 0 ? theme.borderDefault : theme.borderSubtle;
  ctx.lineWidth = 1;
  ctx.strokeRect(cell.x + 0.5, cell.y + 0.5, cell.w - 1, cell.h - 1);

  if (showHeader) paintDirHeader(state, ctx, cell, false);
  return dirInner(cell);
};

/** The child area of a directory cell, or null when it is a summary tile. */
const dirInner = (cell: LayoutCell): Rect | null => {
  const { tooSmall, showHeader } = dirShape(cell);
  if (tooSmall) return null;
  const headerH = showHeader ? DIR_HEADER : 0;
  const inner = {
    x: cell.x + DIR_PAD,
    y: cell.y + headerH + DIR_PAD,
    w: cell.w - DIR_PAD * 2,
    h: cell.h - headerH - DIR_PAD * 2,
  };
  return inner.w > 6 && inner.h > 6 ? inner : null;
};

/**
 * Cache-hit repaint: iterate the cached cells in their original paint
 * order and repaint chrome, fills, rings, and labels without touching
 * the layout list. Only runs when no animation is active, so every
 * reveal/lens/zoom alpha is at its resting value.
 */
const repaintFromLayout = (rctx: RenderCtx): void => {
  const { state } = rctx;
  const { ctx } = state;
  const searching = state.search.trim() !== "";
  for (let index = 0; index < state.layout.length; index++) {
    const cell = state.layout[index];
    ctx.globalAlpha = 1;
    if (cell.node.fileIndex !== null) {
      renderFileCell(rctx, cell, 1, searching);
    } else {
      paintDirChrome(rctx, cell, cell.depth);
    }
  }
  ctx.globalAlpha = 1;
};

// Worst-severity rollup color for directories too small to nest.
const dirSummaryColor = (rctx: RenderCtx, node: TreeNode): string => {
  const { state } = rctx;
  let best = state.theme.cellNeutral;
  let bestRank = -1;
  const walk = (current: TreeNode): void => {
    if (current.fileIndex !== null) {
      const color = lensColor(
        state.lens,
        state.theme,
        state.index,
        state.data.files[current.fileIndex],
      );
      const rank = colorRank(state, color);
      if (rank > bestRank) {
        bestRank = rank;
        best = color;
      }
      return;
    }
    for (const child of current.children) walk(child);
  };
  walk(node);
  return best;
};

const colorRank = (state: AppState, color: string): number => {
  if (color === state.theme.cellNeutral) return 0;
  if (color === state.theme.red) return 4;
  if (color === state.theme.amber) return 3;
  // Entry tint never wins a summary tile: in the overview it is an
  // outline-only marker, and a solid blue block would overclaim.
  if (color === state.theme.cellEntry) return 0;
  return 2;
};

/** Per-lens finding counts of a folder, cached until the lens changes. */
const flagCache = new WeakMap<TreeNode, { lens: string; flagged: number; severe: number }>();

const dirFlagCounts = (state: AppState, node: TreeNode): { flagged: number; severe: number } => {
  const cached = flagCache.get(node);
  if (cached && cached.lens === state.lens) return cached;
  let flagged = 0;
  let severe = 0;
  if (node.fileIndex !== null) {
    const level = lensFindingLevel(
      state.lens,
      state.index,
      state.data.files[node.fileIndex],
      node.fileIndex,
    );
    flagged = level > 0 ? 1 : 0;
    severe = level === 2 ? 1 : 0;
  } else {
    for (const child of node.children) {
      const counts = dirFlagCounts(state, child);
      flagged += counts.flagged;
      severe += counts.severe;
    }
  }
  const entry = { lens: state.lens, flagged, severe };
  flagCache.set(node, entry);
  return entry;
};

const countFiles = (node: TreeNode): number => {
  if (node.fileIndex !== null) return 1;
  let count = 0;
  for (const child of node.children) count += countFiles(child);
  return count;
};

/**
 * File-tile label: prefer dropping the extension over mid-name ellipsis,
 * and render nothing when fewer than five glyphs would fit; empty beats
 * unreadable.
 */
const cellLabel = (ctx: CanvasRenderingContext2D, name: string, maxWidth: number): string => {
  if (ctx.measureText(name).width <= maxWidth) return name;
  const dot = name.lastIndexOf(".");
  const stem = dot > 0 ? name.slice(0, dot) : name;
  if (ctx.measureText(stem).width <= maxWidth) return stem;
  const cut = truncate(ctx, stem, maxWidth);
  return cut.length < 5 ? "" : cut;
};

const truncate = (ctx: CanvasRenderingContext2D, text: string, maxWidth: number): string => {
  if (maxWidth <= 8) return "";
  if (ctx.measureText(text).width <= maxWidth) return text;
  let lo = 0;
  let hi = text.length;
  while (lo < hi) {
    const mid = (lo + hi + 1) >>> 1;
    if (ctx.measureText(`${text.slice(0, mid)}…`).width <= maxWidth) {
      lo = mid;
    } else {
      hi = mid - 1;
    }
  }
  return lo > 0 ? `${text.slice(0, lo)}…` : "";
};

// ── Hit testing & navigation ────────────────────────────────────

/** Smallest cell containing the point (files win over directories). */
export const treemapHitTest = (state: AppState, x: number, y: number): number | null => {
  let hit: number | null = null;
  let hitArea = Infinity;
  for (let index = 0; index < state.layout.length; index++) {
    const cell = state.layout[index];
    if (x >= cell.x && x <= cell.x + cell.w && y >= cell.y && y <= cell.y + cell.h) {
      const isDirHeader =
        cell.node.fileIndex === null && y <= cell.y + DIR_HEADER && cell.w >= 34 && cell.h >= 30;
      const area = cell.w * cell.h;
      if (cell.node.fileIndex !== null || isDirHeader || cell.w < 34 || cell.h < 30) {
        if (area < hitArea) {
          hitArea = area;
          hit = index;
        }
      }
    }
  }
  return hit;
};

/**
 * Where the directory at `targetPath` sits inside the layout of
 * `rootPath`, found by squarifying only along the path between them.
 */
const locateRect = (state: AppState, rootPath: string, targetPath: string): Rect | null => {
  const root = state.index.nodesByPath.get(rootPath) ?? state.index.tree;
  let cells = squarify(root.children, insetRect(stageGeometry(state).root, 1));
  for (;;) {
    const hit = cells.find(
      (cell) => cell.node.path === targetPath || targetPath.startsWith(`${cell.node.path}/`),
    );
    if (!hit) return null;
    if (hit.node.path === targetPath) return hit;
    const inner = dirInner(hit);
    if (!inner) return hit;
    cells = squarify(hit.node.children, inner);
  }
};

/** Drill into a directory cell (with zoom animation). */
export const drillInto = (state: AppState, cell: LayoutCell): void => {
  if (cell.node.fileIndex !== null) return;
  startZoom(state, { x: cell.x, y: cell.y, w: cell.w, h: cell.h }, "in");
  state.drillPath = cell.node.path;
  state.hoveredCell = null;
};

/** Go up one directory level; returns false at the root. */
export const drillUp = (state: AppState): boolean => {
  if (state.drillPath === "") return false;
  const current = state.index.nodesByPath.get(state.drillPath);
  drillTo(state, current?.parent?.path ?? "");
  return true;
};

/** Jump straight to a directory path (breadcrumb navigation). */
export const drillTo = (state: AppState, path: string): void => {
  if (!state.index.nodesByPath.has(path) || path === state.drillPath) return;
  const from = state.drillPath;
  const isAncestor = (outer: string, inner: string): boolean =>
    outer === "" || inner.startsWith(`${outer}/`);
  if (isAncestor(path, from)) {
    startZoom(state, locateRect(state, path, from), "out");
  } else if (isAncestor(from, path)) {
    startZoom(state, locateRect(state, from, path), "in");
  }
  state.drillPath = path;
  state.hoveredCell = null;
};
