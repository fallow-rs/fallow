/**
 * Canvas annotations drawn on top of the overview scene: zoom-level file
 * labels, the teaching intro captions, hover-neighborhood labels, road and
 * cluster labels, the standalone-strip chip, axis endpoints, the legend,
 * and the shift-click path-trace overlay. Every function takes (state, gvs)
 * and paints over the finished scene; none touches the Scene render context.
 */
import type { AppState } from "../state";
import { basename, formatCount } from "../data";
import { dupRamp, heatRamp, zoneColor } from "../theme";
import { fileTipCanvasRect } from "../tooltip";
import {
  type FileNode,
  type GraphViewState,
  FONT_CARD,
  FONT_CHIP,
  FONT_LEGEND,
  FONT_MICRO,
  FONT_SMALL,
  chipRect,
  cubicPoint,
  getGVS,
  markIntroSeen,
  middleTruncate,
  roadGeometry,
  tailTruncate,
  usableStageWidth,
  worldToScreen,
} from "./shared";

/** Axis-aligned overlap between two {x,y,w,h} rectangles. */
const rectsOverlap = (
  a: { x: number; y: number; w: number; h: number },
  b: { x: number; y: number; w: number; h: number },
): boolean => a.x < b.x + b.w && a.x + a.w > b.x && a.y < b.y + b.h && a.y + a.h > b.y;

/** Greedy screen-space labels for the highest-degree files in view. */
export const drawZoomLabels = (
  state: AppState,
  gvs: GraphViewState,
  width: number,
  height: number,
): void => {
  const { ctx, theme, data } = state;
  const { transform } = gvs;
  const candidates: Array<{ node: FileNode; degree: number }> = [];
  for (const node of gvs.fileNodes) {
    if (!node || node.x == null || node.y == null) continue;
    const sx = node.x * transform.k + transform.x;
    const sy = node.y * transform.k + transform.y;
    if (sx < -20 || sx > width + 20 || sy < -20 || sy > height + 20) continue;
    const file = data.files[node.fileIndex];
    candidates.push({ node, degree: file.importer_count + file.import_count });
  }
  const ordered = candidates.toSorted((left, right) => right.degree - left.degree);

  ctx.font = FONT_SMALL;
  ctx.textAlign = "center";
  ctx.textBaseline = "top";
  const placed: Array<{ x: number; y: number; w: number; h: number }> = [];
  let drawn = 0;
  for (const { node } of ordered) {
    if (drawn >= 40) break;
    if (node.x == null || node.y == null) continue;
    const name = basename(data.files[node.fileIndex].path);
    const textW = ctx.measureText(name).width;
    const x = node.x;
    const y = node.y + node.radius + 2 / transform.k;
    // Occupancy check in screen space.
    const sx = x * transform.k + transform.x;
    const sy = y * transform.k + transform.y;
    const rect = { x: sx - textW / 2 - 2, y: sy, w: textW + 4, h: 15 };
    const overlaps = placed.some((placedRect) => rectsOverlap(rect, placedRect));
    if (overlaps) continue;
    placed.push(rect);
    drawn++;
    // Draw in world space (crisper under the active transform); halo
    // instead of a knockout slab, matching the hover labels.
    const worldFont = 12 / transform.k;
    ctx.font = `${worldFont}px "Martian Mono", "JetBrains Mono", ui-monospace, Menlo, monospace`;
    ctx.strokeStyle = theme.bg;
    ctx.lineWidth = 3 / transform.k;
    ctx.lineJoin = "round";
    ctx.globalAlpha = 0.92;
    ctx.strokeText(name, x, y + 1 / transform.k);
    ctx.globalAlpha = 0.9;
    ctx.fillStyle = theme.textLow;
    ctx.fillText(name, x, y + 1 / transform.k);
    ctx.globalAlpha = 1;
    ctx.font = FONT_SMALL;
  }
};
/** Three staged captions that teach the map during the opening reveal. */
export const drawIntroCaptions = (state: AppState, gvs: GraphViewState, width: number): void => {
  if (!gvs.showIntro || gvs.revealAt <= 0) return;
  const { ctx, theme } = state;
  const elapsed = performance.now() - gvs.revealAt;
  // Three beats: the nouns, the lines, then the verbs (what to do next).
  const captions: Array<[number, number, string]> = [
    [0, 2600, "Dots are files, shapes are folders"],
    [2600, 5200, "Lines are imports, thick end points at the importer"],
    [5200, 8600, "Click any dot to open its story"],
  ];
  const total = captions[captions.length - 1][1];
  if (elapsed >= total) {
    gvs.showIntro = false;
    markIntroSeen();
    return;
  }
  for (const [from, to, text] of captions) {
    if (elapsed < from || elapsed >= to) continue;
    const local = (elapsed - from) / (to - from);
    const alpha = local < 0.12 ? local / 0.12 : local > 0.85 ? (1 - local) / 0.15 : 1;
    ctx.font = FONT_CARD;
    ctx.textAlign = "center";
    ctx.textBaseline = "middle";
    const textW = ctx.measureText(text).width;
    ctx.globalAlpha = Math.max(0, alpha);
    // Center on the stage the viewer actually sees: the panel is open on
    // first paint, so full-canvas w/2 would drift under it.
    const cx = usableStageWidth(state, width) / 2;
    // Backed chip so the caption reads over cluster labels behind it.
    chipRect(ctx, cx - textW / 2 - 16, 12, textW + 32, 32, theme.bg, 1, theme.borderSubtle);
    ctx.fillStyle = theme.textHigh;
    ctx.fillText(text, cx, 28.5);
    ctx.globalAlpha = 1;
  }
};
/**
 * Screen-space neighbor labels on hover: fixed 10px regardless of
 * zoom, halo instead of knockout slabs, greedy occupancy across four
 * candidate slots with the docked tooltip pre-seeded as an exclusion
 * zone, and a +N chip for whatever did not fit.
 */
