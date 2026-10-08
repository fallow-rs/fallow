/**
 * Canvas annotations drawn on top of the overview scene: zoom-level file
 * labels, the teaching intro captions, hover-neighborhood labels, road and
 * cluster labels, the standalone-strip chip, axis endpoints, the legend,
 * and the shift-click path-trace overlay. Every function takes (state, gvs)
 * and paints over the finished scene; none touches the Scene render context.
 */
import type { AppState } from "../state";
import { basename, formatCount, lensFindingLevel, overviewMildColor } from "../data";
import { DISPLAY_FACE, dupRamp, heatRamp, mix, zoneColor } from "../theme";
import { fileTipCanvasRect } from "../tooltip";
import {
  CONTROL_RADIUS,
  type ClusterInfo,
  type FileNode,
  type GraphViewState,
  FONT_CARD,
  FONT_CHIP,
  FONT_LEGEND,
  FONT_MICRO,
  FONT_SMALL,
  chipRect,
  getGVS,
  markIntroSeen,
  middleTruncate,
  roadGeometry,
  routePoint,
  tailTruncate,
  panelDocksBelow,
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
    const worldFont = 13 / transform.k;
    ctx.font = `600 ${worldFont}px ${DISPLAY_FACE}`;
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
    [5200, 8600, "Click a dot to open the file details"],
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
    // Each caption rises a few pixels as it fades in and sinks as it
    // leaves, so the three beats read as a sequence, not a flicker.
    const drift = local < 0.5 ? (1 - Math.min(1, alpha)) * 6 : -(1 - Math.min(1, alpha)) * 4;
    // Backed chip so the caption reads over cluster labels behind it.
    // The census readout: an ink plate with paper text.
    chipRect(
      ctx,
      cx - textW / 2 - 16,
      12 + drift,
      textW + 32,
      32,
      theme.textHigh,
      1,
      null,
      CONTROL_RADIUS,
    );
    ctx.fillStyle = theme.bg;
    ctx.fillText(text, cx, 28.5 + drift);
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
  // Keep the anchor in the card's top band: the real card can be shorter
  // than the estimated height, but never shorter than its name and stats.
  const anchorY = Math.min(Math.max(hs.y, tipRect.y + 12), tipRect.y + TIP_ANCHOR_BAND);
  const lr = hoveredNode.radius * gvs.transform.k;
  const ldx = anchorX - hs.x;
  const ldy = anchorY - hs.y;
  const llen = Math.hypot(ldx, ldy) || 1;
  const lsx = hs.x + (ldx / llen) * (lr + 3);
  const lsy = hs.y + (ldy / llen) * (lr + 3);
  ctx.beginPath();
  ctx.moveTo(lsx, lsy);
  ctx.lineTo(anchorX, anchorY);
  // Solid and quiet: dashes on this map mean "imported by the hovered
  // file", and a dashed leader read as one more import.
  ctx.strokeStyle = theme.textMuted;
  ctx.globalAlpha = 0.5;
  ctx.lineWidth = 1;
  ctx.stroke();
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
    const mid = worldToScreen(gvs, routePoint(p0, p1, p2, p3, 0.5));
    const label = formatCount(road.count);
    const textW = ctx.measureText(label).width;
    ctx.fillStyle = theme.bg;
    ctx.globalAlpha = 0.92;
    ctx.fillRect(mid.x - textW / 2 - 3, mid.y - 7, textW + 6, 14);
    ctx.globalAlpha = 1;
    ctx.strokeStyle = theme.borderSubtle;
    ctx.lineWidth = 1;
    ctx.strokeRect(mid.x - textW / 2 - 3.5, mid.y - 7.5, textW + 7, 15);
    if (state.lens === "architecture" && road.violations > 0) ctx.fillStyle = theme.redText;
    else if (state.lens === "architecture" && road.bidi && road.cycleEdges > 0)
      ctx.fillStyle = theme.amberText;
    else ctx.fillStyle = theme.textLow;
    ctx.fillText(label, mid.x, mid.y + 0.5);
  }
};

