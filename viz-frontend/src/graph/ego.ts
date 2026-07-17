/**
 * The ego stage: a selected file centered on screen with its importers
 * fanned out left and its imports right, over a ghosted map. Rows
 * group by directory, collapse behind counts on overflow, and click
 * through to re-root.
 */
import type { AppState } from "../state";
import { basename, dirname, formatCount, lensColor } from "../data";
import {
  type GraphViewState,
  FONT_CARD,
  FONT_MICRO,
  FONT_SMALL,
  STAGE_ENTER_MS,
  chipRect,
  easeOut,
  hullPath,
  middleTruncate,
  tailTruncate,
  worldToScreen,
} from "./shared";

// ── Ghost layer (ego mode background) ───────────────────────────

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

export const renderEgoStage = (
  state: AppState,
  gvs: GraphViewState,
  w: number,
  h: number,
): boolean => {
  const { ctx, theme, data } = state;
  const rootIdx = state.selected;
  if (rootIdx === null) return false;
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
  return t < 1 || rowMarching;
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