interface LabelSlot {
  x: number;
  y: number;
  align: CanvasTextAlign;
}
type PlacedRect = { x: number; y: number; w: number; h: number };

/**
 * Interleave the importer and import lists, each degree-sorted, so neither
 * side monopolizes the label cap.
 */
const interleaveByDegree = (
  state: AppState,
  importers: Set<number>,
  imports: Set<number>,
): number[] => {
  const byDegree = (fileIndex: number): number =>
    state.data.files[fileIndex].importer_count + state.data.files[fileIndex].import_count;
  const sortedImporters = [...importers].toSorted(
    (left, right) => byDegree(right) - byDegree(left),
  );
  const sortedImports = [...imports].toSorted((left, right) => byDegree(right) - byDegree(left));
  const ordered: number[] = [];
  for (let index = 0; index < Math.max(sortedImporters.length, sortedImports.length); index++) {
    if (index < sortedImporters.length) ordered.push(sortedImporters[index]);
    if (index < sortedImports.length) ordered.push(sortedImports[index]);
  }
  return ordered;
};

/**
 * The first candidate slot whose label rect fits inside the viewport and
 * clears every already-placed rect, or null if the label cannot be placed.
 */
const findLabelSlot = (
  slots: LabelSlot[],
  textW: number,
  width: number,
  height: number,
  placed: PlacedRect[],
): { slot: LabelSlot; rect: PlacedRect } | null => {
  for (const slot of slots) {
    const left =
      slot.align === "center"
        ? slot.x - textW / 2
        : slot.align === "left"
          ? slot.x
          : slot.x - textW;
    const rect = { x: left - 4, y: slot.y - 9, w: textW + 8, h: 18 };
    if (rect.x < 4 || rect.x + rect.w > width - 4 || rect.y < 4 || rect.y + rect.h > height - 4)
      continue;
    const overlaps = placed.some((placedRect) => rectsOverlap(rect, placedRect));
    if (!overlaps) return { slot, rect };
  }
  return null;
};