/** Height of the standalone-strip toggle, a compact census control. */
const TOGGLE_H = 30;

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
  ctx.font = FONT_LEGEND;
  ctx.textAlign = "left";
  ctx.textBaseline = "middle";
  const label = gvs.standaloneOpen
    ? "Hide folders that are not connected"
    : `Show ${formatCount(fileCount)} files in folders that are not connected`;
  const textW = ctx.measureText(label).width;
  const cx0 = 12;
  // Dock just above the legend box (which sits at the bottom-left and grows
  // taller with more keys), so the two never overlap.
  // A census secondary control: paper fill and a 1px ink frame.
  const cy0 = canvasHeight - 12 - legendBoxHeight(state) - 8 - TOGGLE_H;
  chipRect(ctx, cx0, cy0, textW + 24, TOGGLE_H, theme.bg, 1, theme.borderStrong, CONTROL_RADIUS);
  ctx.fillStyle = theme.textHigh;
  ctx.fillText(label, cx0 + 12, cy0 + TOGGLE_H / 2 + 0.5);
  gvs.standaloneChip = { x: cx0, y: cy0, w: textW + 24, h: TOGGLE_H };
};

/**
 * Screen areas labels must not cover: the floating arrange control at the
 * top right and the legend at the bottom left.
 */
const labelObstacles = (state: AppState): Array<{ x: number; y: number; w: number; h: number }> => {
  const canvasRect = state.canvas.getBoundingClientRect();
  const rects: Array<{ x: number; y: number; w: number; h: number }> = [];
  const arrange = state.canvas.parentElement?.querySelector<HTMLElement>(".arrange");
  if (arrange && arrange.offsetParent !== null) {
    const box = arrange.getBoundingClientRect();
    rects.push({
      x: box.left - canvasRect.left - 6,
      y: box.top - canvasRect.top - 6,
      w: box.width + 12,
      h: box.height + 12,
    });
  }
  const legendH = legendBoxHeight(state);
  rects.push({
    x: 0,
    y: state.canvas.clientHeight - legendH - 48,
    w: panelDocksBelow() ? state.canvas.clientWidth : 400,
    h: legendH + 48,
  });
  return rects;
};

/** Lowest point, below the card top, where the tooltip leader may land. */
const TIP_ANCHOR_BAND = 48;

/** Gap between a chip and its hull above which a leader line is drawn. */
const LEADER_GAP = 10;

/**
 * A thin line from a displaced chip to its folder. Without it, a chip
 * that had to move away from a crowded hull reads as the name of
 * whichever folder sits closest to it.
 */
const drawLeader = (
  ctx: CanvasRenderingContext2D,
  color: string,
  chip: { x: number; y: number; w: number; h: number },
  hull: { minX: number; maxX: number; minY: number; maxY: number },
): void => {
  const clamp = (value: number, low: number, high: number): number =>
    Math.min(Math.max(value, low), high);
  // Nearest point pair between the two rectangles.
  const hullX = clamp(chip.x + chip.w / 2, hull.minX, hull.maxX);
  const hullY = clamp(chip.y + chip.h / 2, hull.minY, hull.maxY);
  const chipX = clamp(hullX, chip.x, chip.x + chip.w);
  const chipY = clamp(hullY, chip.y, chip.y + chip.h);
  if (Math.hypot(hullX - chipX, hullY - chipY) <= LEADER_GAP) return;
  ctx.save();
  ctx.globalAlpha = 0.5;
  ctx.strokeStyle = color;
  ctx.lineWidth = 1;
  ctx.beginPath();
  ctx.moveTo(chipX, chipY);
  ctx.lineTo(hullX, hullY);
  ctx.stroke();
  ctx.fillStyle = color;
  ctx.beginPath();
  ctx.arc(hullX, hullY, 2, 0, Math.PI * 2);
  ctx.fill();
  ctx.restore();
};

/**
 * Screen rects of the nodes with a high finding in the active lens. A
 * folder label must not cover the very dots the lens is about.
 */
