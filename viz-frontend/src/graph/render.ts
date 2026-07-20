/**
 * Everything the graph view paints: the overview scene (hulls, roads,
 * nodes, labels, legend, axis, minimap, intro captions, hover
 * neighborhood, path trace) and the ego stage with its columns and
 * breadcrumbs.
 */
import type { AppState } from "../state";
import { formatCount, lensColor, lensFindingLevel } from "../data";
import { mix } from "../theme";
import { renderEgoStage, renderGhost } from "./ego";
import { drawMinimap } from "./minimap";
import {
  drawCanvasLegend,
  drawClusterLabels,
  drawHoverLabels,
  drawIntroCaptions,
  drawPathTrace,
  drawRoadLabels,
  drawZoomLabels,
} from "./annotations";
import {
  type ClusterInfo,
  type FileNode,
  type GraphViewState,
  type Pt,
  FONT_MICRO,
  FONT_SMALL,
  LOD_INTER,
  LOD_INTRA,
  LOD_SEVERITY,
  easeOut,
  getGVS,
  hullPath,
  isTestCluster,
  roadGeometry,
  roadWidth,
  taperedRibbon,
  usableStageWidth,
} from "./shared";

export const renderGraph = (state: AppState): void => {
  const { canvas, ctx, theme } = state;
  const gvs = getGVS(state);
  if (!gvs.initialized) return;

  // Re-read per render: the window can move to a display with a
  // different pixel ratio mid-session. Keep state.dpr in sync for
  // any consumer that sizes against the backing store.
  const dpr = window.devicePixelRatio || 1;
  state.dpr = dpr;

  const stageEl = canvas.parentElement;
  const width = stageEl ? stageEl.clientWidth : window.innerWidth;
  const height = stageEl ? stageEl.clientHeight : window.innerHeight;
  const pw = Math.round(width * dpr);
  const ph = Math.round(height * dpr);
  if (canvas.width !== pw || canvas.height !== ph) {
    canvas.style.width = `${width}px`;
    canvas.style.height = `${height}px`;
    canvas.width = pw;
    canvas.height = ph;
  }

  ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
  ctx.fillStyle = theme.bg;
  ctx.fillRect(0, 0, width, height);

  if (state.selected !== null && gvs.fileNodes[state.selected]) {
    renderGhost(state, gvs);
    // The stage reports whether its choreography still runs; this
    // module owns the animation loop.
    if (renderEgoStage(state, gvs, width, height)) {
      cancelAnimationFrame(gvs.raf);
      gvs.raf = requestAnimationFrame(() => {
        if (state.view === "graph") renderGraph(state);
      });
    }
  } else {
    gvs.stageRects = [];
    gvs.lastRoot = null;
    renderOverview(state, gvs, width, height);
  }
};
// ── Overview ────────────────────────────────────────────────────

/** Opening choreography: layers sweep in left to right, then the roads. */
const REVEAL_LAYER_MS = 110;
const REVEAL_FADE_MS = 380;
/** Graph node lens-color crossfade duration, matching the treemap's. */
const GRAPH_LENS_MS = 200;

const revealProgress = (
  gvs: GraphViewState,
  reduced: boolean,
): {
  progress: number;
  cluster: (cluster: ClusterInfo) => number;
  roads: number;
  labels: number;
} => {
  if (gvs.revealAt === 0) gvs.revealAt = reduced ? -1 : performance.now();
  if (gvs.revealAt < 0) {
    return { progress: 1, cluster: () => 1, roads: 1, labels: 1 };
  }
  const elapsed = performance.now() - gvs.revealAt;
  const maxLayer = gvs.clusters.reduce(
    (max, cluster) => Math.max(max, cluster.isolated ? 0 : cluster.layer),
    0,
  );
  const total = (maxLayer + 1) * REVEAL_LAYER_MS + REVEAL_FADE_MS + 420;
  const progress = Math.min(1, elapsed / total);
  const clusterAlpha = (cluster: ClusterInfo): number => {
    const start = (cluster.isolated ? maxLayer + 1 : cluster.layer) * REVEAL_LAYER_MS;
    return easeOut(Math.min(1, Math.max(0, (elapsed - start) / REVEAL_FADE_MS)));
  };
  const roadsStart = (maxLayer + 1) * REVEAL_LAYER_MS * 0.6;
  const roads = easeOut(Math.min(1, Math.max(0, (elapsed - roadsStart) / (REVEAL_FADE_MS + 200))));
  const labelsStart = roadsStart + 180;
  const labels = easeOut(Math.min(1, Math.max(0, (elapsed - labelsStart) / REVEAL_FADE_MS)));
  return { progress, cluster: clusterAlpha, roads, labels };
};

