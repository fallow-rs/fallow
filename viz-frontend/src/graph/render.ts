/**
 * Everything the graph view paints: the overview scene (hulls, roads,
 * nodes, labels, legend, axis, minimap, intro captions, hover
 * neighborhood, path trace) and the ego stage with its columns and
 * breadcrumbs.
 */
import { select } from "d3-selection";
import { zoomIdentity } from "d3-zoom";
import type { AppState } from "../state";
import { basename, formatCount, legendText, lensColor } from "../data";
import { fileTipCanvasRect } from "../tooltip";
import { renderEgoStage, renderGhost } from "./ego";
import {
  type ClusterInfo,
  type FileNode,
  type GraphViewState,
  type Pt,
  FONT_CARD,
  FONT_CHIP,
  FONT_LEGEND,
  FONT_MICRO,
  FONT_SMALL,
  LOD_INTER,
  LOD_INTRA,
  LOD_SEVERITY,
  chipRect,
  clusterBounds,
  cubicPoint,
  easeOut,
  getGVS,
  hullPath,
  isTestCluster,
  markIntroSeen,
  middleTruncate,
  roadGeometry,
  roadWidth,
  taperedRibbon,
  usableStageWidth,
  worldToScreen,
} from "./shared";

export const renderGraph = (state: AppState): void => {
  const { canvas, ctx, theme, dpr } = state;
  const gvs = getGVS(state);
  if (!gvs.initialized) return;

  const stageEl = canvas.parentElement;
  const w = stageEl ? stageEl.clientWidth : window.innerWidth;
  const h = stageEl ? stageEl.clientHeight : window.innerHeight;
  const pw = Math.round(w * dpr);
  const ph = Math.round(h * dpr);
  if (canvas.width !== pw || canvas.height !== ph) {
    canvas.style.width = `${w}px`;
    canvas.style.height = `${h}px`;
    canvas.width = pw;
    canvas.height = ph;
  }

  ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
  ctx.fillStyle = theme.bg;
  ctx.fillRect(0, 0, w, h);

  if (state.selected !== null && gvs.fileNodes[state.selected]) {
    renderGhost(state, gvs);
    // The stage reports whether its choreography still runs; this
    // module owns the animation loop.
    if (renderEgoStage(state, gvs, w, h)) {
      cancelAnimationFrame(gvs.raf);
      gvs.raf = requestAnimationFrame(() => {
        if (state.view === "graph") renderGraph(state);
      });
    }
  } else {
    gvs.stageRects = [];
    gvs.lastRoot = null;
    renderOverview(state, gvs, w, h);
  }
};
// ── Overview ────────────────────────────────────────────────────

/** Opening choreography: layers sweep in left to right, then the roads. */
const REVEAL_LAYER_MS = 110;
const REVEAL_FADE_MS = 380;

const revealProgress = (
  gvs: GraphViewState,
  reduced: boolean,
): { t: number; cluster: (c: ClusterInfo) => number; roads: number; labels: number } => {
  if (gvs.revealAt === 0) gvs.revealAt = reduced ? -1 : performance.now();
  if (gvs.revealAt < 0) {
    return { t: 1, cluster: () => 1, roads: 1, labels: 1 };
  }
  const elapsed = performance.now() - gvs.revealAt;
  const maxLayer = gvs.clusters.reduce((max, c) => Math.max(max, c.isolated ? 0 : c.layer), 0);
  const total = (maxLayer + 1) * REVEAL_LAYER_MS + REVEAL_FADE_MS + 420;
  const t = Math.min(1, elapsed / total);
  const clusterAlpha = (c: ClusterInfo): number => {
    const start = (c.isolated ? maxLayer + 1 : c.layer) * REVEAL_LAYER_MS;
    return easeOut(Math.min(1, Math.max(0, (elapsed - start) / REVEAL_FADE_MS)));
  };
  const roadsStart = (maxLayer + 1) * REVEAL_LAYER_MS * 0.6;
  const roads = easeOut(Math.min(1, Math.max(0, (elapsed - roadsStart) / (REVEAL_FADE_MS + 200))));
  const labelsStart = roadsStart + 180;
  const labels = easeOut(Math.min(1, Math.max(0, (elapsed - labelsStart) / REVEAL_FADE_MS)));
  return { t, cluster: clusterAlpha, roads, labels };
};