const severeNodeRects = (
  state: AppState,
  gvs: GraphViewState,
): Array<{ x: number; y: number; w: number; h: number }> => {
  if (state.lens === "overview") return [];
  const rects: Array<{ x: number; y: number; w: number; h: number }> = [];
  for (const node of gvs.fileNodes) {
    if (node.x === undefined || node.y === undefined) continue;
    const file = state.data.files[node.fileIndex];
    if (lensFindingLevel(state.lens, state.index, file, node.fileIndex) !== 2) continue;
    const center = worldToScreen(gvs, { x: node.x, y: node.y });
    const r = node.radius * gvs.transform.k + 4;
    rects.push({ x: center.x - r, y: center.y - r, w: r * 2, h: r * 2 });
  }
  return rects;
};

/** How many of the largest folders keep a label even when crowded. */
const ALWAYS_LABELED = 6;

export const drawClusterLabels = (state: AppState, gvs: GraphViewState): void => {
  const { ctx, theme } = state;
  ctx.font = FONT_CHIP;
  ctx.textAlign = "left";
  ctx.textBaseline = "middle";
  const placed: Array<{ x: number; y: number; w: number; h: number }> = [
    ...labelObstacles(state),
    ...severeNodeRects(state, gvs),
  ];
  gvs.clusterLabels = [];
  // How much of each folder the active lens flags: the label answers
  // "where are the problems" before anyone hovers a dot.
  const counts = new Map<ClusterInfo, { flagged: number; severe: number }>();
  for (const cluster of gvs.clusters) {
    let flagged = 0;
    let severe = 0;
    if (cluster.indices.length > 1) {
      for (const fileIdx of cluster.indices) {
        const level = lensFindingLevel(state.lens, state.index, state.data.files[fileIdx], fileIdx);
        if (level > 0) flagged += 1;
        if (level === 2) severe += 1;
      }
    }
    counts.set(cluster, { flagged, severe });
  }
  // The largest folders claim their spot first so the map keeps its
  // landmarks. After them, folders with findings come before clean ones,
  // so a crowded map drops the labels that have nothing to report.
  const bySize = gvs.clusters.toSorted(
    (left, right) => right.indices.length - left.indices.length || (left.key < right.key ? -1 : 1),
  );
  const severe = (cluster: ClusterInfo): number => counts.get(cluster)?.severe ?? 0;
  const flagged = (cluster: ClusterInfo): number => counts.get(cluster)?.flagged ?? 0;
  // Folders with high findings come first of all: on a small screen only
  // a few labels fit, and those are the ones the reader looks for.
  const urgent = bySize
    .filter((cluster) => severe(cluster) > 0)
    .toSorted((left, right) => severe(right) - severe(left));
  const landmarks = bySize.slice(0, ALWAYS_LABELED).filter((cluster) => severe(cluster) === 0);
  const rest = bySize
    .slice(ALWAYS_LABELED)
    .filter((cluster) => severe(cluster) === 0)
    .toSorted((left, right) => flagged(right) - flagged(left));
  const ordered = [...urgent, ...landmarks, ...rest];
  const kRel = gvs.transform.k / gvs.fitK;
  // Small multi-file clusters wait for mid zoom (their chips only add
  // collisions at fit); singletons keep their quiet borderless label
  // so no connected dot floats unexplained. On small maps every
  // cluster fits comfortably, so nothing is culled.
  const manyClusters = gvs.clusters.filter((otherCluster) => !otherCluster.isolated).length > 10;
  for (const [rank, cluster] of ordered.entries()) {
    if (cluster.isolated && !getGVS(state).standaloneOpen) continue;
    if (manyClusters && cluster.indices.length >= 2 && cluster.indices.length < 6 && kRel < 1.5) {
      continue;
    }
    // Screen bounds of the hull: labels center over it, or under it when
    // the space above is taken.
    let minX = Infinity;
    let maxX = -Infinity;
    let minY = Infinity;
    let maxY = -Infinity;
    for (const point of cluster.hull.length > 0
      ? cluster.hull
      : [{ x: cluster.cx, y: cluster.cy }]) {
      const screenPoint = worldToScreen(gvs, point);
      minX = Math.min(minX, screenPoint.x);
      maxX = Math.max(maxX, screenPoint.x);
      minY = Math.min(minY, screenPoint.y);
      maxY = Math.max(maxY, screenPoint.y);
    }
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
    const flaggedCount = counts.get(cluster)?.flagged ?? 0;
    const severeCount = counts.get(cluster)?.severe ?? 0;
    const flag =
      flaggedCount === 0
        ? ""
        : severeCount > 0
          ? `${formatCount(severeCount)} high`
          : `${formatCount(flaggedCount)} flagged`;
    // Two lines: the name, then its size and findings. Half the width of a
    // one-line chip, so labels collide less and stay over their cluster.
    ctx.font = FONT_CHIP;
    const labelW = ctx.measureText(label).width;
    ctx.font = FONT_MICRO;
    const subW = sub ? ctx.measureText(sub).width : 0;
    const flagW = flag ? ctx.measureText(flag).width : 0;
    const metaW = subW + (sub && flag ? 18 : 0) + flagW;
    const twoLine = metaW > 0;
    const boxW = Math.max(labelW, metaW) + 14;
    const boxH = twoLine ? 36 : 20;
    // Clamp inside the viewport so edge clusters keep readable chips.
    const maxLeft = usableStageWidth(state, state.canvas.clientWidth) - boxW - 8;
    const clampX = (left: number): number => Math.min(Math.max(6, left), maxLeft);
    const centerX = clampX((minX + maxX) / 2 - boxW / 2);
    // Candidate chip tops in order of preference: centered above the hull,
    // centered below it, nudged sideways above it, then inside its top edge.
    const candidates: Array<{ x: number; y: number }> = [
      { x: centerX, y: minY - boxH - 4 },
      { x: centerX, y: maxY + 4 },
      { x: clampX(minX - boxW + 12), y: minY - boxH - 4 },
      { x: clampX(maxX - 12), y: minY - boxH - 4 },
      { x: centerX, y: minY + 4 },
    ];
    const canvasH = state.canvas.clientHeight;
    const overlaps = (spot: { x: number; y: number }): boolean =>
      spot.y < 4 ||
      spot.y + boxH > canvasH - 4 ||
      placed.some(
        (rect) =>
          spot.x < rect.x + rect.w &&
          spot.x + boxW > rect.x &&
          spot.y < rect.y + rect.h &&
          spot.y + boxH > rect.y,
      );
    const free = candidates.find((spot) => !overlaps(spot));
    // A label that cannot find room stays off rather than stacking on a
    // neighbour; its folder still names itself in the tooltip. The largest
    // folders always label, so the map never loses its largest folders.
    // A small stage has no room to force labels; crowded ones drop.
    if (!free && rank >= (panelDocksBelow() ? 0 : ALWAYS_LABELED)) continue;
    const { x, y } = free ?? candidates[0];
    placed.push({ x: x - 3, y: y - 3, w: boxW + 6, h: boxH + 6 });
    // Record the chip rect so a label hover can light up the cluster's roads.
    if (!cluster.isolated) {
      gvs.clusterLabels.push({ cluster: gvs.clusters.indexOf(cluster), x, y, w: boxW, h: boxH });
    }
    drawLeader(ctx, theme.textMuted, { x, y, w: boxW, h: boxH }, { minX, maxX, minY, maxY });
    if (state.search.trim() !== "") ctx.globalAlpha = 0.35;
    // In a finding lens, folders with nothing to report step back so the
    // flagged ones lead.
    else if (
      state.lens !== "overview" &&
      !single &&
      flaggedCount === 0 &&
      !(state.lens === "architecture" && cluster.tangle)
    ) {
      ctx.globalAlpha = 0.45;
    }
    chipRect(
      ctx,
      x,
      y,
      boxW,
      boxH,
      theme.bg,
      1,
      !single && cluster.tangle && state.lens === "architecture" ? theme.amber : null,
    );
    ctx.textBaseline = "middle";
    ctx.font = FONT_CHIP;
    ctx.fillStyle =
      (cluster.isolated || single) && flaggedCount === 0 ? theme.textMuted : theme.textHigh;
    ctx.fillText(label, x + 7, y + 10.5);
    if (twoLine) {
      ctx.font = FONT_MICRO;
      if (sub) {
        ctx.fillStyle = theme.textMuted;
        ctx.fillText(sub, x + 7, y + 26.5);
      }
      if (flag) {
        ctx.fillStyle = severeCount > 0 ? theme.redText : theme.amberText;
        ctx.fillText(flag, x + 7 + subW + (sub ? 18 : 0), y + 26.5);
        if (sub) {
          ctx.fillStyle = theme.textMuted;
          ctx.fillText("·", x + 7 + subW + 6, y + 26.5);
        }
      }
    }
    ctx.globalAlpha = 1;
  }

  drawStandaloneChip(state, gvs);
};