/** Per-frame context every overview phase shares. */
interface Scene {
  state: AppState;
  gvs: GraphViewState;
  kRel: number;
  reveal: ReturnType<typeof revealProgress>;
  searching: boolean;
  /** Lens-color crossfade progress (1 = settled, no fade in flight). */
  lensT: number;
}

/** Direct hover context produced by the neighborhood phase. */
interface HoverContext {
  hovered: number | null;
  neighbors: Set<number> | null;
  importers: Set<number>;
  imports: Set<number>;
}

/** Guard + path setup shared by every hull pass. */
const forEachHull = (scene: Scene, draw: (cluster: ClusterInfo) => void): void => {
  const { state, gvs } = scene;
  for (const cluster of gvs.clusters) {
    if (cluster.isolated && !gvs.standaloneOpen) continue;
    if (cluster.hull.length < 3) continue;
    state.ctx.beginPath();
    hullPath(state.ctx, cluster.hull);
    draw(cluster);
  }
};

const drawHullFills = (scene: Scene): void => {
  const { state, reveal } = scene;
  const { ctx, theme } = state;
  forEachHull(scene, (cluster) => {
    ctx.fillStyle = theme.surface2;
    ctx.globalAlpha = 0.9 * reveal.cluster(cluster) * (state.graphHovered !== null ? 0.6 : 1);
    ctx.fill();
    ctx.globalAlpha = 1;
  });
};

// Individual file edges are LOD-gated. Inter-cluster edges (deep zoom) join
// the roads BEHIND the hulls; a cluster's own intra-cluster edges (mid zoom)
// draw on top of its hull so the internal wiring stays visible.
const drawInterEdges = (scene: Scene): void => {
  const { state, gvs, kRel } = scene;
  if (kRel >= LOD_INTER) {
    drawFileEdges(state, gvs, false, 0.1, 0.8 / gvs.transform.k);
  }
};

const drawIntraEdges = (scene: Scene): void => {
  const { state, gvs, kRel } = scene;
  if (kRel >= LOD_INTRA) {
    drawFileEdges(state, gvs, true, 0.12, 1 / gvs.transform.k);
  }
};

const drawHullBorders = (scene: Scene): void => {
  const { state, gvs, reveal } = scene;
  const { ctx, theme } = state;
  forEachHull(scene, (cluster) => {
    const showTangle = cluster.tangle && state.lens === "boundaries";
    ctx.strokeStyle = showTangle ? theme.amber : theme.borderDefault;
    ctx.globalAlpha = (showTangle ? 0.7 : 0.6) * reveal.cluster(cluster);
    ctx.lineWidth = 1 / gvs.transform.k;
    ctx.stroke();
    ctx.globalAlpha = 1;
  });
};