export const drawHoverLabels = (
  state: AppState,
  gvs: GraphViewState,
  hovered: number,
  importers: Set<number>,
  imports: Set<number>,
  width: number,
  height: number,
): void => {
  const { ctx, theme } = state;
  const kRel = gvs.transform.k / gvs.fitK;
  const cap = kRel >= 1.2 ? 12 : 6;
  const ordered = interleaveByDegree(state, importers, imports);

  const hoveredNode = gvs.fileNodes[hovered];
  if (!hoveredNode || hoveredNode.x == null || hoveredNode.y == null) return;
  const hs = worldToScreen(gvs, { x: hoveredNode.x, y: hoveredNode.y });
  const tipRect = fileTipCanvasRect(hs.x, hs.y, usableStageWidth(state, width), height);
  const placed: Array<{ x: number; y: number; w: number; h: number }> = [tipRect];

  // Faint leader from the hovered node to the docked tooltip's near edge, so
  // the edge-docked card reads as tied to this node instead of floating off
  // on its own at the far side of the canvas.
  const dockedRight = tipRect.x > hs.x;
  const anchorX = dockedRight ? tipRect.x : tipRect.x + tipRect.w;
  const anchorY = Math.min(Math.max(hs.y, tipRect.y + 12), tipRect.y + tipRect.h - 12);
  const lr = hoveredNode.radius * gvs.transform.k;
  const ldx = anchorX - hs.x;
  const ldy = anchorY - hs.y;
  const llen = Math.hypot(ldx, ldy) || 1;
  const lsx = hs.x + (ldx / llen) * (lr + 3);
  const lsy = hs.y + (ldy / llen) * (lr + 3);
  ctx.beginPath();
  ctx.moveTo(lsx, lsy);
  ctx.lineTo(anchorX, anchorY);
  ctx.strokeStyle = theme.textLow;
  ctx.globalAlpha = 0.34;
  ctx.lineWidth = 1;
  ctx.setLineDash([2, 3]);
  ctx.stroke();
  ctx.setLineDash([]);
  // A small dot where the leader meets the card anchors the connection so
  // the card reads as pinned to this node rather than floating beside it.
  ctx.beginPath();
  ctx.arc(anchorX, anchorY, 2, 0, Math.PI * 2);
  ctx.fillStyle = theme.textLow;
  ctx.globalAlpha = 0.5;
  ctx.fill();
  ctx.globalAlpha = 1;

  ctx.font = FONT_SMALL;
  ctx.textBaseline = "middle";
  ctx.lineJoin = "round";
  let drawn = 0;
  for (const fileIndex of ordered) {
    if (drawn >= cap) break;
    const node = gvs.fileNodes[fileIndex];
    if (!node || node.x == null || node.y == null) continue;
    const screen = worldToScreen(gvs, { x: node.x, y: node.y });
    if (screen.x < -20 || screen.x > width + 20 || screen.y < -20 || screen.y > height + 20)
      continue;
    const name = middleTruncate(ctx, basename(state.data.files[fileIndex].path), 140);
    const textW = ctx.measureText(name).width;
    const radius = node.radius * gvs.transform.k + 3;
    const slots: LabelSlot[] = [
      { x: screen.x, y: screen.y + radius + 9, align: "center" },
      { x: screen.x, y: screen.y - radius - 9, align: "center" },
      { x: screen.x + radius + 5, y: screen.y, align: "left" },
      { x: screen.x - radius - 5, y: screen.y, align: "right" },
    ];
    const found = findLabelSlot(slots, textW, width, height, placed);
    if (!found) continue;
    ctx.textAlign = found.slot.align;
    ctx.strokeStyle = theme.bg;
    ctx.lineWidth = 3;
    ctx.globalAlpha = 0.92;
    ctx.strokeText(name, found.slot.x, found.slot.y);
    ctx.globalAlpha = 1;
    ctx.fillStyle = theme.textLow;
    ctx.fillText(name, found.slot.x, found.slot.y);
    placed.push(found.rect);
    drawn++;
  }

  const total = importers.size + imports.size;
  if (total > drawn) {
    const label = `+${formatCount(total - drawn)} more, click for all`;
    ctx.textAlign = "center";
    ctx.strokeStyle = theme.bg;
    ctx.lineWidth = 3;
    ctx.globalAlpha = 0.92;
    ctx.strokeText(label, hs.x, hs.y + hoveredNode.radius * gvs.transform.k + 24);
    ctx.globalAlpha = 1;
    ctx.fillStyle = theme.textMuted;
    ctx.fillText(label, hs.x, hs.y + hoveredNode.radius * gvs.transform.k + 24);
  }
};