/** One key on the legend: a visual mark plus the word it means. */
type LegendMark =
  | { kind: "dot"; color: string; ring?: { color: string; dash: boolean } }
  | { kind: "ring"; color: string; dash?: boolean }
  | { kind: "ramp"; from: string; to: string }
  | { kind: "line"; color: string };

interface LegendEntry {
  mark: LegendMark;
  label: string;
}

const LEGEND_ROW_H = 22;
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
    label: "medium finding",
  };
  const outlineSevere: LegendEntry = {
    mark: { kind: "ring", color: theme.textHigh },
    label: "high finding",
  };
  // Where a color already names the level, the ring goes around its dot
  // as one key instead of two rows that say the same thing.
  const mildRing = { color: theme.textHigh, dash: true };
  const severeRing = { color: theme.textHigh, dash: false };
  // While a search is active the rings mean matches, not findings.
  if (state.search.trim() !== "") {
    return [
      { mark: { kind: "ring", color: theme.amber }, label: "matches the search" },
      { mark: { kind: "ring", color: theme.blue }, label: "imports a match" },
    ];
  }
  switch (state.lens) {
    case "overview":
      return [
        { mark: { kind: "dot", color: theme.red }, label: "high, or findings in 3+ lenses" },
        { mark: { kind: "dot", color: overviewMildColor(theme) }, label: "one or more findings" },
        { mark: { kind: "dot", color: theme.cellEntry }, label: "entry point" },
        { mark: { kind: "line", color: theme.textMuted }, label: "import (thick = importer)" },
      ];
    case "unused":
      return [
        { mark: { kind: "dot", color: theme.red, ring: severeRing }, label: "unused file" },
        { mark: { kind: "dot", color: theme.amber, ring: mildRing }, label: "unused export" },
      ];
    case "duplication":
      return [
        {
          mark: { kind: "ramp", from: dupRamp(theme, 0.2), to: dupRamp(theme, 1) },
          label: "more duplicated",
        },
        outlineMild,
        outlineSevere,
      ];
    case "architecture": {
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
      if (data.architecture.availability.count + data.summary.circular_deps > 0) {
        entries.push({
          mark: { kind: "ring", color: theme.red, dash: true },
          label: "forbidden import / loop",
        });
      }
      return entries;
    }
    case "health":
      return [
        {
          mark: { kind: "ramp", from: heatRamp(theme, 0.2), to: heatRamp(theme, 1) },
          label: "higher Health risk",
        },
        outlineMild,
        outlineSevere,
      ];
    case "security":
      return [
        {
          mark: { kind: "dot", color: theme.red, ring: severeRing },
          label: "high-priority candidate",
        },
        { mark: { kind: "dot", color: theme.amber, ring: mildRing }, label: "review candidate" },
      ];
    default:
      return [];
  }
};