const renderOverview = (state: AppState, gvs: GraphViewState, w: number, h: number): void => {
  const { ctx, theme, data } = state;
  const { transform, clusters, roads, fileNodes } = gvs;
  const files = data.files;
  const searching = state.search.trim() !== "";
  const kRel = transform.k / gvs.fitK;
  const reveal = revealProgress(gvs, state.reducedMotion);

  ctx.save();
  ctx.translate(transform.x, transform.y);
  ctx.scale(transform.k, transform.k);

  // Hull fills.
  for (const cluster of clusters) {
    if (cluster.isolated && !gvs.standaloneOpen) continue;
    if (cluster.hull.length < 3) continue;
    ctx.beginPath();
    hullPath(ctx, cluster.hull);
    ctx.fillStyle = theme.surface2;
    ctx.globalAlpha = 0.9 * reveal.cluster(cluster) * (state.graphHovered !== null ? 0.6 : 1);
    ctx.fill();
    ctx.globalAlpha = 1;
  }

  // Individual file edges are LOD-gated: intra-cluster from mid zoom,
  // inter-cluster only at deep zoom.
  if (kRel >= LOD_INTRA) {
    drawFileEdges(state, gvs, true, 0.12, 1 / transform.k);
  }
  if (kRel >= LOD_INTER) {
    drawFileEdges(state, gvs, false, 0.1, 0.8 / transform.k);
  }

  // Hull borders.
  for (const cluster of clusters) {
    if (cluster.isolated && !gvs.standaloneOpen) continue;
    if (cluster.hull.length < 3) continue;
    ctx.beginPath();
    hullPath(ctx, cluster.hull);
    const showTangle = cluster.tangle && state.lens === "boundaries";
    ctx.strokeStyle = showTangle ? theme.amber : theme.borderDefault;
    ctx.globalAlpha = (showTangle ? 0.7 : 0.6) * reveal.cluster(cluster);
    ctx.lineWidth = 1 / transform.k;
    ctx.stroke();
    ctx.globalAlpha = 1;
  }

  const hoverDim = state.graphHovered !== null ? 0.35 : 1;

  // Roads: tapered ribbons, wide at importer, narrow at imported.
  // At fit zoom the ribbons carry the whole story, so hold a minimum
  // on-screen width and lift the alpha; both relax as the user zooms in.
  const roadBoost = Math.min(1, Math.max(0, 1.6 - kRel));
  const minRoadW = 1.8 / transform.k;
  // High-traffic roads get promoted a step so the trunk routes survive
  // a projector; the threshold is the 75th percentile of bundle sizes.
  const roadCounts = roads.map((r) => r.count).sort((a, b) => a - b);
  const trunkFloor =
    roadCounts.length > 0 ? roadCounts[Math.floor(roadCounts.length * 0.75)] : Infinity;
  for (const road of roads) {
    const { p0, p1, p2, p3 } = roadGeometry(gvs, road);
    const wSrc = Math.max(minRoadW, roadWidth(road.count));
    ctx.beginPath();
    const thinRatio = road.count >= trunkFloor ? 0.15 : 0.22;
    taperedRibbon(ctx, p0, p1, p2, p3, wSrc, Math.max(0.5, wSrc * thinRatio));
    ctx.fillStyle = theme.textLow;
    // Test-to-source imports are the least interesting overview signal
    // but the biggest bundles; keep them recessive so source roads lead.
    const testDim = isTestCluster(clusters[road.src].key) ? 0.4 : 1;
    const trunk = road.count >= trunkFloor && testDim === 1 ? 0.22 : 0;
    ctx.globalAlpha = (0.3 + 0.18 * roadBoost + trunk) * testDim * reveal.roads * hoverDim;
    ctx.fill();
    ctx.globalAlpha = 1;

    // Severity overdraw parallel to the road (boundaries lens only;
    // the overview stays neutral until the user asks a question).
    if (state.lens === "boundaries" && (road.violations > 0 || (road.bidi && road.cycleEdges > 0))) {
      ctx.beginPath();
      ctx.moveTo(p0.x, p0.y + 4);
      ctx.bezierCurveTo(p1.x, p1.y + 4, p2.x, p2.y + 4, p3.x, p3.y + 4);
      if (road.violations > 0) {
        ctx.strokeStyle = theme.red;
        ctx.setLineDash([]);
      } else {
        ctx.strokeStyle = theme.amber;
        ctx.setLineDash([4 / transform.k, 3 / transform.k]);
      }
      ctx.lineWidth = 1.2 / transform.k;
      ctx.globalAlpha = 0.9;
      ctx.stroke();
      ctx.setLineDash([]);
      ctx.globalAlpha = 1;
    }
  }

  // Individual severity edges from mid zoom (boundaries lens only).
  if (state.lens === "boundaries" && kRel >= LOD_SEVERITY) drawSeverityEdges(state, gvs);

  // Hovered / selected road highlight: bright centerline, marching when hovered.
  const focusRoad = gvs.hoveredRoad ?? gvs.selectedRoad;
  if (focusRoad !== null && gvs.roads[focusRoad]) {
    const { p0, p1, p2, p3 } = roadGeometry(gvs, gvs.roads[focusRoad]);
    ctx.beginPath();
    ctx.moveTo(p0.x, p0.y);
    ctx.bezierCurveTo(p1.x, p1.y, p2.x, p2.y, p3.x, p3.y);
    ctx.strokeStyle = theme.bg;
    ctx.lineWidth = 5 / transform.k;
    ctx.globalAlpha = 0.8;
    ctx.stroke();
    ctx.strokeStyle = theme.blue;
    ctx.lineWidth = 2 / transform.k;
    ctx.globalAlpha = 1;
    if (gvs.hoveredRoad !== null && !state.reducedMotion) {
      ctx.setLineDash([8 / transform.k, 6 / transform.k]);
      ctx.lineDashOffset = -((performance.now() / 40) % 14) / transform.k;
    }
    ctx.stroke();
    ctx.setLineDash([]);
    ctx.lineDashOffset = 0;
    // Direction stamp: a filled dot marks the importer end, so the
    // taper's meaning is confirmable the moment a road is focused.
    ctx.beginPath();
    ctx.arc(p0.x, p0.y, 4 / transform.k, 0, Math.PI * 2);
    ctx.fillStyle = theme.blue;
    ctx.fill();
  }

  // Hover neighborhood. Direction is dual-encoded: files importing the
  // hovered one arrive as solid blue ribbons (thick end at the
  // importer, same rule as roads); its own imports leave as thin
  // dashed blue lines.
  const hovered = state.graphHovered;
  let neighbors: Set<number> | null = null;
  const hoverImporters = new Set<number>();
  const hoverImports = new Set<number>();
  if (hovered !== null) {
    neighbors = new Set([hovered]);
    for (const [from, to] of data.edges) {
      if (from !== hovered && to !== hovered) continue;
      neighbors.add(from);
      neighbors.add(to);
      if (to === hovered && from !== hovered) hoverImporters.add(from);
      if (from === hovered && to !== hovered) hoverImports.add(to);
      const a = fileNodes[from];
      const b = fileNodes[to];
      if (!a || !b || a.x == null || a.y == null || b.x == null || b.y == null) continue;
      edgeUnderlay(ctx, { x: a.x, y: a.y }, { x: b.x, y: b.y }, theme.bg, 4 / transform.k);
      if (to === hovered) {
        const p0 = { x: a.x, y: a.y };
        const p3 = { x: b.x, y: b.y };
        const p1 = { x: p0.x + (p3.x - p0.x) / 3, y: p0.y + (p3.y - p0.y) / 3 };
        const p2 = { x: p0.x + ((p3.x - p0.x) * 2) / 3, y: p0.y + ((p3.y - p0.y) * 2) / 3 };
        ctx.beginPath();
        taperedRibbon(ctx, p0, p1, p2, p3, 2.4 / transform.k, 0.6 / transform.k);
        ctx.fillStyle = theme.blue;
        ctx.globalAlpha = 0.9;
        ctx.fill();
      } else {
        ctx.beginPath();
        ctx.moveTo(a.x, a.y);
        ctx.lineTo(b.x, b.y);
        ctx.strokeStyle = theme.blue;
        ctx.globalAlpha = 0.45;
        ctx.lineWidth = 1.1 / transform.k;
        ctx.setLineDash([4 / transform.k, 3 / transform.k]);
        ctx.stroke();
        ctx.setLineDash([]);
      }
    }
    ctx.globalAlpha = 1;
  }

  // Nodes.
  for (const node of fileNodes) {
    if (!node || node.x == null || node.y == null) continue;
    if (clusters[node.cluster].isolated && !gvs.standaloneOpen) continue;
    const file = files[node.fileIndex];
    const color = lensColor(state.lens, theme, state.index, file);
    const recessive = color === theme.cellNeutral || color === theme.cellEntry;
    const matched = !searching || state.searchMatches.has(node.fileIndex);
    const isNeighbor = neighbors?.has(node.fileIndex) ?? false;
    const dimmed = neighbors !== null && !isNeighbor;

    let alpha = recessive ? 0.82 : 0.95;
    if (dimmed) alpha = 0.16;
    if (searching && !matched) alpha = Math.min(alpha, 0.1);
    if (isNeighbor) alpha = 1;
    alpha *= reveal.cluster(clusters[node.cluster]);
    if (alpha <= 0.01) continue;

    ctx.globalAlpha = alpha;
    ctx.fillStyle = color;
    ctx.beginPath();
    ctx.arc(
      node.x,
      node.y,
      node.radius * (state.graphHovered === node.fileIndex ? 1.3 : 1),
      0,
      Math.PI * 2,
    );
    ctx.fill();

    // Direction ring on hover neighbors, echoing the tooltip prefixes:
    // solid blue ring = imports the hovered file, quiet ring = imported
    // by it.
    if (hoverImporters.has(node.fileIndex) || hoverImports.has(node.fileIndex)) {
      const importer = hoverImporters.has(node.fileIndex);
      ctx.strokeStyle = importer ? theme.blue : theme.borderStrong;
      ctx.lineWidth = (importer ? 1.6 : 1) / transform.k;
      ctx.beginPath();
      ctx.arc(node.x, node.y, node.radius + 2.5 / transform.k, 0, Math.PI * 2);
      ctx.stroke();
    }

    if (!dimmed) {
      if (state.lens === "deadcode" && file.status === "unused") {
        ctx.setLineDash([3 / transform.k, 3 / transform.k]);
        ctx.strokeStyle = theme.redText;
        ctx.lineWidth = 1.4 / transform.k;
        ctx.stroke();
        ctx.setLineDash([]);
      } else if (state.lens === "boundaries" && state.index.violationSources.has(node.fileIndex)) {
        ctx.setLineDash([3 / transform.k, 3 / transform.k]);
        ctx.strokeStyle = theme.red;
        ctx.lineWidth = 1.4 / transform.k;
        ctx.stroke();
        ctx.setLineDash([]);
      }
      if (searching && matched) {
        ctx.strokeStyle = theme.amberText;
        ctx.lineWidth = 2 / transform.k;
        ctx.beginPath();
        ctx.arc(node.x, node.y, node.radius + 2 / transform.k, 0, Math.PI * 2);
        ctx.stroke();
      }
      // Hub ring from mid zoom (a bare ring at fit zoom reads as an
      // artifact); the xN count joins once there is room.
      if (
        file.importer_count >= gvs.hubFloor &&
        (kRel >= 1.2 || state.graphHovered === node.fileIndex)
      ) {
        ctx.globalAlpha = Math.max(alpha, 0.85);
        ctx.strokeStyle = theme.textLow;
        ctx.lineWidth = 1 / transform.k;
        ctx.beginPath();
        ctx.arc(node.x, node.y, node.radius + 3 / transform.k, 0, Math.PI * 2);
        ctx.stroke();
        if (kRel >= 1.5 || state.graphHovered === node.fileIndex) {
          ctx.font = FONT_MICRO;
          ctx.textAlign = "left";
          ctx.textBaseline = "middle";
          ctx.fillStyle = theme.textLow;
          ctx.fillText(
            `×${formatCount(file.importer_count)}`,
            node.x + node.radius + 6 / transform.k,
            node.y,
          );
        }
      }
    }
    ctx.globalAlpha = 1;
  }

  // Deep-zoom file labels: name the important dots once there is room.
  if (kRel >= 2 && state.graphHovered === null) {
    drawZoomLabels(state, gvs, w, h);
  }

  // Search pulse rings.
  if (gvs.pulseFile !== null) {
    const node = fileNodes[gvs.pulseFile];
    const age = performance.now() - gvs.pulseAt;
    if (node && node.x != null && node.y != null && age < 1200) {
      for (const phase of [0, 400]) {
        const t = (age - phase) / 800;
        if (t < 0 || t > 1) continue;
        ctx.beginPath();
        ctx.arc(node.x, node.y, node.radius + 4 + t * 26, 0, Math.PI * 2);
        ctx.strokeStyle = theme.blue;
        ctx.globalAlpha = 0.8 * (1 - t);
        ctx.lineWidth = 2 / transform.k;
        ctx.stroke();
      }
      ctx.globalAlpha = 1;
    } else {
      gvs.pulseFile = null;
    }
  }

  ctx.restore();

  // Labels join once the roads have flowed in (their internal alpha
  // handling would fight a global fade). While a file is hovered the
  // neighborhood labels own the foreground instead.
  if (reveal.labels > 0.35 && hovered === null) {
    drawRoadLabels(state, gvs);
    drawClusterLabels(state, gvs);
  }
  if (hovered !== null && neighbors !== null) {
    drawHoverLabels(state, gvs, hovered, hoverImporters, hoverImports, w, h);
  }
  drawAxisMarkers(state, gvs, usableStageWidth(state, w), h);
  drawCanvasLegend(state, w, h);
  drawPathTrace(state, gvs, w, h);

  drawMinimap(state, gvs, w, h);

  // Transient notice (fades after 1.8s).
  if (gvs.notice !== "") {
    const age = performance.now() - gvs.noticeAt;
    if (age < 1800) {
      ctx.font = FONT_SMALL;
      ctx.textAlign = "center";
      ctx.textBaseline = "top";
      ctx.fillStyle = theme.amberText;
      ctx.globalAlpha = age > 1400 ? 1 - (age - 1400) / 400 : 1;
      ctx.fillText(gvs.notice, w / 2, 28);
      ctx.globalAlpha = 1;
      cancelAnimationFrame(gvs.raf);
      gvs.raf = requestAnimationFrame(() => {
        if (state.view === "graph") renderGraph(state);
      });
    } else {
      gvs.notice = "";
    }
  }

  drawIntroCaptions(state, gvs, w);

  // Motion frames while something animates.
  const animating =
    (gvs.hoveredRoad !== null && !state.reducedMotion) ||
    gvs.pulseFile !== null ||
    reveal.t < 1 ||
    gvs.showIntro;
  if (animating) {
    cancelAnimationFrame(gvs.raf);
    gvs.raf = requestAnimationFrame(() => {
      if (state.view === "graph") renderGraph(state);
    });
  }
};
/** Wide background stroke behind a highlighted edge so it pops. */
const edgeUnderlay = (
  ctx: CanvasRenderingContext2D,
  a: Pt,
  b: Pt,
  bg: string,
  width: number,
): void => {
  ctx.beginPath();
  ctx.moveTo(a.x, a.y);
  ctx.lineTo(b.x, b.y);
  ctx.strokeStyle = bg;
  ctx.globalAlpha = 0.9;
  ctx.lineWidth = width;
  ctx.stroke();
};