/** Path-trace overlay: dim the map, draw the dependency chain on top. */
export const drawPathTrace = (
  state: AppState,
  gvs: GraphViewState,
  width: number,
  height: number,
): void => {
  const { ctx, theme, data } = state;

  if (gvs.pathFrom !== null && gvs.path === null) {
    const node = gvs.fileNodes[gvs.pathFrom];
    if (node && node.x != null && node.y != null) {
      const screen = worldToScreen(gvs, { x: node.x, y: node.y });
      ctx.beginPath();
      ctx.arc(screen.x, screen.y, 10, 0, Math.PI * 2);
      ctx.strokeStyle = theme.blue;
      ctx.lineWidth = 2;
      ctx.setLineDash([4, 3]);
      ctx.stroke();
      ctx.setLineDash([]);
      ctx.font = FONT_MICRO;
      ctx.textAlign = "center";
      ctx.textBaseline = "top";
      ctx.fillStyle = theme.blueText;
      ctx.fillText("Trace from here, shift-click a target", screen.x, screen.y + 16);
    }
    return;
  }

  const path = gvs.path;
  if (!path || path.length < 2) return;

  // Dim everything under the trace.
  ctx.fillStyle = theme.bg;
  ctx.globalAlpha = 0.62;
  ctx.fillRect(0, 0, width, height);
  ctx.globalAlpha = 1;

  const pts = path
    .map((fileIndex) => gvs.fileNodes[fileIndex])
    .filter((node) => node && node.x != null && node.y != null)
    .map((node) => worldToScreen(gvs, { x: node.x ?? 0, y: node.y ?? 0 }));
  if (pts.length < 2) return;

  ctx.beginPath();
  ctx.moveTo(pts[0].x, pts[0].y);
  for (let index = 1; index < pts.length; index++) ctx.lineTo(pts[index].x, pts[index].y);
  ctx.strokeStyle = theme.bg;
  ctx.lineWidth = 6;
  ctx.stroke();
  ctx.strokeStyle = theme.blue;
  ctx.lineWidth = 2;
  ctx.stroke();

  ctx.font = FONT_SMALL;
  ctx.textAlign = "center";
  ctx.textBaseline = "bottom";
  path.forEach((fileIndex, index) => {
    const point = pts[index];
    if (!point) return;
    ctx.beginPath();
    ctx.arc(point.x, point.y, 5, 0, Math.PI * 2);
    ctx.fillStyle = index === 0 || index === path.length - 1 ? theme.blue : theme.textHigh;
    ctx.fill();
    const name = basename(data.files[fileIndex].path);
    const textW = ctx.measureText(name).width;
    ctx.fillStyle = theme.bg;
    ctx.globalAlpha = 0.9;
    ctx.fillRect(point.x - textW / 2 - 3, point.y - 24, textW + 6, 14);
    ctx.globalAlpha = 1;
    ctx.fillStyle = theme.textHigh;
    ctx.fillText(name, point.x, point.y - 11);
  });

  ctx.font = FONT_MICRO;
  ctx.textAlign = "left";
  ctx.textBaseline = "top";
  ctx.fillStyle = theme.blueText;
  ctx.fillText(
    `Dependency trace, ${path.length - 1} hop${path.length === 2 ? "" : "s"}, esc to clear`,
    14,
    28,
  );
};
export const drawRoadLabels = (state: AppState, gvs: GraphViewState): void => {
  const { ctx, theme } = state;
  ctx.font = FONT_MICRO;
  ctx.textAlign = "center";
  ctx.textBaseline = "middle";
  const kRel = gvs.transform.k / gvs.fitK;
  for (let ri = 0; ri < gvs.roads.length; ri++) {
    const road = gvs.roads[ri];
    const focused = gvs.hoveredRoad === ri || gvs.selectedRoad === ri;
    // Quiet by default: numbers appear on zoom or on intent (hover/click).
    if (!focused && kRel < 1.5) continue;
    if (road.count < 2 && !focused) continue;
    const { p0, p1, p2, p3 } = roadGeometry(gvs, road);
    const mid = worldToScreen(gvs, cubicPoint(p0, p1, p2, p3, 0.5));
    const label = formatCount(road.count);
    const textW = ctx.measureText(label).width;
    ctx.fillStyle = theme.bg;
    ctx.globalAlpha = 0.92;
    ctx.fillRect(mid.x - textW / 2 - 3, mid.y - 7, textW + 6, 14);
    ctx.globalAlpha = 1;
    ctx.strokeStyle = theme.borderSubtle;
    ctx.lineWidth = 1;
    ctx.strokeRect(mid.x - textW / 2 - 3.5, mid.y - 7.5, textW + 7, 15);
    if (state.lens === "boundaries" && road.violations > 0) ctx.fillStyle = theme.redText;
    else if (state.lens === "boundaries" && road.bidi && road.cycleEdges > 0)
      ctx.fillStyle = theme.amberText;
    else ctx.fillStyle = theme.textLow;
    ctx.fillText(label, mid.x, mid.y + 0.5);
  }
};