/** Row height and gaps of the one-line legend used on narrow screens. */
const COMPACT_ROW_H = 22;
const COMPACT_GAP = 14;

/**
 * Narrow-screen legend: color keys only, laid out in rows that wrap to the
 * stage width. The ring keys repeat what the colors say, so they drop.
 */
const compactLegendRows = (state: AppState, width: number): LegendEntry[][] => {
  const { ctx } = state;
  // Measuring must not leak a font change into the caller's drawing.
  ctx.save();
  ctx.font = FONT_LEGEND;
  const rows: LegendEntry[][] = [[]];
  let used = 0;
  for (const entry of legendEntries(state)) {
    if (entry.mark.kind === "ring" && state.search.trim() === "") continue;
    const entryW = 16 + ctx.measureText(entry.label).width + COMPACT_GAP;
    if (used > 0 && used + entryW > width - 32) {
      rows.push([]);
      used = 0;
    }
    rows[rows.length - 1].push(entry);
    used += entryW;
  }
  ctx.restore();
  return rows.filter((row) => row.length > 0);
};

/** Pixel height of the legend box, so the standalone chip can dock above it. */
const legendBoxHeight = (state: AppState): number => {
  if (panelDocksBelow()) {
    const rows = compactLegendRows(state, state.canvas.clientWidth).length;
    return rows === 0 ? 0 : rows * COMPACT_ROW_H + 8;
  }
  const count = legendEntries(state).length;
  return count === 0 ? 0 : LEGEND_PAD_Y * 2 + count * LEGEND_ROW_H;
};

