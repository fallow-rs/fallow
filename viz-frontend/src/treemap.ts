import type { AppState } from "./state";
import type { LayoutCell, TreeNode } from "./types";
import { contrastText, mix } from "./theme";
import { formatCount, legendText, lensColor, lensFindingLevel } from "./data";
import { usableStageWidth } from "./graph";

// ── Constants ───────────────────────────────────────────────────

const DIR_HEADER = 18;
const DIR_PAD = 3;
const MIN_LABEL_W = 40;
const MIN_LABEL_H = 13;
const FONT_CELL = '12px "Martian Mono", "JetBrains Mono", ui-monospace, Menlo, monospace';
const FONT_LEGEND = '11px "Martian Mono", "JetBrains Mono", ui-monospace, Menlo, monospace';
const FONT_DIR = '12px "Martian Mono", "JetBrains Mono", ui-monospace, Menlo, monospace';
const ZOOM_MS = 220;
const LENS_MS = 200;
const REVEAL_MS = 420;

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
  /** Zoom: the cell rect (in viewport coords) being expanded/collapsed. */
  rect?: Rect;
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

const easeOut = (progress: number): number => 1 - (1 - progress) * (1 - progress);

/** Kick a zoom transition; `rect` is the drilled cell in viewport coords. */
const startZoom = (state: AppState, rect: Rect, dir: "in" | "out"): void => {
  if (state.reducedMotion) return;
  const tm = getTM(state);
  tm.anim = { kind: dir === "in" ? "zoom-in" : "zoom-out", start: performance.now(), rect };
};

/** Kick a lens crossfade from the current cell colors. */
export const startLensFade = (state: AppState, prevColors: Map<number, string>): void => {
  if (state.reducedMotion) return;
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
  /** 0..1 lens crossfade progress (1 = no fade active). */
  lensT: number;
  prevColors: Map<number, string> | null;
  /** 0..1 reveal progress (1 = fully revealed). */
  revealT: number;
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
  const legend = legendText(state.lens, state.data, "map");
  if (legend !== "") footerChip(ctx, theme, legend, "left", 16, y);
  // The treemap's one non-obvious gesture is drilling; teach it at the
  // root (once drilled, the breadcrumb already shows how to navigate).
  if (state.drillPath === "") {
    footerChip(
      ctx,
      theme,
      "Click a folder to zoom in",
      "right",
      usableStageWidth(state, width) - 16,
      y,
    );
  }
};

