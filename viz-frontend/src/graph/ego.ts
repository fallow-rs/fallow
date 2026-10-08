/**
 * The ego stage: a selected file centered on screen with its importers
 * fanned out left and its imports right, over a ghosted map. Rows
 * group by directory, collapse behind counts on overflow, and click
 * through to re-root.
 */
import type { AppState } from "../state";
import { basename, dirname, formatCount, lensColor, reachSet } from "../data";
import {
  type GraphViewState,
  CONTROL_RADIUS,
  FONT_CARD,
  FONT_LEGEND,
  FONT_MICRO,
  FONT_SMALL,
  STAGE_ENTER_MS,
  chipRect,
  easeOut,
  hullPath,
  middleTruncate,
  tailTruncate,
  usableStageWidth,
  worldToScreen,
} from "./shared";

// ── Ghost layer (ego mode background) ───────────────────────────

// Per-selection memo for the transitive blast radius. Invalidated when
// the selected file changes or a new dataset swaps the adjacency array.
let blastCache: { sel: number; adj: number[][]; set: Set<number> } | null = null;

const blastRadius = (state: AppState): Set<number> | null => {
  if (state.selected === null) return null;
  const adj = state.index.importersOf;
  if (blastCache && blastCache.sel === state.selected && blastCache.adj === adj) {
    return blastCache.set;
  }
  const set = reachSet(adj, state.selected);
  blastCache = { sel: state.selected, adj, set };
  return set;
};