/**
 * Fixed standalone-strip toggle chip, docked above the canvas legend so
 * it never floats orphaned in world space. When open, a caption sits by
 * the revealed strip itself. Also records the chip's hit rect on gvs.
 */
const drawStandaloneChip = (state: AppState, gvs: GraphViewState): void => {
  const { ctx, theme } = state;
  const isolated = gvs.clusters.filter((cluster) => cluster.isolated);
  gvs.standaloneChip = null;
  if (isolated.length === 0) return;
  const canvasHeight = state.canvas.clientHeight;
  const fileCount = isolated.reduce((sum, cluster) => sum + cluster.indices.length, 0);
  ctx.font = FONT_MICRO;
  ctx.textAlign = "left";
  ctx.textBaseline = "middle";
  const label = gvs.standaloneOpen
    ? "Hide standalone files"
    : `${formatCount(fileCount)} standalone files, nothing imports them`;
  const textW = ctx.measureText(label).width;
  const cx0 = 12;
  // Dock just above the legend box (which sits at the bottom-left and grows
  // taller with more keys), so the two never overlap.
  const cy0 = canvasHeight - 12 - legendBoxHeight(state) - 8 - 22;
  chipRect(ctx, cx0, cy0, textW + 16, 22, theme.bg, 0.9, theme.borderSubtle);
  ctx.fillStyle = theme.textMuted;
  ctx.fillText(label, cx0 + 8, cy0 + 11.5);
  gvs.standaloneChip = { x: cx0, y: cy0, w: textW + 16, h: 22 };
  if (gvs.standaloneOpen) {
    const minX = Math.min(...isolated.map((cluster) => cluster.cx - cluster.r));
    const minY = Math.min(...isolated.map((cluster) => cluster.cy - cluster.r));
    const screen = worldToScreen(gvs, { x: minX, y: minY });
    ctx.fillStyle = theme.textMuted;
    ctx.globalAlpha = 0.7;
    ctx.fillText("STANDALONE: configs and CI that nothing imports", screen.x, screen.y - 26);
    ctx.globalAlpha = 1;
  }
};