export const renderTreemap = (state: AppState): void => {
  const { canvas, ctx } = state;
  // Re-read per render: the window can move to a display with a
  // different pixel ratio mid-session. Keep state.dpr in sync for
  // any consumer that sizes against the backing store.
  const dpr = window.devicePixelRatio || 1;
  state.dpr = dpr;
  const tm = getTM(state);
  const stage = canvas.parentElement;
  const width = stage ? stage.clientWidth : window.innerWidth;
  const height = stage ? stage.clientHeight : window.innerHeight;

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
    tm.hatch = buildHatch(state.theme.textHigh);
    tm.hatchMild = buildHatch(state.theme.textHigh, 0.25);
    tm.hatchKey = state.theme.red;
  }

  const now = performance.now();
  const anim = tm.anim;
  let animT = 1;
  if (anim) {
    const dur = anim.kind === "lens" ? LENS_MS : anim.kind === "reveal" ? REVEAL_MS : ZOOM_MS;
    animT = Math.min(1, (now - anim.start) / dur);
    if (animT >= 1) tm.anim = null;
  }

  ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
  ctx.fillStyle = state.theme.bg;
  ctx.fillRect(0, 0, width, height);

  const rootNode = state.index.nodesByPath.get(state.drillPath) ?? state.index.tree;
  // Reserve a footer gutter for the legend and keep tiles clear of an
  // open right panel.
  const rootRect: Rect = { x: 0, y: 0, w: usableStageWidth(state, width), h: height - 22 };

  const zooming = anim && (anim.kind === "zoom-in" || anim.kind === "zoom-out") && animT < 1;
  const easedProgress = easeOut(animT);

  if (zooming && anim) {
    ctx.save();
    if (anim.kind === "zoom-in" && anim.rect) {
      // The drilled cell expands to fill the viewport: render the new layout
      // inside an interpolated rect that grows from the cell to full size.
      const rect = anim.rect;
      ctx.translate(rect.x * (1 - easedProgress), rect.y * (1 - easedProgress));
      ctx.scale(
        rect.w / width + (1 - rect.w / width) * easedProgress,
        rect.h / height + (1 - rect.h / height) * easedProgress,
      );
    } else {
      // Zoom out: the parent view settles back from slightly zoomed-in.
      const scale = 1.08 - 0.08 * easedProgress;
      ctx.translate((width - width * scale) / 2, (height - height * scale) / 2);
      ctx.scale(scale, scale);
    }
  }

  const rctx: RenderCtx = {
    state,
    now,
    lensT: anim?.kind === "lens" ? easedProgress : 1,
    prevColors: anim?.kind === "lens" ? (anim.prevColors ?? null) : null,
    revealT: anim?.kind === "reveal" ? easedProgress : 1,
    hitTest: !zooming,
    labels: !zooming,
  };

  // Layout cache: geometry only changes with drill, stage size, panel
  // state, or DPR. On a paint-only render (hover, selection ring) the
  // cached cells repaint without re-running squarify. Any in-flight
  // animation bypasses the cache; the reveal populates `state.layout`
  // incrementally and a zoom repaints scaled geometry.
  const layoutKey = treemapLayoutKey(state.drillPath, width, height, rootRect.w, dpr);
  if (tm.anim === null && tm.layoutKey === layoutKey && state.layout.length > 0) {
    repaintFromLayout(rctx);
  } else {
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
  }

  if (!zooming) drawTreemapFooter(state, width, height);

  if (zooming) {
    ctx.restore();
    scheduleFrame(state);
  } else if (anim && animT < 1) {
    scheduleFrame(state);
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

/** Recursively render one cell; returns the running sequence counter. */
const renderCell = (
  rctx: RenderCtx,
  cell: LayoutCell,
  depth: number,
  seq: number,
  totalTop: number,
): number => {
  const { state } = rctx;
  const { ctx } = state;
  const isFile = cell.node.fileIndex !== null;

  // Staggered reveal: top-level cells appear like terminal output lines.
  let alpha = 1;
  if (rctx.revealT < 1 && depth === 0) {
    const slot = totalTop <= 1 ? 0 : seq / (totalTop * 1.4);
    const local = Math.min(1, Math.max(0, (rctx.revealT - slot) / (1 - slot || 1)));
    alpha = easeOut(local);
  }
  const nextSeq = seq + 1;
  if (alpha <= 0.01) return nextSeq;

  cell.depth = depth;
  const layoutIndex = state.layout.length;
  if (rctx.hitTest) state.layout.push(cell);

  const hovered = rctx.hitTest && state.hoveredCell === layoutIndex;
  const searching = state.search.trim() !== "";

  ctx.globalAlpha = alpha;

  if (isFile) {
    renderFileCell(rctx, cell, alpha, hovered, searching);
    ctx.globalAlpha = 1;
    return nextSeq;
  }
  const dirSeq = renderDirCell(rctx, cell, depth, nextSeq, totalTop, alpha, hovered);
  ctx.globalAlpha = 1;
  return dirSeq;
};

/** A file tile: lens fill, finding hatch, rings, and its label. */
const renderFileCell = (
  rctx: RenderCtx,
  cell: LayoutCell,
  alpha: number,
  hovered: boolean,
  searching: boolean,
): void => {
  const { state } = rctx;
  const { ctx, theme, data, index } = state;
  const fi = cell.node.fileIndex as number;
  const file = data.files[fi];
  // In the overview lens entry points keep a neutral fill with a blue
  // outline; a solid tint floods test-heavy repos and stops reading
  // as a marker.
  const entryOutline = state.lens === "overview" && file.status === "entryPoint";
  let fill = entryOutline ? theme.cellNeutral : lensColor(state.lens, theme, index, file);
  if (rctx.prevColors && rctx.lensT < 1) {
    const prev = rctx.prevColors.get(fi);
    if (prev && prev !== fill) fill = mix(prev, fill, rctx.lensT);
  }

  const matched = !searching || state.searchMatches.has(fi);
  if (searching && !matched) ctx.globalAlpha = alpha * 0.18;

  const rect = { x: cell.x + 0.5, y: cell.y + 0.5, w: cell.w - 1, h: cell.h - 1 };
  ctx.fillStyle = fill;
  ctx.fillRect(rect.x, rect.y, rect.w, rect.h);

  if (entryOutline && cell.w > 6 && cell.h > 6) {
    ctx.strokeStyle = theme.blue;
    ctx.globalAlpha = ctx.globalAlpha * 0.55;
    ctx.lineWidth = 1;
    ctx.strokeRect(rect.x + 1, rect.y + 1, rect.w - 2, rect.h - 2);
    ctx.globalAlpha = alpha;
  }

  // Texture channel: hatch marks findings so color is never the only
  // signal; severe findings get the dense hatch, mild ones a light one.
  const tm = getTM(state);
  const level = lensFindingLevel(state.lens, index, file, fi);
  if (level === 2 && tm.hatch) {
    ctx.fillStyle = tm.hatch;
    ctx.fillRect(rect.x, rect.y, rect.w, rect.h);
  } else if (level === 1 && tm.hatchMild) {
    ctx.fillStyle = tm.hatchMild;
    ctx.fillRect(rect.x, rect.y, rect.w, rect.h);
  }

  // Hover: inverse-selection wash + strong border.
  if (hovered) {
    ctx.fillStyle = theme.textHigh;
    ctx.globalAlpha = ctx.globalAlpha * 0.18;
    ctx.fillRect(rect.x, rect.y, rect.w, rect.h);
    ctx.globalAlpha = alpha;
    ctx.strokeStyle = theme.textHigh;
    ctx.lineWidth = 1;
    ctx.strokeRect(rect.x + 0.5, rect.y + 0.5, rect.w - 1, rect.h - 1);
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
  hovered: boolean,
): number => {
  const inner = paintDirChrome(rctx, cell, depth, alpha, hovered);
  if (inner) {
    const children = squarify(cell.node.children, inner);
    let childSeq = nextSeq;
    for (const child of children) {
      childSeq = renderCell(rctx, child, depth + 1, childSeq, totalTop);
    }
    rctx.state.ctx.globalAlpha = 1;
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
const paintDirChrome = (
  rctx: RenderCtx,
  cell: LayoutCell,
  depth: number,
  alpha: number,
  hovered: boolean,
): Rect | null => {
  const { state } = rctx;
  const { ctx, theme } = state;
  // Directory container.
  const tooSmall = cell.w < 34 || cell.h < 30;
  const showHeader = !tooSmall && cell.h > DIR_HEADER + 12 && cell.w > 46;

  if (tooSmall) {
    ctx.fillStyle = dirSummaryColor(rctx, cell.node);
    ctx.fillRect(cell.x + 0.5, cell.y + 0.5, cell.w - 1, cell.h - 1);
    if (hovered) {
      ctx.fillStyle = theme.textHigh;
      ctx.globalAlpha = alpha * 0.18;
      ctx.fillRect(cell.x + 0.5, cell.y + 0.5, cell.w - 1, cell.h - 1);
      ctx.globalAlpha = alpha;
    }
    return null;
  }
  ctx.fillStyle = theme.dirFill;
  ctx.fillRect(cell.x, cell.y, cell.w, cell.h);
  ctx.strokeStyle = depth === 0 ? theme.borderDefault : theme.borderSubtle;
  ctx.lineWidth = 1;
  ctx.strokeRect(cell.x + 0.5, cell.y + 0.5, cell.w - 1, cell.h - 1);

  const headerH = showHeader ? DIR_HEADER : 0;
  if (showHeader) {
    ctx.fillStyle = hovered ? theme.surface3 : theme.dirHeader;
    ctx.fillRect(cell.x + 1, cell.y + 1, cell.w - 2, headerH - 1);
    ctx.fillStyle = hovered ? theme.textHigh : theme.textLow;
    ctx.font = FONT_DIR;
    ctx.textBaseline = "top";
    ctx.textAlign = "left";
    const count = countFiles(cell.node);
    const suffix = cell.w > 150 ? `  ${formatCount(count)}` : "";
    const label = truncate(ctx, `${cell.node.name}/`, cell.w - 10 - ctx.measureText(suffix).width);
    ctx.fillText(label, cell.x + 5, cell.y + 4);
    if (suffix) {
      ctx.fillStyle = theme.textMuted;
      ctx.textAlign = "right";
      ctx.fillText(suffix.trim(), cell.x + cell.w - 5, cell.y + 4);
      ctx.textAlign = "left";
    }
  }

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
    const hovered = state.hoveredCell === index;
    ctx.globalAlpha = 1;
    if (cell.node.fileIndex !== null) {
      renderFileCell(rctx, cell, 1, hovered, searching);
    } else {
      paintDirChrome(rctx, cell, cell.depth, 1, hovered);
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
  state.drillPath = current?.parent?.path ?? "";
  state.hoveredCell = null;
  startZoom(state, { x: 0, y: 0, w: 0, h: 0 }, "out");
  return true;
};

/** Jump straight to a directory path (breadcrumb navigation). */
export const drillTo = (state: AppState, path: string): void => {
  if (!state.index.nodesByPath.has(path)) return;
  const zoomIn = path.length > state.drillPath.length;
  state.drillPath = path;
  state.hoveredCell = null;
  if (!zoomIn) startZoom(state, { x: 0, y: 0, w: 0, h: 0 }, "out");
};