/** Roads with severity overdraw, plus the focused road highlight. */
const drawRoads = (scene: Scene): void => {
  const { state, gvs, kRel, reveal } = scene;
  const { ctx, theme } = state;
  const { transform, clusters, roads } = gvs;
  const hoverDim = state.graphHovered !== null ? 0.35 : 1;
  // Roads: tapered ribbons, wide at importer, narrow at imported.
  // At fit zoom the ribbons carry the whole story, so hold a minimum
  // on-screen width and lift the alpha; both relax as the user zooms in.
  const roadBoost = Math.min(1, Math.max(0, 1.6 - kRel));
  const minRoadW = 1.8 / transform.k;
  // High-traffic roads get promoted a step so the trunk routes survive
  // a projector; the threshold is the 75th percentile of bundle sizes.
  const roadCounts = roads.map((road) => road.count).toSorted((left, right) => left - right);
  const trunkFloor =
    roadCounts.length > 0 ? roadCounts[Math.floor(roadCounts.length * 0.75)] : Infinity;
  // With many clusters the pairwise roads become a mesh at fit zoom, so show
  // only the strongest ones there and reveal the rest as the user zooms in
  // (fully by kRel 1.6, where individual edges begin). Severity roads always
  // show so the boundaries story is never hidden.
  const manyClusters = clusters.length > 20;
  const zoomRelax = Math.min(1, Math.max(0, (kRel - 1) / 0.6));
  // The more clusters, the denser the mesh, so thin harder at fit zoom: a
  // 30-cluster map keeps ~its top third of roads, a 90-cluster map only its
  // top ~sixth. The floor relaxes to 0 as the user zooms in.
  const basePct = manyClusters ? Math.min(0.85, 0.4 + clusters.length / 150) : 0;
  const floorPct = basePct * (1 - zoomRelax);
  const roadFloor =
    floorPct > 0 && roadCounts.length > 0
      ? roadCounts[Math.min(roadCounts.length - 1, Math.floor(roadCounts.length * floorPct))]
      : 0;
  // Hovering a cluster label lights up every road touching it and dims the
  // rest so the cluster's dependency fan reads at a glance.
  const litCluster = gvs.hoveredCluster;
  for (const road of roads) {
    const severity = road.violations > 0 || (road.bidi && road.cycleEdges > 0);
    const lit = litCluster !== null && (road.src === litCluster || road.dst === litCluster);
    if (road.count < roadFloor && !severity && !lit) continue;
    const { p0, p1, p2, p3 } = roadGeometry(gvs, road);
    const wSrc = Math.max(minRoadW, roadWidth(road.count));
    ctx.beginPath();
    const thinRatio = road.count >= trunkFloor ? 0.15 : 0.22;
    taperedRibbon(ctx, p0, p1, p2, p3, wSrc, Math.max(0.5, wSrc * thinRatio));
    ctx.fillStyle = lit ? theme.blueText : theme.textLow;
    // Test-to-source imports are the least interesting overview signal
    // but the biggest bundles; keep them recessive so source roads lead.
    const testDim = isTestCluster(clusters[road.src].key) ? 0.4 : 1;
    const trunk = road.count >= trunkFloor && testDim === 1 ? 0.22 : 0;
    let alpha = (0.3 + 0.18 * roadBoost + trunk) * testDim * reveal.roads * hoverDim;
    if (litCluster !== null) alpha = lit ? Math.min(1, alpha + 0.55) : alpha * 0.18;
    ctx.globalAlpha = alpha;
    ctx.fill();
    ctx.globalAlpha = 1;

    // Severity overdraw parallel to the road (boundaries lens only;
    // the overview stays neutral until the user asks a question).
    if (
      state.lens === "boundaries" &&
      (road.violations > 0 || (road.bidi && road.cycleEdges > 0))
    ) {
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
};

/** Direction-encoded edges to the hovered file's direct neighbors. */
const drawHoverNeighborhood = (scene: Scene): HoverContext => {
  const { state, gvs } = scene;
  const { ctx, theme } = state;
  const { transform, fileNodes } = gvs;
  // Hover neighborhood. Direction is dual-encoded: files importing the
  // hovered one arrive as solid blue ribbons (thick end at the
  // importer, same rule as roads); its own imports leave as thin
  // dashed blue lines. The adjacency index already carries exactly the
  // hovered file's neighbors per direction: O(degree), not O(edges).
  const hovered = state.graphHovered;
  let neighbors: Set<number> | null = null;
  const hoverImporters = new Set<number>();
  const hoverImports = new Set<number>();
  if (hovered !== null) {
    neighbors = new Set([hovered]);
    const target = fileNodes[hovered];
    for (const from of state.index.importersOf[hovered]) {
      if (from === hovered) continue;
      neighbors.add(from);
      hoverImporters.add(from);
      const importerNode = fileNodes[from];
      if (!importerNode || !target) continue;
      if (importerNode.x == null || importerNode.y == null || target.x == null || target.y == null)
        continue;
      edgeUnderlay(
        ctx,
        { x: importerNode.x, y: importerNode.y },
        { x: target.x, y: target.y },
        theme.bg,
        4 / transform.k,
      );
      const p0 = { x: importerNode.x, y: importerNode.y };
      const p3 = { x: target.x, y: target.y };
      const p1 = { x: p0.x + (p3.x - p0.x) / 3, y: p0.y + (p3.y - p0.y) / 3 };
      const p2 = { x: p0.x + ((p3.x - p0.x) * 2) / 3, y: p0.y + ((p3.y - p0.y) * 2) / 3 };
      ctx.beginPath();
      taperedRibbon(ctx, p0, p1, p2, p3, 2.4 / transform.k, 0.6 / transform.k);
      ctx.fillStyle = theme.blue;
      ctx.globalAlpha = 0.9;
      ctx.fill();
    }
    for (const to of state.index.importsOf[hovered]) {
      if (to === hovered) continue;
      neighbors.add(to);
      hoverImports.add(to);
      const importedNode = fileNodes[to];
      if (!target || !importedNode) continue;
      if (target.x == null || target.y == null || importedNode.x == null || importedNode.y == null)
        continue;
      edgeUnderlay(
        ctx,
        { x: target.x, y: target.y },
        { x: importedNode.x, y: importedNode.y },
        theme.bg,
        4 / transform.k,
      );
      ctx.beginPath();
      ctx.moveTo(target.x, target.y);
      ctx.lineTo(importedNode.x, importedNode.y);
      ctx.strokeStyle = theme.blue;
      ctx.globalAlpha = 0.45;
      ctx.lineWidth = 1.1 / transform.k;
      ctx.setLineDash([4 / transform.k, 3 / transform.k]);
      ctx.stroke();
      ctx.setLineDash([]);
    }
    ctx.globalAlpha = 1;
  }

  return { hovered, neighbors, importers: hoverImporters, imports: hoverImports };
};

/** Every file dot with its lens color, rings, badges, and dim states. */
/** Color and alpha for one overview node. */
interface NodeAppearance {
  color: string;
  alpha: number;
  matched: boolean;
  inReach: boolean;
  dimmed: boolean;
}

/**
 * Resolve a node's color and alpha from the lens, the lens crossfade, the
 * search state, the hover neighborhood, and the reveal, or null when the
 * node is effectively invisible and should be skipped. Pure (no drawing),
 * so the blended-visibility logic is isolated from the paint loop.
 */
const nodeAppearance = (
  scene: Scene,
  hover: HoverContext,
  node: FileNode,
): NodeAppearance | null => {
  const { state, gvs, reveal, searching } = scene;
  const { theme } = state;
  const file = state.data.files[node.fileIndex];
  let color = lensColor(state.lens, theme, state.index, file);
  // Crossfade node colors on a lens switch, matching the treemap.
  if (gvs.lensPrev && scene.lensT < 1) {
    const prev = gvs.lensPrev.get(node.fileIndex);
    if (prev && prev !== color) color = mix(prev, color, scene.lensT);
  }
  const recessive = color === theme.cellNeutral || color === theme.cellEntry;
  const matched = !searching || state.searchMatches.has(node.fileIndex);
  const inReach = searching && state.searchReach.has(node.fileIndex);
  const isNeighbor = hover.neighbors?.has(node.fileIndex) ?? false;
  const dimmed = hover.neighbors !== null && !isNeighbor;

  let alpha = recessive ? 0.82 : 0.95;
  if (dimmed) alpha = 0.16;
  // Files reachable from the matched set stay legible (the combined blast
  // radius); everything else recedes.
  if (searching && !matched) alpha = Math.min(alpha, inReach ? 0.5 : 0.1);
  if (isNeighbor) alpha = 1;
  alpha *= reveal.cluster(gvs.clusters[node.cluster]);
  if (alpha <= 0.01) return null;
  return { color, alpha, matched, inReach, dimmed };
};

const drawNodes = (scene: Scene, hover: HoverContext, width: number, height: number): void => {
  const { state, gvs, kRel, searching } = scene;
  const { ctx, theme, data } = state;
  const { transform, clusters, fileNodes } = gvs;
  const files = data.files;
  const { importers: hoverImporters, imports: hoverImports } = hover;
  // Nodes.
  for (const node of fileNodes) {
    if (!node || node.x == null || node.y == null) continue;
    if (clusters[node.cluster].isolated && !gvs.standaloneOpen) continue;
    const look = nodeAppearance(scene, hover, node);
    if (!look) continue;
    const file = files[node.fileIndex];
    const { color, alpha, matched, inReach, dimmed } = look;

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
      // Non-color finding channel, mirroring the treemap hatch: severe
      // findings ring the dot solidly, mild ones with a dash, so lens
      // findings survive with the fill color removed. The overview lens
      // is always level 0 and pays only the switch dispatch.
      const level = lensFindingLevel(state.lens, state.index, file, node.fileIndex);
      if (level > 0) {
        ctx.strokeStyle = theme.textHigh;
        ctx.lineWidth = 1 / transform.k;
        if (level === 1) ctx.setLineDash([2 / transform.k, 2 / transform.k]);
        ctx.beginPath();
        ctx.arc(node.x, node.y, node.radius + 1.5 / transform.k, 0, Math.PI * 2);
        ctx.stroke();
        ctx.setLineDash([]);
      }
      if (searching && matched) {
        ctx.strokeStyle = theme.amberText;
        ctx.lineWidth = 2 / transform.k;
        ctx.beginPath();
        ctx.arc(node.x, node.y, node.radius + 2 / transform.k, 0, Math.PI * 2);
        ctx.stroke();
      } else if (inReach) {
        ctx.strokeStyle = theme.blue;
        ctx.globalAlpha = 0.6;
        ctx.lineWidth = 1 / transform.k;
        ctx.beginPath();
        ctx.arc(node.x, node.y, node.radius + 1.5 / transform.k, 0, Math.PI * 2);
        ctx.stroke();
        ctx.globalAlpha = alpha;
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
    drawZoomLabels(state, gvs, width, height);
  }
};

const drawSearchPulse = (scene: Scene): void => {
  const { state, gvs } = scene;
  const { ctx, theme } = state;
  const { transform, fileNodes } = gvs;
  // Reduced motion gets no expanding rings; the camera still centers.
  if (state.reducedMotion) return;
  // Search pulse rings.
  if (gvs.pulseFile !== null) {
    const node = fileNodes[gvs.pulseFile];
    const age = performance.now() - gvs.pulseAt;
    if (node && node.x != null && node.y != null && age < 1200) {
      for (const phase of [0, 400]) {
        const progress = (age - phase) / 800;
        if (progress < 0 || progress > 1) continue;
        ctx.beginPath();
        ctx.arc(node.x, node.y, node.radius + 4 + progress * 26, 0, Math.PI * 2);
        ctx.strokeStyle = theme.blue;
        ctx.globalAlpha = 0.8 * (1 - progress);
        ctx.lineWidth = 2 / transform.k;
        ctx.stroke();
      }
      ctx.globalAlpha = 1;
    } else {
      gvs.pulseFile = null;
    }
  }
};

const renderOverview = (
  state: AppState,
  gvs: GraphViewState,
  width: number,
  height: number,
): void => {
  const { ctx, theme } = state;
  const { transform } = gvs;
  const kRel = transform.k / gvs.fitK;
  const reveal = revealProgress(gvs, state.reducedMotion);
  const lensT =
    state.reducedMotion || gvs.lensFadeAt <= 0
      ? 1
      : easeOut(Math.min(1, (performance.now() - gvs.lensFadeAt) / GRAPH_LENS_MS));
  if (lensT >= 1) {
    gvs.lensPrev = null;
    gvs.lensFadeAt = 0;
  }
  const scene: Scene = {
    state,
    gvs,
    kRel,
    reveal,
    searching: state.search.trim() !== "",
    lensT,
  };

  ctx.save();
  ctx.translate(transform.x, transform.y);
  ctx.scale(transform.k, transform.k);

  // Connective tissue first, so cross-cluster roads and edges sit BEHIND the
  // hulls instead of crossing over them; a cluster's own internal wiring
  // draws back on top of its hull, below the nodes.
  drawRoads(scene);
  drawInterEdges(scene);
  drawHullFills(scene);
  drawHullBorders(scene);
  drawIntraEdges(scene);
  const hover = drawHoverNeighborhood(scene);
  drawNodes(scene, hover, width, height);
  drawSearchPulse(scene);

  ctx.restore();

  // Labels join once the roads have flowed in (their internal alpha
  // handling would fight a global fade). While a file is hovered the
  // neighborhood labels own the foreground instead.
  if (reveal.labels > 0.35 && hover.hovered === null) {
    drawRoadLabels(state, gvs);
    drawClusterLabels(state, gvs);
  }
  if (hover.hovered !== null && hover.neighbors !== null) {
    drawHoverLabels(state, gvs, hover.hovered, hover.importers, hover.imports, width, height);
  }
  drawCanvasLegend(state, width, height);
  drawPathTrace(state, gvs, width, height);

  drawMinimap(state, gvs, width, height);

  // Transient notice (fades after 1.8s).
  if (gvs.notice !== "") {
    const age = performance.now() - gvs.noticeAt;
    if (age < 1800) {
      ctx.font = FONT_SMALL;
      ctx.textAlign = "center";
      ctx.textBaseline = "top";
      ctx.fillStyle = theme.amberText;
      ctx.globalAlpha = age > 1400 ? 1 - (age - 1400) / 400 : 1;
      ctx.fillText(gvs.notice, usableStageWidth(state, width) / 2, 28);
      ctx.globalAlpha = 1;
      cancelAnimationFrame(gvs.raf);
      gvs.raf = requestAnimationFrame(() => {
        if (state.view === "graph") renderGraph(state);
      });
    } else {
      gvs.notice = "";
    }
  }

  drawIntroCaptions(state, gvs, width);

  // Motion frames while something animates.
  const animating =
    (gvs.hoveredRoad !== null && !state.reducedMotion) ||
    (gvs.pulseFile !== null && !state.reducedMotion) ||
    scene.lensT < 1 ||
    reveal.progress < 1 ||
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
  start: Pt,
  end: Pt,
  bg: string,
  width: number,
): void => {
  ctx.beginPath();
  ctx.moveTo(start.x, start.y);
  ctx.lineTo(end.x, end.y);
  ctx.strokeStyle = bg;
  ctx.globalAlpha = 0.9;
  ctx.lineWidth = width;
  ctx.stroke();
};

/** One pass of raw file edges from the precomputed cluster partition. */
const drawFileEdges = (
  state: AppState,
  gvs: GraphViewState,
  sameCluster: boolean,
  alpha: number,
  lineWidth: number,
): void => {
  const { ctx, theme } = state;
  const { fileNodes } = gvs;
  const edges = sameCluster ? gvs.intraEdges : gvs.interEdges;
  ctx.strokeStyle = theme.textMuted;
  ctx.globalAlpha = alpha;
  ctx.lineWidth = lineWidth;
  ctx.beginPath();
  for (const [from, to] of edges) {
    const fromNode = fileNodes[from];
    const toNode = fileNodes[to];
    if (!fromNode || !toNode) continue;
    if (fromNode.x == null || fromNode.y == null || toNode.x == null || toNode.y == null) continue;
    ctx.moveTo(fromNode.x, fromNode.y);
    ctx.lineTo(toNode.x, toNode.y);
  }
  ctx.stroke();
  ctx.globalAlpha = 1;
};

const drawSeverityEdges = (state: AppState, gvs: GraphViewState): void => {
  const { ctx, theme, data } = state;
  const fileCount = data.files.length;
  const scale = gvs.transform.k;
  for (const [from, to] of data.edges) {
    const packed = from * fileCount + to;
    const isViolation = state.index.violationEdges.has(packed);
    const isCycle = state.index.cycleEdges.has(packed);
    if (!isViolation && !isCycle) continue;
    const fromNode = gvs.fileNodes[from];
    const toNode = gvs.fileNodes[to];
    if (
      !fromNode ||
      !toNode ||
      fromNode.x == null ||
      fromNode.y == null ||
      toNode.x == null ||
      toNode.y == null
    )
      continue;
    ctx.beginPath();
    ctx.moveTo(fromNode.x, fromNode.y);
    ctx.lineTo(toNode.x, toNode.y);
    ctx.strokeStyle = theme.bg;
    ctx.lineWidth = 3 / scale;
    ctx.globalAlpha = 0.9;
    ctx.setLineDash([]);
    ctx.stroke();
    ctx.strokeStyle = isViolation ? theme.red : theme.amber;
    ctx.lineWidth = 1.4 / scale;
    if (isCycle && !isViolation) ctx.setLineDash([4 / scale, 3 / scale]);
    ctx.stroke();
    ctx.setLineDash([]);
    ctx.globalAlpha = 1;
  }
};