export const drawClusterLabels = (state: AppState, gvs: GraphViewState): void => {
  const { ctx, theme } = state;
  ctx.font = FONT_CHIP;
  ctx.textAlign = "left";
  ctx.textBaseline = "middle";
  const placed: Array<{ x: number; y: number; w: number; h: number }> = [];
  gvs.clusterLabels = [];
  // Bigger clusters claim their spot first; smaller ones move below on overlap.
  const ordered = gvs.clusters.toSorted(
    (left, right) => right.indices.length - left.indices.length || (left.key < right.key ? -1 : 1),
  );
  const kRel = gvs.transform.k / gvs.fitK;
  for (const cluster of ordered) {
    if (cluster.isolated && !getGVS(state).standaloneOpen) continue;
    // Small multi-file clusters wait for mid zoom (their chips only add
    // collisions at fit); singletons keep their quiet borderless label
    // so no connected dot floats unexplained. On small maps every
    // cluster fits comfortably, so nothing is culled.
    const manyClusters = gvs.clusters.filter((otherCluster) => !otherCluster.isolated).length > 10;
    if (manyClusters && cluster.indices.length >= 2 && cluster.indices.length < 6 && kRel < 1.5) {
      continue;
    }
    let topLeft = cluster.hull[0] ?? { x: cluster.cx, y: cluster.cy };
    for (const point of cluster.hull) {
      if (point.y < topLeft.y || (point.y === topLeft.y && point.x < topLeft.x)) topLeft = point;
    }
    const screen = worldToScreen(gvs, topLeft);
    // Single-file clusters: just the filename, borderless dim text. The
    // full path lives in the tooltip; quiet labels collide far less.
    const single = cluster.indices.length === 1;
    const raw = single ? basename(state.data.files[cluster.indices[0]].path) : cluster.key;
    // Natural case: folder names like `Sidebar`/`Calendar` carry meaning in
    // their casing, so keep it. Multi-file keys are directory paths whose
    // last segment identifies them, so drop leading segments and keep whole
    // trailing ones; single-file labels are bare filenames, where the middle
    // is the safest thing to cut.
    let label: string;
    if (single) {
      label = middleTruncate(ctx, raw, 210);
    } else {
      label = tailTruncate(ctx, raw, 210);
      if (label === "…/" || label === "") {
        label = middleTruncate(ctx, raw.split("/").pop() ?? raw, 210);
      }
    }
    const sub = single ? "" : `${formatCount(cluster.indices.length)} files`;
    const labelW = ctx.measureText(label).width;
    const subW = sub ? ctx.measureText(sub).width : -8;
    const boxW = labelW + subW + 17;
    // Clamp inside the viewport so edge clusters keep readable chips.
    const x = Math.min(
      Math.max(6, screen.x - 4),
      usableStageWidth(state, state.canvas.clientWidth) - boxW - 8,
    );
    let y = screen.y - 12;
    for (let tries = 0; tries < 6; tries++) {
      const overlaps = placed.some(
        (placedRect) =>
          x < placedRect.x + placedRect.w &&
          x + boxW > placedRect.x &&
          y - 9 < placedRect.y + placedRect.h &&
          y + 9 > placedRect.y,
      );
      if (!overlaps) break;
      y += 19;
    }
    placed.push({ x: x - 4, y: y - 10, w: boxW + 3, h: 20 });
    // Record the chip rect so a label hover can light up the cluster's roads.
    if (!cluster.isolated) {
      gvs.clusterLabels.push({
        cluster: gvs.clusters.indexOf(cluster),
        x: x - 4,
        y: y - 10,
        w: labelW + subW + 18,
        h: 20,
      });
    }
    if (state.search.trim() !== "") ctx.globalAlpha = 0.35;
    chipRect(
      ctx,
      x - 4,
      y - 10,
      labelW + subW + 18,
      20,
      theme.bg,
      single ? 0.75 : 0.92,
      single
        ? null
        : cluster.tangle && state.lens === "boundaries"
          ? theme.amber
          : theme.borderSubtle,
    );
    ctx.fillStyle = cluster.isolated || single ? theme.textMuted : theme.textLow;
    ctx.fillText(label, x + 2, y + 0.5);
    if (sub) {
      ctx.fillStyle = theme.textMuted;
      ctx.fillText(sub, x + labelW + 8, y + 0.5);
    }
    ctx.globalAlpha = 1;
  }

  drawStandaloneChip(state, gvs);
};

