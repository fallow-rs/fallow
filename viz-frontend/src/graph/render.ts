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
  roadDensityFloor,
  octilinearBend,
  roadGeometry,
  roadIsDrawn,
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
/** Graph node lens-color ripple: total duration and the share spent staggering. */
const GRAPH_LENS_MS = 640;
const GRAPH_LENS_REDUCED_MS = 180;
const GRAPH_LENS_SPREAD = 0.55;
/** Hover focus: how long the rest of the map takes to recede. */
const HOVER_DIM_MS = 160;
/** Opacity of nodes outside the hovered neighborhood once the dim settles. */
const HOVER_DIM_ALPHA = 0.16;

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
  /** Linear lens-color fade progress (1 = settled, no fade in flight). */
  lensT: number;
  /** Screen point the lens ripple spreads from, and its farthest reach. */
  rippleCenter: { x: number; y: number };
  rippleReach: number;
  /** 0..1 progress of the hover dim settling in. */
  dimT: number;
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
  const hoverDim = state.graphHovered !== null ? 1 - 0.4 * scene.dimT : 1;
  forEachHull(scene, (cluster) => {
    ctx.fillStyle = theme.surface2;
    ctx.globalAlpha = 0.9 * reveal.cluster(cluster) * hoverDim;
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
    const showTangle = cluster.tangle && state.lens === "architecture";
    ctx.strokeStyle = showTangle ? theme.amber : theme.borderDefault;
    ctx.globalAlpha = (showTangle ? 0.7 : 0.6) * reveal.cluster(cluster);
    ctx.lineWidth = 1 / gvs.transform.k;
    ctx.stroke();
    ctx.globalAlpha = 1;
  });
};

/** Roads with severity overdraw, plus the focused road highlight. */
/**
 * Start a new path holding a road's tapered ribbon: full width at the
 * importer, thinning toward the imported folder (thinner still on trunks).
 */
const traceRoadRibbon = (
  ctx: CanvasRenderingContext2D,
  route: { p0: Pt; p1: Pt; p2: Pt; p3: Pt },
  wSrc: number,
  trunk: boolean,
): void => {
  ctx.beginPath();
  const thinRatio = trunk ? 0.15 : 0.22;
  taperedRibbon(ctx, route.p0, route.p1, route.p2, route.p3, wSrc, Math.max(0.5, wSrc * thinRatio));
};