/** One pass of raw file edges, filtered to intra- or inter-cluster. */
const drawFileEdges = (
  state: AppState,
  gvs: GraphViewState,
  sameCluster: boolean,
  alpha: number,
  lineWidth: number,
): void => {
  const { ctx, theme, data } = state;
  const { fileNodes } = gvs;
  ctx.strokeStyle = theme.textMuted;
  ctx.globalAlpha = alpha;
  ctx.lineWidth = lineWidth;
  ctx.beginPath();
  for (const [from, to] of data.edges) {
    const a = fileNodes[from];
    const b = fileNodes[to];
    if (!a || !b || (a.cluster === b.cluster) !== sameCluster) continue;
    if (a.x == null || a.y == null || b.x == null || b.y == null) continue;
    ctx.moveTo(a.x, a.y);
    ctx.lineTo(b.x, b.y);
  }
  ctx.stroke();
  ctx.globalAlpha = 1;
};

/** Greedy screen-space labels for the highest-degree files in view. */
const drawZoomLabels = (state: AppState, gvs: GraphViewState, w: number, h: number): void => {
  const { ctx, theme, data } = state;
  const { transform } = gvs;
  const candidates: Array<{ node: FileNode; degree: number }> = [];
  for (const node of gvs.fileNodes) {
    if (!node || node.x == null || node.y == null) continue;
    const sx = node.x * transform.k + transform.x;
    const sy = node.y * transform.k + transform.y;
    if (sx < -20 || sx > w + 20 || sy < -20 || sy > h + 20) continue;
    const file = data.files[node.fileIndex];
    candidates.push({ node, degree: file.importer_count + file.import_count });
  }
  candidates.sort((a, b) => b.degree - a.degree);

  ctx.font = FONT_SMALL;
  ctx.textAlign = "center";
  ctx.textBaseline = "top";
  const placed: Array<{ x: number; y: number; w: number; h: number }> = [];
  let drawn = 0;
  for (const { node } of candidates) {
    if (drawn >= 40) break;
    if (node.x == null || node.y == null) continue;
    const name = basename(data.files[node.fileIndex].path);
    const textW = ctx.measureText(name).width;
    const x = node.x;
    const y = node.y + node.radius + 2 / transform.k;
    // Occupancy check in screen space.
    const sx = x * transform.k + transform.x;
    const sy = y * transform.k + transform.y;
    const rect = { x: sx - textW / 2 - 2, y: sy, w: textW + 4, h: 13 };
    const overlaps = placed.some(
      (r) => rect.x < r.x + r.w && rect.x + rect.w > r.x && rect.y < r.y + r.h && rect.y + rect.h > r.y,
    );
    if (overlaps) continue;
    placed.push(rect);
    drawn++;
    // Draw in world space (crisper under the active transform); halo
    // instead of a knockout slab, matching the hover labels.
    const worldFont = 10 / transform.k;
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
const drawIntroCaptions = (state: AppState, gvs: GraphViewState, w: number): void => {
  if (!gvs.showIntro || gvs.revealAt <= 0) return;
  const { ctx, theme } = state;
  const elapsed = performance.now() - gvs.revealAt;
  // Three beats: the nouns, the lines, then the verbs (what to do next).
  const captions: Array<[number, number, string]> = [
    [0, 2600, "Every dot is a file · the shapes are folders"],
    [2600, 5200, "Lines are imports · the thick end is the importer"],
    [5200, 8600, "Click any dot for its story · keys 1-5 switch lenses"],
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
    // Backed chip so the caption reads over cluster labels behind it.
    chipRect(ctx, w / 2 - textW / 2 - 16, 12, textW + 32, 32, theme.bg, 1, theme.borderSubtle);
    ctx.fillStyle = theme.textHigh;
    ctx.fillText(text, w / 2, 28.5);
    ctx.globalAlpha = 1;
  }
};
/**
 * Screen-space neighbor labels on hover: fixed 10px regardless of
 * zoom, halo instead of knockout slabs, greedy occupancy across four
 * candidate slots with the docked tooltip pre-seeded as an exclusion
 * zone, and a +N chip for whatever did not fit.
 */
const drawHoverLabels = (
  state: AppState,
  gvs: GraphViewState,
  hovered: number,
  importers: Set<number>,
  imports: Set<number>,
  w: number,
  h: number,
): void => {
  const { ctx, theme } = state;
  const kRel = gvs.transform.k / gvs.fitK;
  const cap = kRel >= 1.2 ? 12 : 6;

  // Interleave importer and import lists (each degree-sorted) so
  // neither side monopolizes the cap.
  const byDegree = (idx: number): number =>
    state.data.files[idx].importer_count + state.data.files[idx].import_count;
  const a = [...importers].sort((x, y) => byDegree(y) - byDegree(x));
  const b = [...imports].sort((x, y) => byDegree(y) - byDegree(x));
  const ordered: number[] = [];
  for (let i = 0; i < Math.max(a.length, b.length); i++) {
    if (i < a.length) ordered.push(a[i]);
    if (i < b.length) ordered.push(b[i]);
  }

  const hoveredNode = gvs.fileNodes[hovered];
  if (!hoveredNode || hoveredNode.x == null || hoveredNode.y == null) return;
  const hs = worldToScreen(gvs, { x: hoveredNode.x, y: hoveredNode.y });
  const placed: Array<{ x: number; y: number; w: number; h: number }> = [
    fileTipCanvasRect(hs.x, hs.y, usableStageWidth(state, w), h),
  ];

  ctx.font = FONT_SMALL;
  ctx.textBaseline = "middle";
  ctx.lineJoin = "round";
  let drawn = 0;
  for (const idx of ordered) {
    if (drawn >= cap) break;
    const node = gvs.fileNodes[idx];
    if (!node || node.x == null || node.y == null) continue;
    const s = worldToScreen(gvs, { x: node.x, y: node.y });
    if (s.x < -20 || s.x > w + 20 || s.y < -20 || s.y > h + 20) continue;
    const name = middleTruncate(ctx, basename(state.data.files[idx].path), 140);
    const textW = ctx.measureText(name).width;
    const r = node.radius * gvs.transform.k + 3;
    const slots: Array<{ x: number; y: number; align: CanvasTextAlign }> = [
      { x: s.x, y: s.y + r + 9, align: "center" },
      { x: s.x, y: s.y - r - 9, align: "center" },
      { x: s.x + r + 5, y: s.y, align: "left" },
      { x: s.x - r - 5, y: s.y, align: "right" },
    ];
    for (const slot of slots) {
      const left = slot.align === "center" ? slot.x - textW / 2 : slot.align === "left" ? slot.x : slot.x - textW;
      const rect = { x: left - 4, y: slot.y - 9, w: textW + 8, h: 18 };
      if (rect.x < 4 || rect.x + rect.w > w - 4 || rect.y < 4 || rect.y + rect.h > h - 4) continue;
      if (placed.some((p) => rect.x < p.x + p.w && rect.x + rect.w > p.x && rect.y < p.y + p.h && rect.y + rect.h > p.y)) {
        continue;
      }
      ctx.textAlign = slot.align;
      ctx.strokeStyle = theme.bg;
      ctx.lineWidth = 3;
      ctx.globalAlpha = 0.92;
      ctx.strokeText(name, slot.x, slot.y);
      ctx.globalAlpha = 1;
      ctx.fillStyle = theme.textLow;
      ctx.fillText(name, slot.x, slot.y);
      placed.push(rect);
      drawn++;
      break;
    }
  }

  const total = importers.size + imports.size;
  if (total > drawn) {
    const label = `+${formatCount(total - drawn)} more · click for all`;
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

// ── Minimap ─────────────────────────────────────────────────────

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
  w: number,
  h: number,
): MinimapFrame | null => {
  if (gvs.clusters.length < 2) return null;
  // Keep clear of the detail panel when a road drill-down is open.
  const panelW = state.selectedRoad !== null ? Math.min(380, w * 0.9) : 0;
  const { minX, minY, maxX, maxY } = clusterBounds(gvs.clusters, () => true);
  const worldW = Math.max(1, maxX - minX);
  const worldH = Math.max(1, maxY - minY);
  const scale = Math.min((MINIMAP_W - 12) / worldW, (MINIMAP_H - 12) / worldH);
  return {
    x: w - panelW - MINIMAP_W - MINIMAP_MARGIN,
    y: h - MINIMAP_H - MINIMAP_MARGIN - 24,
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
  const w = stageEl ? stageEl.clientWidth : window.innerWidth;
  const h = stageEl ? stageEl.clientHeight : window.innerHeight;
  return minimapFrameAt(state, gvs, w, h);
};

const drawMinimap = (state: AppState, gvs: GraphViewState, w: number, h: number): void => {
  const { ctx, theme } = state;
  // Only earns pixels once the camera left the fit view.
  if (gvs.transform.k / gvs.fitK < 1.08) return;
  const frame = minimapFrameAt(state, gvs, w, h);
  if (!frame) return;

  ctx.fillStyle = theme.surface1;
  ctx.globalAlpha = 0.92;
  ctx.fillRect(frame.x, frame.y, frame.w, frame.h);
  ctx.globalAlpha = 1;
  ctx.strokeStyle = theme.borderDefault;
  ctx.lineWidth = 1;
  ctx.strokeRect(frame.x + 0.5, frame.y + 0.5, frame.w - 1, frame.h - 1);

  const toMini = (p: Pt): Pt => ({
    x: frame.x + (p.x - frame.worldX) * frame.scale,
    y: frame.y + (p.y - frame.worldY) * frame.scale,
  });

  // Cluster footprints, tangles in amber.
  for (const cluster of gvs.clusters) {
    const c = toMini({ x: cluster.cx, y: cluster.cy });
    const r = Math.max(1.5, cluster.r * frame.scale);
    ctx.beginPath();
    ctx.arc(c.x, c.y, r, 0, Math.PI * 2);
    ctx.fillStyle = cluster.tangle ? theme.amber : theme.borderStrong;
    ctx.globalAlpha = cluster.tangle ? 0.55 : 0.4;
    ctx.fill();
  }
  ctx.globalAlpha = 1;

  // Viewport rectangle (inverse of the active transform).
  const { transform } = gvs;
  const topLeft = toMini({ x: -transform.x / transform.k, y: -transform.y / transform.k });
  const bottomRight = toMini({
    x: (w - transform.x) / transform.k,
    y: (h - transform.y) / transform.k,
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
  const w = stageEl ? stageEl.clientWidth : window.innerWidth;
  const h = stageEl ? stageEl.clientHeight : window.innerHeight;
  const worldX = frame.worldX + (x - frame.x) / frame.scale;
  const worldY = frame.worldY + (y - frame.y) / frame.scale;
  const k = gvs.transform.k;
  select(state.canvas).call(
    gvs.zoomBehavior.transform,
    zoomIdentity.translate(w / 2 - worldX * k, h / 2 - worldY * k).scale(k),
  );
};
/** Path-trace overlay: dim the map, draw the dependency chain on top. */
const drawPathTrace = (state: AppState, gvs: GraphViewState, w: number, h: number): void => {
  const { ctx, theme, data } = state;

  if (gvs.pathFrom !== null && gvs.path === null) {
    const node = gvs.fileNodes[gvs.pathFrom];
    if (node && node.x != null && node.y != null) {
      const s = worldToScreen(gvs, { x: node.x, y: node.y });
      ctx.beginPath();
      ctx.arc(s.x, s.y, 10, 0, Math.PI * 2);
      ctx.strokeStyle = theme.blue;
      ctx.lineWidth = 2;
      ctx.setLineDash([4, 3]);
      ctx.stroke();
      ctx.setLineDash([]);
      ctx.font = FONT_MICRO;
      ctx.textAlign = "center";
      ctx.textBaseline = "top";
      ctx.fillStyle = theme.blueText;
      ctx.fillText("trace from here · shift-click a target", s.x, s.y + 16);
    }
    return;
  }

  const path = gvs.path;
  if (!path || path.length < 2) return;

  // Dim everything under the trace.
  ctx.fillStyle = theme.bg;
  ctx.globalAlpha = 0.62;
  ctx.fillRect(0, 0, w, h);
  ctx.globalAlpha = 1;

  const pts = path
    .map((idx) => gvs.fileNodes[idx])
    .filter((n) => n && n.x != null && n.y != null)
    .map((n) => worldToScreen(gvs, { x: n.x ?? 0, y: n.y ?? 0 }));
  if (pts.length < 2) return;

  ctx.beginPath();
  ctx.moveTo(pts[0].x, pts[0].y);
  for (let i = 1; i < pts.length; i++) ctx.lineTo(pts[i].x, pts[i].y);
  ctx.strokeStyle = theme.bg;
  ctx.lineWidth = 6;
  ctx.stroke();
  ctx.strokeStyle = theme.blue;
  ctx.lineWidth = 2;
  ctx.stroke();

  ctx.font = FONT_SMALL;
  ctx.textAlign = "center";
  ctx.textBaseline = "bottom";
  path.forEach((idx, i) => {
    const p = pts[i];
    if (!p) return;
    ctx.beginPath();
    ctx.arc(p.x, p.y, 5, 0, Math.PI * 2);
    ctx.fillStyle = i === 0 || i === path.length - 1 ? theme.blue : theme.textHigh;
    ctx.fill();
    const name = basename(data.files[idx].path);
    const textW = ctx.measureText(name).width;
    ctx.fillStyle = theme.bg;
    ctx.globalAlpha = 0.9;
    ctx.fillRect(p.x - textW / 2 - 3, p.y - 24, textW + 6, 14);
    ctx.globalAlpha = 1;
    ctx.fillStyle = theme.textHigh;
    ctx.fillText(name, p.x, p.y - 11);
  });

  ctx.font = FONT_MICRO;
  ctx.textAlign = "left";
  ctx.textBaseline = "top";
  ctx.fillStyle = theme.blueText;
  ctx.fillText(
    `dependency trace · ${path.length - 1} hop${path.length === 2 ? "" : "s"} · esc to clear`,
    14,
    28,
  );
};
const drawRoadLabels = (state: AppState, gvs: GraphViewState): void => {
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
    else if (state.lens === "boundaries" && road.bidi && road.cycleEdges > 0) ctx.fillStyle = theme.amberText;
    else ctx.fillStyle = theme.textLow;
    ctx.fillText(label, mid.x, mid.y + 0.5);
  }
};

const drawClusterLabels = (state: AppState, gvs: GraphViewState): void => {
  const { ctx, theme } = state;
  ctx.font = FONT_CHIP;
  ctx.textAlign = "left";
  ctx.textBaseline = "middle";
  const placed: Array<{ x: number; y: number; w: number; h: number }> = [];
  // Bigger clusters claim their spot first; smaller ones move below on overlap.
  const ordered = [...gvs.clusters].sort(
    (a, b) => b.indices.length - a.indices.length || (a.key < b.key ? -1 : 1),
  );
  const kRel = gvs.transform.k / gvs.fitK;
  for (const cluster of ordered) {
    if (cluster.isolated && !getGVS(state).standaloneOpen) continue;
    // Small multi-file clusters wait for mid zoom (their chips only add
    // collisions at fit); singletons keep their quiet borderless label
    // so no connected dot floats unexplained. On small maps every
    // cluster fits comfortably, so nothing is culled.
    const manyClusters = gvs.clusters.filter((c) => !c.isolated).length > 10;
    if (manyClusters && cluster.indices.length >= 2 && cluster.indices.length < 6 && kRel < 1.5) {
      continue;
    }
    let topLeft = cluster.hull[0] ?? { x: cluster.cx, y: cluster.cy };
    for (const p of cluster.hull) {
      if (p.y < topLeft.y || (p.y === topLeft.y && p.x < topLeft.x)) topLeft = p;
    }
    const s = worldToScreen(gvs, topLeft);
    // Single-file clusters: just the filename, borderless dim text. The
    // full path lives in the tooltip; quiet labels collide far less.
    const single = cluster.indices.length === 1;
    const raw = single
      ? basename(state.data.files[cluster.indices[0]].path)
      : cluster.key;
    const label = middleTruncate(ctx, raw.toUpperCase(), 210);
    const sub = single ? "" : `${formatCount(cluster.indices.length)} files`;
    const labelW = ctx.measureText(label).width;
    const subW = sub ? ctx.measureText(sub).width : -8;
    const boxW = labelW + subW + 17;
    // Clamp inside the viewport so edge clusters keep readable chips.
    const x = Math.min(
      Math.max(6, s.x - 4),
      usableStageWidth(state, state.canvas.clientWidth) - boxW - 8,
    );
    let y = s.y - 12;
    for (let tries = 0; tries < 6; tries++) {
      const overlaps = placed.some(
        (r) => x < r.x + r.w && x + boxW > r.x && y - 9 < r.y + r.h && y + 9 > r.y,
      );
      if (!overlaps) break;
      y += 19;
    }
    placed.push({ x: x - 4, y: y - 10, w: boxW + 3, h: 20 });
    if (state.search.trim() !== "") ctx.globalAlpha = 0.35;
    chipRect(
      ctx,
      x - 4,
      y - 10,
      labelW + subW + 18,
      20,
      theme.bg,
      single ? 0.75 : 0.92,
      single ? null : cluster.tangle && state.lens === "boundaries" ? theme.amber : theme.borderSubtle,
    );
    ctx.fillStyle = cluster.isolated || single ? theme.textMuted : theme.textLow;
    ctx.fillText(label, x + 2, y + 0.5);
    if (sub) {
      ctx.fillStyle = theme.textMuted;
      ctx.fillText(sub, x + labelW + 8, y + 0.5);
    }
    ctx.globalAlpha = 1;
  }

  // Standalone toggle: a fixed chip docked above the canvas legend so it
  // never floats orphaned in world space. When open, a caption sits by
  // the revealed strip itself.
  const isolated = gvs.clusters.filter((c) => c.isolated);
  gvs.standaloneChip = null;
  if (isolated.length > 0) {
    const h = state.canvas.clientHeight;
    const fileCount = isolated.reduce((sum, c) => sum + c.indices.length, 0);
    ctx.font = FONT_MICRO;
    ctx.textAlign = "left";
    ctx.textBaseline = "middle";
    const label = gvs.standaloneOpen
      ? "hide standalone files"
      : `standalone · ${formatCount(fileCount)} files · nothing imports them`;
    const textW = ctx.measureText(label).width;
    const cx0 = 12;
    const cy0 = h - 56;
    chipRect(ctx, cx0, cy0, textW + 16, 22, theme.bg, 0.9, theme.borderSubtle);
    ctx.fillStyle = theme.textMuted;
    ctx.fillText(label, cx0 + 8, cy0 + 11.5);
    gvs.standaloneChip = { x: cx0, y: cy0, w: textW + 16, h: 22 };
    if (gvs.standaloneOpen) {
      const minX = Math.min(...isolated.map((c) => c.cx - c.r));
      const minY = Math.min(...isolated.map((c) => c.cy - c.r));
      const s = worldToScreen(gvs, { x: minX, y: minY });
      ctx.fillStyle = theme.textMuted;
      ctx.globalAlpha = 0.7;
      ctx.fillText("STANDALONE · configs & CI · nothing imports them", s.x, s.y - 26);
      ctx.globalAlpha = 1;
    }
  }
};

/** Screen-space axis endpoints: the one annotation that explains the x layout. */
const drawAxisMarkers = (state: AppState, gvs: GraphViewState, w: number, h: number): void => {
  // The entry-to-shared axis only describes the folder rank layout;
  // import communities carry no left-right meaning.
  if (gvs.clusterMode === "imports") return;
  const kRel = gvs.transform.k / gvs.fitK;
  if (kRel > 1.3 || state.selected !== null) return;
  const { ctx, theme } = state;
  ctx.font = FONT_SMALL;
  ctx.textBaseline = "middle";
  ctx.fillStyle = theme.textLow;
  ctx.globalAlpha = 0.85;
  ctx.textAlign = "left";
  ctx.fillText("ENTRY CODE", 14, h / 2);
  ctx.textAlign = "right";
  ctx.fillText("SHARED FOUNDATIONS", w - 14, h / 2);
  ctx.globalAlpha = 1;
};

const drawCanvasLegend = (state: AppState, w: number, h: number): void => {
  const { ctx, theme } = state;
  const text = legendText(state.lens, state.data, "graph");
  if (text === "") return;
  ctx.font = FONT_LEGEND;
  ctx.textAlign = "left";
  ctx.textBaseline = "middle";
  const textW = ctx.measureText(text).width;
  ctx.fillStyle = theme.bg;
  ctx.globalAlpha = 0.85;
  ctx.fillRect(10, h - 27, textW + 12, 19);
  ctx.globalAlpha = 0.9;
  ctx.fillStyle = theme.textLow;
  ctx.fillText(text, 16, h - 17.5);
  ctx.globalAlpha = 1;
  void w;
};

const drawSeverityEdges = (state: AppState, gvs: GraphViewState): void => {
  const { ctx, theme, data } = state;
  const n = data.files.length;
  const k = gvs.transform.k;
  for (const [from, to] of data.edges) {
    const packed = from * n + to;
    const isViolation = state.index.violationEdges.has(packed);
    const isCycle = state.index.cycleEdges.has(packed);
    if (!isViolation && !isCycle) continue;
    const a = gvs.fileNodes[from];
    const b = gvs.fileNodes[to];
    if (!a || !b || a.x == null || a.y == null || b.x == null || b.y == null) continue;
    ctx.beginPath();
    ctx.moveTo(a.x, a.y);
    ctx.lineTo(b.x, b.y);
    ctx.strokeStyle = theme.bg;
    ctx.lineWidth = 3 / k;
    ctx.globalAlpha = 0.9;
    ctx.setLineDash([]);
    ctx.stroke();
    ctx.strokeStyle = isViolation ? theme.red : theme.amber;
    ctx.lineWidth = 1.4 / k;
    if (isCycle && !isViolation) ctx.setLineDash([4 / k, 3 / k]);
    ctx.stroke();
    ctx.setLineDash([]);
    ctx.globalAlpha = 1;
  }
};