const drawCompactLegend = (state: AppState, width: number, height: number): void => {
  const rows = compactLegendRows(state, width);
  if (rows.length === 0) return;
  const { ctx, theme } = state;
  const boxH = legendBoxHeight(state);
  const boxY = height - 8 - boxH;
  // Flat census key: an opaque paper band under the map, no translucency.
  ctx.fillStyle = theme.bg;
  ctx.fillRect(8, boxY, width - 16, boxH);
  ctx.font = FONT_LEGEND;
  ctx.textAlign = "left";
  ctx.textBaseline = "middle";
  rows.forEach((row, rowIndex) => {
    const cy = boxY + 4 + COMPACT_ROW_H * rowIndex + COMPACT_ROW_H / 2;
    let x = 16;
    for (const entry of row) {
      drawLegendMark(ctx, entry.mark, x - 4, cy, 16);
      ctx.fillStyle = theme.textLow;
      ctx.fillText(entry.label, x + 14, cy);
      x += 16 + ctx.measureText(entry.label).width + COMPACT_GAP;
    }
  });
};

/** A legend dot, with its finding ring when the mark has one. */
const drawDotMark = (
  ctx: CanvasRenderingContext2D,
  mark: Extract<LegendMark, { kind: "dot" }>,
  cx: number,
  cy: number,
): void => {
  ctx.fillStyle = mark.color;
  ctx.beginPath();
  ctx.arc(cx, cy, mark.ring ? 4 : 5, 0, Math.PI * 2);
  ctx.fill();
  if (!mark.ring) return;
  ctx.strokeStyle = mark.ring.color;
  ctx.lineWidth = 1.2;
  if (mark.ring.dash) ctx.setLineDash([2, 2]);
  ctx.beginPath();
  ctx.arc(cx, cy, 6.5, 0, Math.PI * 2);
  ctx.stroke();
  ctx.setLineDash([]);
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
      drawDotMark(ctx, mark, cx, cy);
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
      // Census swatches are flat: the ramp shows as solid steps, no gradient.
      const steps = 4;
      const stepW = width / steps;
      for (let step = 0; step < steps; step++) {
        ctx.fillStyle = mix(mark.from, mark.to, step / (steps - 1));
        ctx.fillRect(x + step * stepW, cy - 4, stepW - 1, 8);
      }
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
  if (panelDocksBelow()) {
    drawCompactLegend(state, width, height);
    return;
  }
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
  // The map key is a panel printed on the map: paper fill, 1px ink frame.
  ctx.fillStyle = theme.bg;
  ctx.fillRect(boxX, boxY, boxWidth, boxHeight);
  ctx.strokeStyle = theme.borderStrong;
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