export const renderGhost = (state: AppState, gvs: GraphViewState): void => {
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
  // Blast radius: everything that transitively depends on the selected
  // file glows blue in the ghost, so the spread beyond the 1-hop ego
  // fan is visible at a glance. Memoized per selection so the enter
  // animation and row-hover marching do not re-run the BFS each frame.
  const affected = blastRadius(state);
  for (const node of gvs.fileNodes) {
    if (!node || node.x == null || node.y == null) continue;
    const inBlast = affected?.has(node.fileIndex) ?? false;
    ctx.globalAlpha = inBlast ? 0.5 : 0.12;
    ctx.fillStyle = inBlast
      ? theme.blue
      : lensColor(state.lens, theme, state.index, state.data.files[node.fileIndex]);
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
  /** The edge to or from the root loads its target lazily; drawn dashed. */
  dynamic?: boolean;
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
  const fileCount = files.length;
  const isViolation = (other: number): boolean =>
    side === "left"
      ? state.index.violationEdges.has(other * fileCount + rootIdx)
      : state.index.violationEdges.has(rootIdx * fileCount + other);
  const isCycle = (other: number): boolean =>
    state.index.cycleEdges.has(rootIdx * fileCount + other) ||
    state.index.cycleEdges.has(other * fileCount + rootIdx);
  const isDynamic = (other: number): boolean =>
    side === "left"
      ? state.index.dynamicEdges.has(other * fileCount + rootIdx)
      : state.index.dynamicEdges.has(rootIdx * fileCount + other);

  const groups = new Map<string, number[]>();
  for (const fileIndex of indices) {
    const top = files[fileIndex].path.split("/")[0];
    if (!groups.has(top)) groups.set(top, []);
    groups.get(top)?.push(fileIndex);
  }
  const layerOf = (dir: string): number => {
    const cluster = gvs.clusters.find(
      (candidate) => candidate.key === dir || candidate.key.startsWith(`${dir}/`),
    );
    return cluster ? cluster.layer * 1000 + cluster.order : 999999;
  };
  const groupKeys = [...groups.keys()].toSorted(
    (leftKey, rightKey) => layerOf(leftKey) - layerOf(rightKey) || (leftKey < rightKey ? -1 : 1),
  );

  const fileRow = (fileIndex: number): StageRow => ({
    kind: "file",
    fileIndex,
    label: basename(files[fileIndex].path),
    dim: dirname(files[fileIndex].path),
    violation: isViolation(fileIndex),
    cycle: isCycle(fileIndex),
    dynamic: isDynamic(fileIndex),
  });

  const sortIndices = (list: number[]): number[] =>
    list.toSorted((leftIndex, rightIndex) => {
      const leftSeverity = (isViolation(leftIndex) ? 2 : 0) + (isCycle(leftIndex) ? 1 : 0);
      const rightSeverity = (isViolation(rightIndex) ? 2 : 0) + (isCycle(rightIndex) ? 1 : 0);
      if (leftSeverity !== rightSeverity) return rightSeverity - leftSeverity;
      return files[leftIndex].path < files[rightIndex].path ? -1 : 1;
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
    for (const fileIndex of members) rows.push(fileRow(fileIndex));
  }
  if (rows.length > maxRows) {
    const kept = rows.slice(0, maxRows - 1);
    const hidden = rows.length - (maxRows - 1);
    kept.push({ kind: "more", label: `+${hidden} more in the panel` });
    return kept;
  }
  return rows;
};

/** Shared geometry of one ego stage frame. */
interface EgoLayout {
  stageW: number;
  cx: number;
  cy: number;
  availH: number;
  compact: boolean;
  cardW: number;
  leftX: number;
  rightX: number;
  ease: number;
}

/** Restart the enter animation and extend the trail when the root changes. */
const trackEgoRoot = (state: AppState, gvs: GraphViewState, rootIdx: number): void => {
  if (gvs.lastRoot === rootIdx) return;
  gvs.stageEnterAt = state.reducedMotion ? 0 : performance.now();
  if (gvs.crumbs[gvs.crumbs.length - 1] !== rootIdx) {
    gvs.crumbs.push(rootIdx);
    if (gvs.crumbs.length > 12) gvs.crumbs.shift();
  }
  gvs.lastRoot = rootIdx;
};

/**
 * Stage area: the same usable width the treemap and folder graph use, so
 * the ego columns keep the identical clearance from the detail panel.
 */
const egoLayout = (state: AppState, width: number, height: number, ease: number): EgoLayout => {
  const stageW = usableStageWidth(state, width);
  const cx = stageW / 2;
  // On a narrow stage the card shrinks and the columns pull in, so both
  // sides keep room for their file names instead of running off the canvas.
  const compact = stageW < 760;
  const cardW = compact ? Math.max(150, Math.min(250, stageW * 0.34)) : 250;
  const colOffset = compact ? cardW / 2 + 26 : Math.min(Math.max(0.3 * stageW, 230), 430);
  return {
    stageW,
    cx,
    cy: height / 2,
    availH: height - 170,
    compact,
    cardW,
    leftX: cx - colOffset,
    rightX: cx + colOffset,
    ease,
  };
};

/**
 * Column headers, anchored just above each column's own rows (or the
 * card when a side is empty) instead of floating at the viewport top.
 */
const drawColumnHeaders = (
  state: AppState,
  layout: EgoLayout,
  counts: { importers: number; imports: number },
  leftRows: StageRow[],
  rightRows: StageRow[],
): void => {
  const { ctx, theme } = state;
  const { cy, availH, leftX, rightX, compact } = layout;
  const headerY = (rows: StageRow[]): number => {
    if (rows.length === 0) return cy - 33 - 18;
    const rowH = Math.min(24, Math.max(18, availH / rows.length));
    return cy - (rows.length * rowH) / 2 - 18;
  };
  ctx.font = FONT_MICRO;
  ctx.textBaseline = "middle";
  ctx.fillStyle = theme.textMuted;
  ctx.textAlign = "right";
  ctx.fillText(`Imported by ${formatCount(counts.importers)}`, leftX, headerY(leftRows));
  ctx.textAlign = "left";
  ctx.fillText(`Imports ${formatCount(counts.imports)}`, rightX, headerY(rightRows));
  if (counts.importers === 0) {
    ctx.textAlign = "right";
    ctx.fillText(compact ? "None" : "Nothing imports this file", leftX, cy);
  }
  if (counts.imports === 0) {
    ctx.textAlign = "left";
    ctx.fillText(compact ? "None" : "No imports", rightX, cy);
  }
};

/**
 * Escape hatch at the point of attention, not only in the statusbar.
 * A census secondary control: paper fill and a 1px ink frame.
 */
const drawBackChip = (state: AppState, gvs: GraphViewState, ease: number): void => {
  const { ctx, theme } = state;
  ctx.font = FONT_LEGEND;
  ctx.textAlign = "left";
  ctx.textBaseline = "middle";
  const backLabel = "← Back to map (esc)";
  const backW = ctx.measureText(backLabel).width;
  ctx.globalAlpha = ease;
  chipRect(ctx, 12, 12, backW + 24, 30, theme.bg, 1, theme.borderStrong, CONTROL_RADIUS);
  ctx.fillStyle = theme.textHigh;
  ctx.fillText(backLabel, 24, 27.5);
  gvs.egoBackChip = { x: 12, y: 12, w: backW + 24, h: 30 };
};

/** Center card: folder, file name and the in/out counts of the root. */
const drawCenterCard = (
  state: AppState,
  layout: EgoLayout,
  path: string,
  counts: { importers: number; imports: number },
): void => {
  const { ctx, theme } = state;
  const { cx, cy, cardW, compact } = layout;
  const cardH = 66;
  ctx.fillStyle = theme.surface1;
  ctx.fillRect(cx - cardW / 2, cy - cardH / 2, cardW, cardH);
  ctx.strokeStyle = theme.borderStrong;
  ctx.lineWidth = 1;
  ctx.strokeRect(cx - cardW / 2 + 0.5, cy - cardH / 2 + 0.5, cardW - 1, cardH - 1);
  ctx.textAlign = "center";
  ctx.font = FONT_MICRO;
  ctx.fillStyle = theme.textMuted;
  const dir = dirname(path);
  ctx.fillText(middleTruncate(ctx, dir ? `${dir}/` : "", cardW - 20), cx, cy - 18);
  ctx.font = FONT_CARD;
  ctx.fillStyle = theme.textHigh;
  ctx.fillText(middleTruncate(ctx, basename(path), cardW - 20), cx, cy + 1);
  ctx.font = FONT_MICRO;
  ctx.fillStyle = theme.textLow;
  const inCount = formatCount(counts.importers);
  const outCount = formatCount(counts.imports);
  ctx.fillText(
    middleTruncate(
      ctx,
      compact ? `${inCount} in, ${outCount} out` : `Imported by ${inCount}, imports ${outCount}`,
      cardW - 16,
    ),
    cx,
    cy + 19,
  );
};

/** Crosshair at the true map position of the root file. */
const drawRootCrosshair = (
  state: AppState,
  gvs: GraphViewState,
  rootIdx: number,
  ease: number,
): void => {
  const { ctx, theme } = state;
  const rootNode = gvs.fileNodes[rootIdx];
  if (!rootNode || rootNode.x == null || rootNode.y == null) return;
  const screenPos = worldToScreen(gvs, { x: rootNode.x, y: rootNode.y });
  ctx.strokeStyle = theme.blue;
  ctx.globalAlpha = 0.5 * ease;
  ctx.lineWidth = 1;
  ctx.beginPath();
  ctx.moveTo(screenPos.x - 6, screenPos.y);
  ctx.lineTo(screenPos.x + 6, screenPos.y);
  ctx.moveTo(screenPos.x, screenPos.y - 6);
  ctx.lineTo(screenPos.x, screenPos.y + 6);
  ctx.stroke();
  ctx.globalAlpha = ease;
};

/** True while the hovered row's connector marches. */
const rowMarching = (state: AppState, gvs: GraphViewState): boolean =>
  !state.reducedMotion &&
  state.graphHovered !== null &&
  gvs.stageRects.some((rect) => rect.kind === "file" && rect.fileIndex === state.graphHovered);

export const renderEgoStage = (
  state: AppState,
  gvs: GraphViewState,
  width: number,
  height: number,
): boolean => {
  const { ctx } = state;
  const rootIdx = state.selected;
  if (rootIdx === null) return false;

  trackEgoRoot(state, gvs, rootIdx);
  const progress = state.reducedMotion
    ? 1
    : Math.min(1, (performance.now() - gvs.stageEnterAt) / STAGE_ENTER_MS);
  const ease = easeOut(progress);
  const layout = egoLayout(state, width, height, ease);
  const { cx, cy, availH, stageW, cardW, leftX, rightX } = layout;

  gvs.stageRects = [];

  const importers = state.index.importersOf[rootIdx];
  const imports = state.index.importsOf[rootIdx];
  const counts = { importers: importers.length, imports: imports.length };
  const maxRows = Math.max(6, Math.floor(availH / 19));
  const leftRows = buildColumn(state, gvs, rootIdx, importers, "left", maxRows);
  const rightRows = buildColumn(state, gvs, rootIdx, imports, "right", maxRows);

  ctx.save();
  ctx.globalAlpha = ease;

  drawColumnHeaders(state, layout, counts, leftRows, rightRows);

  drawStageColumn(state, gvs, leftRows, "left", leftX, cy, availH, cx, ease, stageW, cardW);
  drawStageColumn(state, gvs, rightRows, "right", rightX, cy, availH, cx, ease, stageW, cardW);

  drawBackChip(state, gvs, ease);
  drawCenterCard(state, layout, state.data.files[rootIdx].path, counts);
  drawRootCrosshair(state, gvs, rootIdx, ease);

  drawCrumbs(state, gvs, stageW);

  ctx.restore();

  return progress < 1 || rowMarching(state, gvs);
};

/** Geometry a stage row shares between its painters. */
interface RowGeom {
  rowY: number;
  textX: number;
  dotX: number;
  dirSign: number;
  side: "left" | "right";
  maxTextW: number;
  ease: number;
}

/**
 * Census map geometry: run level, take the climb at 45 degrees when it
 * fits (a 90 degree elbow when it does not), then run level to the row.
 */
const traceConnector = (
  ctx: CanvasRenderingContext2D,
  cardEdgeX: number,
  cy: number,
  endX: number,
  rowY: number,
): void => {
  ctx.beginPath();
  ctx.moveTo(cardEdgeX, cy);
  const deltaX = endX - cardEdgeX;
  const climb = Math.abs(rowY - cy);
  const direction = Math.sign(deltaX);
  if (climb <= Math.abs(deltaX) * 0.8) {
    const bendX = cardEdgeX + (deltaX - direction * climb) / 2;
    ctx.lineTo(bendX, cy);
    ctx.lineTo(bendX + direction * climb, rowY);
  } else {
    const bendX = cardEdgeX + deltaX / 2;
    ctx.lineTo(bendX, cy);
    ctx.lineTo(bendX, rowY);
  }
  ctx.lineTo(endX, rowY);
};

/** Severity stroke of a connector: red violation, dashed amber cycle, blue edge. */
const styleConnector = (state: AppState, row: StageRow, hoveredRow: boolean): void => {
  const { ctx, theme } = state;
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
    ctx.setLineDash(row.dynamic ? [2, 3] : []);
  }
};

/** Octilinear connector from the card edge to a row, severity-styled. */
const drawRowConnector = (
  state: AppState,
  row: StageRow,
  geom: RowGeom,
  cardEdgeX: number,
  cy: number,
  endX: number,
): void => {
  const { ctx } = state;
  const hoveredRow =
    row.kind === "file" && row.fileIndex !== undefined && state.graphHovered === row.fileIndex;
  traceConnector(ctx, cardEdgeX, cy, endX, geom.rowY);
  ctx.lineJoin = "round";
  styleConnector(state, row, hoveredRow);
  if (hoveredRow && !state.reducedMotion) {
    ctx.setLineDash([8, 6]);
    ctx.lineDashOffset = -((performance.now() / 40) % 14);
  }
  ctx.globalAlpha = hoveredRow ? geom.ease : 0.7 * geom.ease;
  ctx.stroke();
  ctx.setLineDash([]);
  ctx.lineDashOffset = 0;
  ctx.globalAlpha = geom.ease;
};

/** A file row's label: dim directory prefix + severity-colored name. */
const drawFileRowLabel = (state: AppState, row: StageRow, geom: RowGeom): void => {
  const { ctx, theme } = state;
  const dim = row.dim ? `${row.dim}/` : "";
  const nameColor = row.violation ? theme.redText : row.cycle ? theme.amberText : theme.textHigh;
  const fullName = row.cycle ? `${row.label} ~` : row.label;
  // The name truncates only when it alone does not fit; the folder goes first.
  const name =
    ctx.measureText(fullName).width > geom.maxTextW
      ? middleTruncate(ctx, fullName, Math.max(24, geom.maxTextW))
      : fullName;
  const nameW = ctx.measureText(name).width;
  let drawDim = dim;
  if (ctx.measureText(dim).width + nameW > geom.maxTextW) {
    drawDim = tailTruncate(ctx, dim, Math.max(0, geom.maxTextW - nameW));
  }
  const totalW = nameW + ctx.measureText(drawDim).width;
  ctx.fillStyle = theme.bg;
  const prevAlpha = ctx.globalAlpha;
  ctx.globalAlpha = 0.85 * geom.ease;
  if (geom.side === "left") {
    ctx.fillRect(geom.textX - totalW - 2, geom.rowY - 7, totalW + 4, 14);
  } else {
    ctx.fillRect(geom.textX - 2, geom.rowY - 7, totalW + 4, 14);
  }
  ctx.globalAlpha = prevAlpha;
  if (geom.side === "left") {
    ctx.fillStyle = nameColor;
    ctx.fillText(name, geom.textX, geom.rowY);
    ctx.fillStyle = theme.textMuted;
    ctx.fillText(drawDim, geom.textX - nameW, geom.rowY);
  } else {
    ctx.fillStyle = theme.textMuted;
    ctx.fillText(drawDim, geom.textX, geom.rowY);
    ctx.fillStyle = nameColor;
    ctx.fillText(name, geom.textX + ctx.measureText(drawDim).width, geom.rowY);
  }
};

/** Faint leader from a row's dot to the file's true map position. */
const drawRowLeader = (
  state: AppState,
  gvs: GraphViewState,
  row: StageRow,
  geom: RowGeom,
): void => {
  if (row.kind !== "file" || row.fileIndex === undefined) return;
  const node = gvs.fileNodes[row.fileIndex];
  if (!node || node.x == null || node.y == null) return;
  const { ctx, theme } = state;
  const screenPos = worldToScreen(gvs, { x: node.x, y: node.y });
  ctx.beginPath();
  ctx.moveTo(geom.dotX, geom.rowY);
  ctx.lineTo(screenPos.x, screenPos.y);
  ctx.strokeStyle = theme.textLow;
  ctx.globalAlpha = 0.08 * geom.ease;
  ctx.lineWidth = 1;
  ctx.stroke();
  ctx.globalAlpha = geom.ease;
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
  cardW: number,
): void => {
  const { ctx, theme } = state;
  if (rows.length === 0) return;
  const rowH = Math.min(24, Math.max(18, availH / rows.length));
  const totalH = rows.length * rowH;
  let y = cy - totalH / 2 + rowH / 2;
  const dirSign = side === "left" ? -1 : 1;
  const slide = 14 * (1 - ease) * dirSign;
  const cardEdgeX = centerX + dirSign * (cardW / 2 + 3);
  const maxTextW = side === "left" ? colX - 44 : stageW - colX - 44;

  for (const row of rows) {
    const geom: RowGeom = {
      rowY: y,
      textX: colX + dirSign * 14 + slide,
      dotX: colX - dirSign * 6 + slide,
      dirSign,
      side,
      maxTextW,
      ease,
    };
    y += rowH;

    if (row.kind === "file" || row.kind === "group") {
      drawRowConnector(state, row, geom, cardEdgeX, cy, colX - dirSign * 6 + slide);
    }

    ctx.textBaseline = "middle";
    ctx.textAlign = side === "left" ? "right" : "left";

    if (row.kind === "header" || row.kind === "more") {
      ctx.font = FONT_MICRO;
      ctx.fillStyle = theme.textMuted;
      ctx.fillText(
        middleTruncate(
          ctx,
          row.kind === "header" ? row.label.toUpperCase() : row.label,
          Math.max(24, maxTextW),
        ),
        geom.textX,
        geom.rowY,
      );
      continue;
    }

    if (row.kind === "file" && row.fileIndex !== undefined) {
      ctx.fillStyle = lensColor(state.lens, theme, state.index, state.data.files[row.fileIndex]);
    } else {
      ctx.fillStyle = theme.borderStrong;
    }
    ctx.beginPath();
    ctx.arc(geom.dotX, geom.rowY, 4, 0, Math.PI * 2);
    ctx.fill();

    ctx.font = FONT_SMALL;
    if (row.kind === "group") {
      const label = `${row.label} (${row.count ?? 0})`;
      ctx.fillStyle = row.violation ? theme.redText : row.cycle ? theme.amberText : theme.textHigh;
      ctx.fillText(middleTruncate(ctx, label, Math.min(maxTextW, 320)), geom.textX, geom.rowY);
    } else {
      drawFileRowLabel(state, row, geom);
    }
    drawRowLeader(state, gvs, row, geom);

    const rectW = Math.min(maxTextW + 40, 460);
    gvs.stageRects.push({
      x: side === "left" ? colX - rectW : colX - 8,
      y: geom.rowY - rowH / 2,
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
  // Sits below the back chip (y 12..36), not on top of it: the trail only
  // appears once there are 2+ crumbs, so the single-file ego view never
  // reveals the collision. Baseline picked so the whitespace under the chip
  // matches the 12px gap above it (chip top to the header rule).
  const y = 53;
  shown.forEach((fileIndex, index) => {
    const name = basename(data.files[fileIndex].path);
    const isLast = index === shown.length - 1;
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
        fileIndex,
      });
    }
    x += textW;
    if (!isLast) {
      ctx.fillStyle = theme.textMuted;
      ctx.fillText(" / ", x, y);
      x += ctx.measureText(" / ").width;
    }
  });
};