/** One key on the legend: a visual mark plus the word it means. */
type LegendMark =
  | { kind: "dot"; color: string }
  | { kind: "ring"; color: string; dash?: boolean }
  | { kind: "ramp"; from: string; to: string }
  | { kind: "line"; color: string };

interface LegendEntry {
  mark: LegendMark;
  label: string;
}

const LEGEND_ROW_H = 19;
const LEGEND_PAD_Y = 8;
/** Max zones listed before the boundaries key folds the rest into "+N". */
const MAX_ZONE_LEGEND = 8;

/** What the active lens actually draws on the map, as a real key: color dots,
 *  gradient ramps, and the tapered import line, each with a one-word gloss. */
const legendEntries = (state: AppState): LegendEntry[] => {
  const { theme, data } = state;
  // The map also outlines flagged nodes as a color-blind-safe shape channel:
  // a dashed ring is a milder finding, a solid ring a more severe one. Keyed
  // as two rows so both outlines are shown, not just described.
  const outlineMild: LegendEntry = {
    mark: { kind: "ring", color: theme.textHigh, dash: true },
    label: "milder finding",
  };
  const outlineSevere: LegendEntry = {
    mark: { kind: "ring", color: theme.textHigh },
    label: "severe finding",
  };
  switch (state.lens) {
    case "overview":
      return [
        { mark: { kind: "line", color: theme.textMuted }, label: "import (thick = importer)" },
        { mark: { kind: "dot", color: theme.cellEntry }, label: "entry point" },
      ];
    case "deadcode":
      return [
        { mark: { kind: "dot", color: theme.red }, label: "unused file" },
        { mark: { kind: "dot", color: theme.amber }, label: "unused export" },
        outlineMild,
        outlineSevere,
      ];
    case "dupes":
      return [
        {
          mark: { kind: "ramp", from: dupRamp(theme, 0.2), to: dupRamp(theme, 1) },
          label: "more duplicated",
        },
        outlineMild,
        outlineSevere,
      ];
    case "boundaries": {
      const shown = data.zones.slice(0, MAX_ZONE_LEGEND);
      const entries: LegendEntry[] = shown.map((zone, index) => ({
        mark: { kind: "dot", color: zoneColor(theme, index) },
        label: zone.name,
      }));
      const hidden = data.zones.length - shown.length;
      if (hidden > 0) {
        entries.push({
          mark: { kind: "dot", color: theme.zoneOther },
          label: `+${formatCount(hidden)} more zones`,
        });
      }
      // The amber hull / label outline marks a folder caught in a cluster-level
      // import cycle (folders that import each other); key it only when present.
      if (getGVS(state).clusters.some((cluster) => cluster.tangle)) {
        entries.push({
          mark: { kind: "ring", color: theme.amber },
          label: "folder in an import loop",
        });
      }
      if (data.summary.boundary_violations + data.summary.circular_deps > 0) {
        entries.push({
          mark: { kind: "ring", color: theme.red, dash: true },
          label: "forbidden import / loop",
        });
      }
      return entries;
    }
    case "hotspots":
      return [
        {
          mark: { kind: "ramp", from: heatRamp(theme, 0.2), to: heatRamp(theme, 1) },
          label: "harder to change",
        },
        outlineMild,
        outlineSevere,
      ];
    default:
      return [];
  }
};