const drawRoads = (scene: Scene): void => {
  const { state, gvs, kRel, reveal } = scene;
  const { ctx, theme } = state;
  const { transform, clusters, roads } = gvs;
  const hoverDim = state.graphHovered !== null ? 1 - 0.65 * scene.dimT : 1;
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
  const roadFloor = roadDensityFloor(gvs);
  // Hovering a cluster label lights up every road touching it and dims the
  // rest so the cluster's dependency fan reads at a glance.
  const litCluster = gvs.hoveredCluster;
  // Census focus: a focused road keeps its weight and the others fade to
  // 18%, so its path reads end to end through the mesh.
  const focusRoad = gvs.hoveredRoad ?? gvs.selectedRoad;
  for (const [roadIndex, road] of roads.entries()) {
    const lit = litCluster !== null && (road.src === litCluster || road.dst === litCluster);
    if (!roadIsDrawn(gvs, road, roadFloor)) continue;
    const { p0, p1, p2, p3 } = roadGeometry(gvs, road);
    const wSrc = Math.max(minRoadW, roadWidth(road.count));
    traceRoadRibbon(ctx, { p0, p1, p2, p3 }, wSrc, road.count >= trunkFloor);
    ctx.fillStyle = lit ? theme.blueText : theme.textLow;
    // Test-to-source imports are the least interesting overview signal
    // but the biggest bundles; keep them recessive so source roads lead.
    const testDim = isTestCluster(clusters[road.src].key) ? 0.4 : 1;
    const trunk = road.count >= trunkFloor && testDim === 1 ? 0.22 : 0;
    let alpha = (0.3 + 0.18 * roadBoost + trunk) * testDim * reveal.roads * hoverDim;
    if (litCluster !== null) alpha = lit ? Math.min(1, alpha + 0.55) : alpha * 0.18;
    if (focusRoad !== null && roadIndex !== focusRoad) alpha *= 0.18;
    ctx.globalAlpha = alpha;
    ctx.fill();
    ctx.globalAlpha = 1;

    // Severity overdraw parallel to the road (boundaries lens only;
    // the overview stays neutral until the user asks a question).
    if (
      state.lens === "architecture" &&
      (road.violations > 0 || (road.bidi && road.cycleEdges > 0))
    ) {
      ctx.beginPath();
      ctx.moveTo(p0.x, p0.y + 4);
      ctx.lineTo(p1.x, p1.y + 4);
      ctx.lineTo(p2.x, p2.y + 4);
      ctx.lineTo(p3.x, p3.y + 4);
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
  if (state.lens === "architecture" && kRel >= LOD_SEVERITY) drawSeverityEdges(state, gvs);

  // Focused road, as on the census map: a pale blue halo along its whole
  // path, then the road itself in full ink at its own weight.
  if (focusRoad !== null && roads[focusRoad]) {
    const road = roads[focusRoad];
    const { p0, p1, p2, p3 } = roadGeometry(gvs, road);
    const wSrc = Math.max(minRoadW, roadWidth(road.count));
    ctx.beginPath();
    ctx.moveTo(p0.x, p0.y);
    ctx.lineTo(p1.x, p1.y);
    ctx.lineTo(p2.x, p2.y);
    ctx.lineTo(p3.x, p3.y);
    ctx.lineJoin = "round";
    ctx.lineCap = "butt";
    ctx.strokeStyle = theme.blueSubtle;
    ctx.lineWidth = wSrc + 10 / transform.k;
    ctx.stroke();
    traceRoadRibbon(ctx, { p0, p1, p2, p3 }, wSrc, road.count >= trunkFloor);
    ctx.fillStyle = theme.textHigh;
    ctx.fill();
    // Direction stamp: a station ring marks the importer end, so the
    // taper's meaning is confirmable the moment a road is focused.
    ctx.beginPath();
    ctx.arc(p0.x, p0.y, 4 / transform.k, 0, Math.PI * 2);
    ctx.fillStyle = theme.bg;
    ctx.fill();
    ctx.lineWidth = 2 / transform.k;
    ctx.strokeStyle = theme.textHigh;
    ctx.stroke();
  }
};

/** Direction-encoded edges to the hovered file's direct neighbors. */
const drawHoverNeighborhood = (scene: Scene): HoverContext => {
  const { state, gvs } = scene;
  const { ctx, theme } = state;
  const { transform, fileNodes } = gvs;
  // Hover neighborhood, drawn like a focused census line: octilinear
  // routes on a pale blue halo. Direction is dual-encoded: files importing
  // the hovered one arrive as solid ink ribbons (thick end at the importer,
  // same rule as roads); its own imports leave as thin dashed ink lines. The adjacency index already carries exactly the
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
      const p0 = { x: importerNode.x, y: importerNode.y };
      const p3 = { x: target.x, y: target.y };
      const bend = octilinearBend(p0, p3);
      routeHalo(ctx, p0, bend, p3, theme.blueSubtle, 7 / transform.k);
      ctx.beginPath();
      taperedRibbon(ctx, p0, bend, bend, p3, 2.4 / transform.k, 0.8 / transform.k);
      ctx.fillStyle = theme.textHigh;
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
      const start = { x: target.x, y: target.y };
      const end = { x: importedNode.x, y: importedNode.y };
      const bend = octilinearBend(start, end);
      routeHalo(ctx, start, bend, end, theme.blueSubtle, 5 / transform.k);
      traceRoute(ctx, start, bend, end);
      ctx.strokeStyle = theme.textHigh;
      ctx.lineWidth = 1.2 / transform.k;
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
    if (prev && prev !== color) color = mix(prev, color, lensRipple(scene, node));
  }
  const recessive = color === theme.cellNeutral || color === theme.cellEntry;
  const matched = !searching || state.searchMatches.has(node.fileIndex);
  const inReach = searching && state.searchReach.has(node.fileIndex);
  const isNeighbor = hover.neighbors?.has(node.fileIndex) ?? false;
  const dimmed = hover.neighbors !== null && !isNeighbor;

  let alpha = recessive ? 0.82 : 0.95;
  if (dimmed) alpha += (HOVER_DIM_ALPHA - alpha) * scene.dimT;
  // Files reachable from the matched set stay legible (the combined blast
  // radius); everything else recedes.
  if (searching && !matched) alpha = Math.min(alpha, inReach ? 0.5 : 0.1);
  if (isNeighbor) alpha = 1;
  alpha *= reveal.cluster(gvs.clusters[node.cluster]);
  if (alpha <= 0.01) return null;
  return { color, alpha, matched, inReach, dimmed };
};

/** Lenses with few flagged files halo the medium ones as well. */
const SPARSE_FINDINGS = 40;

/**
 * Files that get a halo, with its color: the copies of an open duplicated
 * block, high findings, and, when findings are sparse, medium ones.
 */
const beaconFiles = (scene: Scene): Map<number, string> => {
  const { state } = scene;
  const { theme } = state;
  const beacons = new Map<number, string>();
  if (state.selectedClone !== null) {
    for (const instance of state.data.clones[state.selectedClone]?.instances ?? []) {
      beacons.set(instance.file, theme.amber);
    }
    return beacons;
  }
  if (state.lens === "overview" || state.activeAnalysis !== null) return beacons;
  const medium: number[] = [];
  state.data.files.forEach((file, fileIdx) => {
    const level = lensFindingLevel(state.lens, state.index, file, fileIdx);
    if (level === 2) beacons.set(fileIdx, theme.red);
    else if (level === 1) medium.push(fileIdx);
  });
  if (beacons.size + medium.length <= SPARSE_FINDINGS) {
    for (const fileIdx of medium) beacons.set(fileIdx, theme.amber);
  }
  return beacons;
};

const drawNodes = (scene: Scene, hover: HoverContext, width: number, height: number): void => {
  const { state, gvs, kRel, searching } = scene;
  const { ctx, theme, data } = state;
  const { transform, clusters, fileNodes } = gvs;
  const files = data.files;
  const { importers: hoverImporters, imports: hoverImports } = hover;
  const beacons = beaconFiles(scene);
  // Nodes.
  for (const node of fileNodes) {
    if (!node || node.x == null || node.y == null) continue;
    if (clusters[node.cluster].isolated && !gvs.standaloneOpen) continue;
    const look = nodeAppearance(scene, hover, node);
    if (!look) continue;
    const file = files[node.fileIndex];
    const { color, alpha, matched, inReach, dimmed } = look;

    // A flagged file is often a 3 px dot; a fixed-size census interchange
    // ring (paper fill, solid route-color ring) keeps it findable at any
    // zoom without a glow.
    if (!dimmed && !searching && beacons.has(node.fileIndex)) {
      ctx.globalAlpha = alpha;
      ctx.beginPath();
      ctx.arc(
        node.x,
        node.y,
        Math.max(node.radius + 4 / transform.k, 8 / transform.k),
        0,
        Math.PI * 2,
      );
      ctx.fillStyle = theme.bg;
      ctx.fill();
      ctx.lineWidth = 2 / transform.k;
      ctx.strokeStyle = beacons.get(node.fileIndex) ?? color;
      ctx.stroke();
    }

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
    // a solid ink ring = imports the hovered file, a dashed ink ring =
    // imported by it. The hovered file itself is the interchange.
    if (state.graphHovered === node.fileIndex) {
      ctx.strokeStyle = theme.textHigh;
      ctx.lineWidth = 2.5 / transform.k;
      ctx.beginPath();
      ctx.arc(node.x, node.y, node.radius * 1.3 + 3 / transform.k, 0, Math.PI * 2);
      ctx.stroke();
    }
    if (hoverImporters.has(node.fileIndex) || hoverImports.has(node.fileIndex)) {
      const importer = hoverImporters.has(node.fileIndex);
      ctx.strokeStyle = theme.textHigh;
      if (!importer) ctx.setLineDash([2 / transform.k, 2 / transform.k]);
      ctx.lineWidth = (importer ? 1.6 : 1.2) / transform.k;
      ctx.beginPath();
      ctx.arc(node.x, node.y, node.radius + 2.5 / transform.k, 0, Math.PI * 2);
      ctx.stroke();
      ctx.setLineDash([]);
    }

    if (!dimmed) {
      if (state.lens === "unused" && file.status === "unused") {
        ctx.setLineDash([3 / transform.k, 3 / transform.k]);
        ctx.strokeStyle = theme.redText;
        ctx.lineWidth = 1.4 / transform.k;
        ctx.stroke();
        ctx.setLineDash([]);
      } else if (
        state.lens === "architecture" &&
        state.index.violationSources.has(node.fileIndex)
      ) {
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

/**
 * Lens fade progress of one node. The new colors ripple outward from the
 * middle of the stage, so a lens switch spreads through the graph the way
 * the treemap's sweep crosses the map. Reduced motion fades uniformly.
 */
const lensRipple = (scene: Scene, node: FileNode): number => {
  if (scene.state.reducedMotion) return easeOut(scene.lensT);
  const { transform } = scene.gvs;
  const sx = (node.x ?? 0) * transform.k + transform.x;
  const sy = (node.y ?? 0) * transform.k + transform.y;
  const dist = Math.hypot(sx - scene.rippleCenter.x, sy - scene.rippleCenter.y);
  const delay = Math.min(1, dist / scene.rippleReach) * GRAPH_LENS_SPREAD;
  return easeOut(Math.min(1, Math.max(0, (scene.lensT - delay) / (1 - GRAPH_LENS_SPREAD))));
};

/** Lens crossfade progress; clears the previous lens once the fade ends. */
const lensFadeProgress = (state: AppState, gvs: GraphViewState, now: number): number => {
  const lensMs = state.reducedMotion ? GRAPH_LENS_REDUCED_MS : GRAPH_LENS_MS;
  const lensT = gvs.lensFadeAt <= 0 ? 1 : Math.min(1, (now - gvs.lensFadeAt) / lensMs);
  if (lensT >= 1) {
    gvs.lensPrev = null;
    gvs.lensFadeAt = 0;
  }
  return lensT;
};

/**
 * Hover dim progress. The hover dim eases in over a beat so the
 * neighborhood surfaces instead of snapping; leaving a node restores the
 * map at once.
 */
const hoverDimProgress = (state: AppState, gvs: GraphViewState, now: number): number => {
  const hovering = state.graphHovered ?? null;
  if (hovering !== gvs.hoverFadeFile) {
    const fromNothing = gvs.hoverFadeFile === null;
    gvs.hoverFadeFile = hovering;
    // Moving between nodes keeps the map dimmed; only a fresh hover fades.
    if (fromNothing) gvs.hoverFadeAt = now;
  }
  if (state.reducedMotion || hovering === null) return 1;
  return easeOut(Math.min(1, (now - gvs.hoverFadeAt) / HOVER_DIM_MS));
};

/** Queue one more graph frame, replacing any frame already queued. */
const scheduleGraphFrame = (state: AppState, gvs: GraphViewState): void => {
  cancelAnimationFrame(gvs.raf);
  gvs.raf = requestAnimationFrame(() => {
    if (state.view === "graph") renderGraph(state);
  });
};

/** Transient notice (fades after 1.8s). */
const drawNotice = (state: AppState, gvs: GraphViewState, width: number): void => {
  if (gvs.notice === "") return;
  const age = performance.now() - gvs.noticeAt;
  if (age >= 1800) {
    gvs.notice = "";
    return;
  }
  const { ctx, theme } = state;
  ctx.font = FONT_SMALL;
  ctx.textAlign = "center";
  ctx.textBaseline = "top";
  ctx.fillStyle = theme.amberText;
  ctx.globalAlpha = age > 1400 ? 1 - (age - 1400) / 400 : 1;
  ctx.fillText(gvs.notice, usableStageWidth(state, width) / 2, 28);
  ctx.globalAlpha = 1;
  scheduleGraphFrame(state, gvs);
};

/** True while some part of the overview still animates. */
const overviewAnimating = (scene: Scene): boolean => {
  const { state, gvs } = scene;
  const motion = !state.reducedMotion && (gvs.hoveredRoad !== null || gvs.pulseFile !== null);
  return motion || scene.lensT < 1 || scene.dimT < 1 || scene.reveal.progress < 1 || gvs.showIntro;
};

/** Build the per-frame scene of the overview. */
const buildScene = (state: AppState, gvs: GraphViewState, width: number, height: number): Scene => {
  const now = performance.now();
  const usableW = usableStageWidth(state, width);
  return {
    state,
    gvs,
    kRel: gvs.transform.k / gvs.fitK,
    reveal: revealProgress(gvs, state.reducedMotion),
    searching: state.search.trim() !== "",
    lensT: lensFadeProgress(state, gvs, now),
    rippleCenter: { x: usableW / 2, y: height / 2 },
    rippleReach: Math.hypot(usableW / 2, height / 2),
    dimT: hoverDimProgress(state, gvs, now),
  };
};

/**
 * Labels join once the roads have flowed in (their internal alpha
 * handling would fight a global fade). While a file is hovered the
 * neighborhood labels own the foreground instead.
 */
const drawOverviewLabels = (
  scene: Scene,
  hover: HoverContext,
  width: number,
  height: number,
): void => {
  const { state, gvs } = scene;
  if (scene.reveal.labels > 0.35 && hover.hovered === null) {
    drawRoadLabels(state, gvs);
    drawClusterLabels(state, gvs);
  }
  if (hover.hovered !== null && hover.neighbors !== null) {
    drawHoverLabels(state, gvs, hover.hovered, hover.importers, hover.imports, width, height);
  }
};

const renderOverview = (
  state: AppState,
  gvs: GraphViewState,
  width: number,
  height: number,
): void => {
  const { ctx } = state;
  const { transform } = gvs;
  const scene = buildScene(state, gvs, width, height);

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

  drawOverviewLabels(scene, hover, width, height);
  drawCanvasLegend(state, width, height);
  drawPathTrace(state, gvs, width, height);

  drawMinimap(state, gvs, width, height);

  drawNotice(state, gvs, width);

  drawIntroCaptions(state, gvs, width);

  // Motion frames while something animates.
  if (overviewAnimating(scene)) scheduleGraphFrame(state, gvs);
};
/** Trace the census route start → bend → end into a new path. */
const traceRoute = (ctx: CanvasRenderingContext2D, start: Pt, bend: Pt, end: Pt): void => {
  ctx.beginPath();
  ctx.moveTo(start.x, start.y);
  ctx.lineTo(bend.x, bend.y);
  ctx.lineTo(end.x, end.y);
};

/** The census focus halo: a pale blue band along a whole route. */
const routeHalo = (
  ctx: CanvasRenderingContext2D,
  start: Pt,
  bend: Pt,
  end: Pt,
  color: string,
  width: number,
): void => {
  traceRoute(ctx, start, bend, end);
  ctx.lineJoin = "round";
  ctx.strokeStyle = color;
  ctx.globalAlpha = 1;
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