/** Pixel height of the legend box, so the standalone chip can dock above it. */
const legendBoxHeight = (state: AppState): number => {
  const count = legendEntries(state).length;
  return count === 0 ? 0 : LEGEND_PAD_Y * 2 + count * LEGEND_ROW_H;
};

/** Draw one legend mark centered at `cy`, spanning `[x, x + width]`. */
const drawLegendMark = (
  ctx: CanvasRenderingContext2D,
  mark: LegendMark,
  x: number,
  cy: number,
  width: number,
): void => {
  const cx = x + width / 2;
  switch (mark.kind) {
    case "dot":
      ctx.fillStyle = mark.color;
      ctx.beginPath();
      ctx.arc(cx, cy, 5, 0, Math.PI * 2);
      ctx.fill();
      break;
    case "ring":
      ctx.strokeStyle = mark.color;
      ctx.lineWidth = 1.5;
      if (mark.dash) ctx.setLineDash([2.5, 2.5]);
      ctx.beginPath();
      ctx.arc(cx, cy, 4.5, 0, Math.PI * 2);
      ctx.stroke();
      ctx.setLineDash([]);
      break;
    case "ramp": {
      const gradient = ctx.createLinearGradient(x, cy, x + width, cy);
      gradient.addColorStop(0, mark.from);
      gradient.addColorStop(1, mark.to);
      ctx.fillStyle = gradient;
      ctx.fillRect(x, cy - 4, width, 8);
      break;
    }
    case "line":
      // Tapered like the map's import ribbons: thick at the importer end.
      ctx.fillStyle = mark.color;
      ctx.beginPath();
      ctx.moveTo(x, cy - 3);
      ctx.lineTo(x + width, cy - 0.75);
      ctx.lineTo(x + width, cy + 0.75);
      ctx.lineTo(x, cy + 3);
      ctx.closePath();
      ctx.fill();
      break;
  }
};

export const drawCanvasLegend = (state: AppState, width: number, height: number): void => {
  const entries = legendEntries(state);
  if (entries.length === 0) return;
  const { ctx, theme } = state;
  ctx.font = FONT_LEGEND;
  ctx.textAlign = "left";
  ctx.textBaseline = "middle";
  const markWidth = 22;
  const markGap = 9;
  const padX = 10;
  let maxLabelWidth = 0;
  for (const entry of entries) {
    maxLabelWidth = Math.max(maxLabelWidth, ctx.measureText(entry.label).width);
  }
  const boxWidth = padX + markWidth + markGap + maxLabelWidth + padX;
  const boxHeight = legendBoxHeight(state);
  const boxX = 10;
  const boxY = height - 12 - boxHeight;
  ctx.fillStyle = theme.bg;
  ctx.globalAlpha = 0.88;
  ctx.fillRect(boxX, boxY, boxWidth, boxHeight);
  ctx.globalAlpha = 1;
  ctx.strokeStyle = theme.borderSubtle;
  ctx.lineWidth = 1;
  ctx.strokeRect(boxX + 0.5, boxY + 0.5, boxWidth - 1, boxHeight - 1);
  entries.forEach((entry, index) => {
    const cy = boxY + LEGEND_PAD_Y + LEGEND_ROW_H * index + LEGEND_ROW_H / 2;
    drawLegendMark(ctx, entry.mark, boxX + padX, cy, markWidth);
    ctx.fillStyle = theme.textLow;
    ctx.fillText(entry.label, boxX + padX + markWidth + markGap, cy);
  });
  void width;
};
