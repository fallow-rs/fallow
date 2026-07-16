import {
  forceSimulation,
  forceLink,
  forceManyBody,
  forceCenter,
  forceCollide,
  forceX,
  forceY,
  type SimulationNodeDatum,
  type SimulationLinkDatum,
} from "d3-force";
import { select } from "d3-selection";
import { zoom, zoomIdentity, type D3ZoomEvent } from "d3-zoom";
import Graph from "graphology";
import louvain from "graphology-communities-louvain";
import type { AppState } from "./state";
import type { VizFile } from "./types";
import { lensColor, lensFlag } from "./data";

// ── Types ───────────────────────────────────────────────────────

interface FileNode extends SimulationNodeDatum {
  fileIndex: number;
  radius: number;
  group: string;
  groupX: number;
  groupY: number;
}

interface FileLink extends SimulationLinkDatum<FileNode> {
  isCross: boolean;
  typeOnly: boolean;
  isCycle: boolean;
  isViolation: boolean;
}

interface GroupInfo {
  name: string;
  cx: number;
  cy: number;
  radius: number;
  fileCount: number;
  findings: number;
}

export type ClusterMode = "directory" | "imports";

interface GraphViewState {
  fileNodes: FileNode[];
  fileLinks: FileLink[];
  groups: GroupInfo[];
  transform: { x: number; y: number; k: number };
  draggedNode: number | null;
  initialized: boolean;
  clusterMode: ClusterMode;
  simulation: ReturnType<typeof forceSimulation<FileNode>> | null;
  zoomBehavior: ReturnType<typeof zoom<HTMLCanvasElement, unknown>> | null;
}

const FONT = '10px "Martian Mono", "JetBrains Mono", ui-monospace, Menlo, monospace';
const FONT_SMALL = '9px "Martian Mono", "JetBrains Mono", ui-monospace, Menlo, monospace';

const NODE_R_MIN = 2.5;
const NODE_R_MAX = 11;
const MAX_CLUSTERS = 40;

// ── State accessor ──────────────────────────────────────────────

const getGVS = (state: AppState): GraphViewState => {
  const ext = state as AppState & { _gvs?: GraphViewState };
  if (!ext._gvs) {
    ext._gvs = {
      fileNodes: [],
      fileLinks: [],
      groups: [],
      transform: { x: 0, y: 0, k: 1 },
      draggedNode: null,
      initialized: false,
      clusterMode: "directory",
      simulation: null,
      zoomBehavior: null,
    };
  }
  return ext._gvs;
};

export const getClusterMode = (state: AppState): ClusterMode => getGVS(state).clusterMode;

// ── Clustering ──────────────────────────────────────────────────

const louvainCluster = (
  files: VizFile[],
  edges: [number, number, number][],
): Map<string, number[]> => {
  const g = new Graph({ type: "undirected" });
  for (let i = 0; i < files.length; i++) g.addNode(String(i));

  const seen = new Set<string>();
  for (const [src, tgt] of edges) {
    if (src >= files.length || tgt >= files.length || src === tgt) continue;
    const key = src < tgt ? `${src}-${tgt}` : `${tgt}-${src}`;
    if (seen.has(key)) continue;
    seen.add(key);
    g.addEdge(String(src), String(tgt));
  }

  const communities = louvain(g, { resolution: 1.2 });
  const communityMap = new Map<number, number[]>();
  for (let i = 0; i < files.length; i++) {
    const comm = communities[String(i)] ?? 0;
    if (!communityMap.has(comm)) communityMap.set(comm, []);
    communityMap.get(comm)?.push(i);
  }

  const result = new Map<string, number[]>();
  for (const [, indices] of communityMap) {
    const dirCounts = new Map<string, number>();
    for (const idx of indices) {
      const parts = files[idx].path.split("/");
      const dir = parts.length > 1 ? parts.slice(0, 2).join("/") : parts[0];
      const shortDir = dir.split("/").pop() ?? dir;
      dirCounts.set(shortDir, (dirCounts.get(shortDir) ?? 0) + 1);
    }
    const sorted = Array.from(dirCounts.entries()).sort((a, b) => b[1] - a[1]);
    let name = sorted.slice(0, 2).map(([dir]) => dir).join(" + ");
    if (result.has(name)) {
      let suffix = 2;
      while (result.has(`${name} (${suffix})`)) suffix++;
      name = `${name} (${suffix})`;
    }
    result.set(name, indices);
  }
  return result;
};

const directoryCluster = (files: VizFile[]): Map<string, number[]> => {
  const clusters = new Map<string, { indices: number[]; depth: number }>();
  for (let i = 0; i < files.length; i++) {
    const key = files[i].path.split("/")[0];
    const existing = clusters.get(key);
    if (existing) existing.indices.push(i);
    else clusters.set(key, { indices: [i], depth: 0 });
  }

  for (let round = 0; round < 10; round++) {
    if (clusters.size >= MAX_CLUSTERS) break;
    let largestKey = "";
    let largestSize = 0;
    for (const [key, { indices }] of clusters) {
      if (indices.length > largestSize) {
        largestSize = indices.length;
        largestKey = key;
      }
    }
    if (largestSize <= Math.max(20, files.length / MAX_CLUSTERS)) break;

    const largest = clusters.get(largestKey);
    if (!largest) break;
    const nextDepth = largest.depth + 1;
    const subMap = new Map<string, number[]>();
    for (const idx of largest.indices) {
      const parts = files[idx].path.split("/");
      const key =
        parts.length > nextDepth + 1 ? parts.slice(0, nextDepth + 1).join("/") : parts.join("/");
      if (!subMap.has(key)) subMap.set(key, []);
      subMap.get(key)?.push(idx);
    }
    if (subMap.size <= 1 || clusters.size + subMap.size - 1 > MAX_CLUSTERS) break;
    clusters.delete(largestKey);
    for (const [key, indices] of subMap) clusters.set(key, { indices, depth: nextDepth });
  }

  const result = new Map<string, number[]>();
  for (const [key, { indices }] of clusters) result.set(key, indices);
  return result;
};

// ── Init ────────────────────────────────────────────────────────

export const initGraphNodes = (state: AppState): void => {
  const { data, canvas } = state;
  const gvs = getGVS(state);
  if (gvs.initialized) {
    renderGraph(state);
    return;
  }

  const stage = canvas.parentElement;
  const w = stage ? stage.clientWidth : window.innerWidth;
  const h = stage ? stage.clientHeight : window.innerHeight;
  const cx = w / 2;
  const cy = h / 2;

  const files = data.files;
  const maxSize = files.reduce((max, f) => Math.max(max, f.size), 1);
  const n = files.length;

  const groupMap =
    gvs.clusterMode === "imports"
      ? louvainCluster(files, data.edges)
      : directoryCluster(files);

  const groupEntries = Array.from(groupMap.entries()).sort((a, b) => b[1].length - a[1].length);
  const groupCount = groupEntries.length;

  const groupInfos: GroupInfo[] = groupEntries.map(([name, indices]) => {
    let findings = 0;
    for (const idx of indices) {
      if (lensFlag(state.lens, state.index, files[idx], idx)) findings++;
      else if (files[idx].status === "hasUnusedExports") findings++;
    }
    return {
      name,
      cx: 0,
      cy: 0,
      radius: Math.sqrt(indices.length) * 15 + 25,
      fileCount: indices.length,
      findings,
    };
  });

  const groupSim = forceSimulation(groupInfos as unknown as SimulationNodeDatum[])
    .force("charge", forceManyBody().strength(-30))
    .force("center", forceCenter(cx, cy))
    .force(
      "collide",
      forceCollide<SimulationNodeDatum & { radius: number }>((d) => d.radius * 0.6),
    )
    .stop();
  for (let g = 0; g < groupCount; g++) {
    const a = (g / groupCount) * Math.PI * 2 - Math.PI / 2;
    const r = Math.min(w, h) * 0.15;
    (groupInfos[g] as unknown as SimulationNodeDatum).x = cx + Math.cos(a) * r;
    (groupInfos[g] as unknown as SimulationNodeDatum).y = cy + Math.sin(a) * r;
  }
  for (let i = 0; i < 200; i++) groupSim.tick();
  for (let g = 0; g < groupCount; g++) {
    const node = groupInfos[g] as unknown as SimulationNodeDatum;
    groupInfos[g].cx = node.x ?? cx;
    groupInfos[g].cy = node.y ?? cy;
  }
  gvs.groups = groupInfos;

  const fileToGroup = new Map<number, string>();
  for (const [name, indices] of groupMap) {
    for (const idx of indices) fileToGroup.set(idx, name);
  }
  const groupCenterMap = new Map<string, { x: number; y: number }>();
  for (let g = 0; g < groupCount; g++) {
    groupCenterMap.set(groupEntries[g][0], { x: groupInfos[g].cx, y: groupInfos[g].cy });
  }

  gvs.fileNodes = files.map((file, i) => {
    const group = fileToGroup.get(i) ?? "unknown";
    const gc = groupCenterMap.get(group) ?? { x: cx, y: cy };
    const sizeRatio = Math.log(file.size + 1) / Math.log(maxSize + 1);
    const radius = NODE_R_MIN + sizeRatio * (NODE_R_MAX - NODE_R_MIN);
    return {
      fileIndex: i,
      radius,
      group,
      groupX: gc.x,
      groupY: gc.y,
      x: gc.x + (Math.random() - 0.5) * 40,
      y: gc.y + (Math.random() - 0.5) * 40,
    };
  });

  gvs.fileLinks = data.edges
    .filter(([s, t]) => s < n && t < n)
    .map(([s, t, flags]) => ({
      source: s,
      target: t,
      isCross: gvs.fileNodes[s].group !== gvs.fileNodes[t].group,
      typeOnly: (flags & 1) === 1,
      isCycle: state.index.cycleEdges.has(s * n + t),
      isViolation: state.index.violationEdges.has(s * n + t),
    }));

  const sim = forceSimulation(gvs.fileNodes)
    .force("charge", forceManyBody<FileNode>().strength(-15).distanceMax(200))
    .force(
      "link",
      forceLink<FileNode, FileLink>(gvs.fileLinks)
        .distance((d) => (d.isCross ? 150 : 30))
        .strength((d) => (d.isCross ? 0.005 : 0.12)),
    )
    .force("collide", forceCollide<FileNode>((d) => d.radius + 1.5))
    .force("groupX", forceX<FileNode>((d) => d.groupX).strength(0.5))
    .force("groupY", forceY<FileNode>((d) => d.groupY).strength(0.5))
    .alphaDecay(0.02)
    .on("tick", () => renderGraph(state))
    .stop();

  for (let i = 0; i < 300; i++) sim.tick();
  sim.stop();
  gvs.simulation = sim;

  // Recompute group hulls from settled node positions.
  for (const gi of gvs.groups) {
    let sx = 0;
    let sy = 0;
    let count = 0;
    for (const fn of gvs.fileNodes) {
      if (fn.group === gi.name && fn.x != null && fn.y != null) {
        sx += fn.x;
        sy += fn.y;
        count++;
      }
    }
    if (count > 0) {
      gi.cx = sx / count;
      gi.cy = sy / count;
    }
    let maxDist = 0;
    for (const fn of gvs.fileNodes) {
      if (fn.group !== gi.name || fn.x == null || fn.y == null) continue;
      const dx = fn.x - gi.cx;
      const dy = fn.y - gi.cy;
      maxDist = Math.max(maxDist, Math.sqrt(dx * dx + dy * dy) + fn.radius);
    }
    gi.radius = maxDist + 12;
  }

  // Fit-to-view.
  let minX = Infinity;
  let minY = Infinity;
  let maxX = -Infinity;
  let maxY = -Infinity;
  for (const fn of gvs.fileNodes) {
    if (fn.x == null || fn.y == null) continue;
    minX = Math.min(minX, fn.x - fn.radius);
    minY = Math.min(minY, fn.y - fn.radius);
    maxX = Math.max(maxX, fn.x + fn.radius);
    maxY = Math.max(maxY, fn.y + fn.radius);
  }
  const padding = 40;
  const bboxW = maxX - minX + padding * 2;
  const bboxH = maxY - minY + padding * 2;
  const fitScale = Math.min(w / bboxW, h / bboxH);
  const fitX = (w - bboxW * fitScale) / 2 - minX * fitScale + padding * fitScale;
  const fitY = (h - bboxH * fitScale) / 2 - minY * fitScale + padding * fitScale;
  gvs.transform = { x: fitX, y: fitY, k: fitScale };

  const zoomBehavior = zoom<HTMLCanvasElement, unknown>()
    .scaleExtent([0.1, 6])
    .filter((event: MouseEvent | WheelEvent) => {
      if (event.type === "wheel") return !event.ctrlKey;
      if ((event as MouseEvent).button !== 0) return true;
      const rect = canvas.getBoundingClientRect();
      const px = (event as MouseEvent).clientX - rect.left;
      const py = (event as MouseEvent).clientY - rect.top;
      return graphHitTest(state, px, py) === null;
    })
    .on("zoom", (event: D3ZoomEvent<HTMLCanvasElement, unknown>) => {
      gvs.transform = { x: event.transform.x, y: event.transform.y, k: event.transform.k };
      renderGraph(state);
    });

  const initialTransform = zoomIdentity.translate(fitX, fitY).scale(fitScale);
  select(canvas).call(zoomBehavior).call(zoomBehavior.transform, initialTransform);
  gvs.zoomBehavior = zoomBehavior;

  gvs.initialized = true;
  renderGraph(state);
};

export const setClusterMode = (state: AppState, mode: ClusterMode): void => {
  const gvs = getGVS(state);
  if (gvs.clusterMode === mode) return;
  gvs.clusterMode = mode;
  gvs.initialized = false;
  initGraphNodes(state);
};

// ── Rendering ───────────────────────────────────────────────────

export const renderGraph = (state: AppState): void => {
  const { canvas, ctx, data, theme, dpr } = state;
  const gvs = getGVS(state);
  if (!gvs.initialized) return;

  const stage = canvas.parentElement;
  const w = stage ? stage.clientWidth : window.innerWidth;
  const h = stage ? stage.clientHeight : window.innerHeight;
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

  const { transform, fileNodes, fileLinks, groups } = gvs;
  const files = data.files;
  const searching = state.search.trim() !== "";

  ctx.save();
  ctx.translate(transform.x, transform.y);
  ctx.scale(transform.k, transform.k);

  // Group hulls.
  ctx.font = FONT;
  for (const g of groups) {
    ctx.fillStyle = theme.surface1;
    ctx.globalAlpha = 0.5;
    ctx.beginPath();
    ctx.arc(g.cx, g.cy, g.radius, 0, Math.PI * 2);
    ctx.fill();
    ctx.globalAlpha = 0.35;
    ctx.strokeStyle = theme.borderSubtle;
    ctx.lineWidth = 1 / transform.k;
    ctx.stroke();

    ctx.globalAlpha = 0.9;
    ctx.fillStyle = theme.textLow;
    ctx.textAlign = "center";
    ctx.textBaseline = "bottom";
    const nameParts = g.name.split("/");
    const shortName = nameParts.length > 2 ? nameParts.slice(-2).join("/") : g.name;
    ctx.fillText(shortName, g.cx, g.cy - g.radius - 5);
    ctx.globalAlpha = 1;
  }

  // Hover/selection neighborhood. Hover dims the rest hard; a selection
  // alone keeps the surrounding graph readable.
  const focus = state.graphHovered ?? state.selected;
  const hardDim = state.graphHovered !== null;
  const dimAlpha = hardDim ? 0.08 : 0.3;
  const dimEdgeAlpha = hardDim ? 0.02 : 0.05;
  let neighbors: Set<number> | null = null;
  if (focus !== null) {
    neighbors = new Set<number>([focus]);
    for (const link of fileLinks) {
      const si = (link.source as FileNode).fileIndex;
      const ti = (link.target as FileNode).fileIndex;
      if (si === focus) neighbors.add(ti);
      if (ti === focus) neighbors.add(si);
    }
  }

  // Edges.
  for (const link of fileLinks) {
    const src = link.source as FileNode;
    const tgt = link.target as FileNode;
    if (src.x == null || tgt.x == null || src.y == null || tgt.y == null) continue;

    const inFocus =
      neighbors !== null && neighbors.has(src.fileIndex) && neighbors.has(tgt.fileIndex);

    if (link.isViolation) {
      ctx.strokeStyle = theme.red;
      ctx.globalAlpha = neighbors !== null && !inFocus ? 0.1 : 0.75;
      ctx.lineWidth = 1.5 / transform.k;
      ctx.setLineDash([]);
    } else if (link.isCycle) {
      ctx.strokeStyle = theme.amber;
      ctx.globalAlpha = neighbors !== null && !inFocus ? 0.1 : 0.6;
      ctx.lineWidth = 1.2 / transform.k;
      ctx.setLineDash([5 / transform.k, 3 / transform.k]);
    } else if (inFocus) {
      const srcColor = lensColor(state.lens, theme, state.index, files[src.fileIndex]);
      ctx.strokeStyle = srcColor === theme.cellNeutral ? theme.textLow : srcColor;
      ctx.globalAlpha = 0.6;
      ctx.lineWidth = 1.5 / transform.k;
      ctx.setLineDash(link.typeOnly ? [2 / transform.k, 2 / transform.k] : []);
    } else if (neighbors !== null) {
      ctx.globalAlpha = dimEdgeAlpha;
      ctx.strokeStyle = theme.textMuted;
      ctx.lineWidth = 0.4 / transform.k;
      ctx.setLineDash([]);
    } else {
      ctx.strokeStyle = theme.textMuted;
      ctx.globalAlpha = link.typeOnly ? 0.04 : 0.08;
      ctx.lineWidth = 0.5 / transform.k;
      ctx.setLineDash(link.typeOnly ? [2 / transform.k, 2 / transform.k] : []);
    }

    ctx.beginPath();
    ctx.moveTo(src.x, src.y);
    ctx.lineTo(tgt.x, tgt.y);
    ctx.stroke();
  }
  ctx.setLineDash([]);
  ctx.globalAlpha = 1;

  // Nodes: flagged findings render on top.
  const renderNode = (i: number, pass: "base" | "top"): void => {
    const node = fileNodes[i];
    if (node.x == null || node.y == null) return;
    const file = files[node.fileIndex];
    const flagged =
      lensFlag(state.lens, state.index, file, node.fileIndex) ||
      (state.lens === "deadcode" && file.status === "hasUnusedExports");
    if (pass === "base" && flagged) return;
    if (pass === "top" && !flagged) return;

    const isFocus = focus === node.fileIndex;
    const isNeighbor = neighbors !== null && neighbors.has(node.fileIndex);
    const dimmed = neighbors !== null && !isNeighbor;
    const matched = !searching || state.searchMatches.has(node.fileIndex);

    const color = lensColor(state.lens, theme, state.index, file);
    const recessive = color === theme.cellNeutral || color === theme.cellEntry;
    let alpha = recessive ? 0.6 : 0.95;
    if (dimmed) alpha = dimAlpha;
    if (searching && !matched) alpha = Math.min(alpha, 0.08);
    if (isFocus || isNeighbor) alpha = 1;

    let r = node.radius;
    if (isFocus) r *= 1.35;

    ctx.globalAlpha = alpha;
    ctx.fillStyle = color;
    ctx.beginPath();
    ctx.arc(node.x, node.y, r, 0, Math.PI * 2);
    ctx.fill();

    // Ring encodings (never color alone): dashed red = unused file,
    // solid amber = unused exports, solid blue = selection.
    if (!dimmed) {
      if (state.lens === "deadcode" && file.status === "unused") {
        ctx.setLineDash([3 / transform.k, 3 / transform.k]);
        ctx.strokeStyle = theme.redText;
        ctx.lineWidth = 1.5 / transform.k;
        ctx.stroke();
        ctx.setLineDash([]);
      } else if (state.lens === "boundaries" && state.index.violationSources.has(node.fileIndex)) {
        ctx.setLineDash([3 / transform.k, 3 / transform.k]);
        ctx.strokeStyle = theme.red;
        ctx.lineWidth = 1.5 / transform.k;
        ctx.stroke();
        ctx.setLineDash([]);
      }
      if (searching && matched) {
        ctx.strokeStyle = theme.amberText;
        ctx.lineWidth = 1.5 / transform.k;
        ctx.stroke();
      }
    }
    if (state.selected === node.fileIndex) {
      ctx.strokeStyle = theme.blue;
      ctx.lineWidth = 2 / transform.k;
      ctx.stroke();
    }
    ctx.globalAlpha = 1;
  };

  for (let i = 0; i < fileNodes.length; i++) renderNode(i, "base");
  for (let i = 0; i < fileNodes.length; i++) renderNode(i, "top");

  // Labels for the focused neighborhood.
  if (neighbors !== null) {
    for (const node of fileNodes) {
      if (node.x == null || node.y == null) continue;
      if (!neighbors.has(node.fileIndex)) continue;
      const isFocus = focus === node.fileIndex;
      const file = files[node.fileIndex];
      const name = file.path.split("/").pop() ?? file.path;
      const r = node.radius * (isFocus ? 1.35 : 1);

      ctx.font = isFocus ? FONT : FONT_SMALL;
      const textW = ctx.measureText(name).width;
      ctx.fillStyle = theme.bg;
      ctx.globalAlpha = 0.85;
      ctx.fillRect(node.x - textW / 2 - 2, node.y + r + 1, textW + 4, isFocus ? 14 : 12);
      ctx.globalAlpha = isFocus ? 1 : 0.85;
      ctx.fillStyle = theme.textHigh;
      ctx.textAlign = "center";
      ctx.textBaseline = "top";
      ctx.fillText(name, node.x, node.y + r + 2);
    }
    ctx.globalAlpha = 1;
  }

  ctx.restore();
};

// ── Hit testing / hover / drag ──────────────────────────────────

export const graphHitTest = (state: AppState, canvasX: number, canvasY: number): number | null => {
  const gvs = getGVS(state);
  const { transform, fileNodes } = gvs;
  const gx = (canvasX - transform.x) / transform.k;
  const gy = (canvasY - transform.y) / transform.k;
  for (let i = fileNodes.length - 1; i >= 0; i--) {
    const node = fileNodes[i];
    if (node.x == null || node.y == null) continue;
    const dx = gx - node.x;
    const dy = gy - node.y;
    if (dx * dx + dy * dy <= (node.radius + 4) ** 2) return node.fileIndex;
  }
  return null;
};

export const graphDragStart = (state: AppState, fileIndex: number): void => {
  const gvs = getGVS(state);
  const node = gvs.fileNodes[fileIndex];
  if (!node) return;
  gvs.draggedNode = fileIndex;
  node.fx = node.x;
  node.fy = node.y;
  if (gvs.simulation && !state.reducedMotion) {
    gvs.simulation.alphaTarget(0.3).restart();
  }
};

export const graphDrag = (state: AppState, canvasX: number, canvasY: number): void => {
  const gvs = getGVS(state);
  if (gvs.draggedNode === null) return;
  const node = gvs.fileNodes[gvs.draggedNode];
  const gx = (canvasX - gvs.transform.x) / gvs.transform.k;
  const gy = (canvasY - gvs.transform.y) / gvs.transform.k;
  node.fx = gx;
  node.fy = gy;
  node.x = gx;
  node.y = gy;
};

export const graphDragEnd = (state: AppState): void => {
  const gvs = getGVS(state);
  if (gvs.draggedNode === null) return;
  const node = gvs.fileNodes[gvs.draggedNode];
  node.fx = null;
  node.fy = null;
  gvs.draggedNode = null;
  if (gvs.simulation) gvs.simulation.alphaTarget(0);
};

export const isDragging = (state: AppState): boolean => getGVS(state).draggedNode !== null;

/** Pan/zoom the graph so a file's node sits centered (panel navigation). */
export const centerOnFile = (state: AppState, fileIndex: number): void => {
  const gvs = getGVS(state);
  if (!gvs.initialized || !gvs.zoomBehavior) return;
  const node = gvs.fileNodes[fileIndex];
  if (!node || node.x == null || node.y == null) return;
  const stage = state.canvas.parentElement;
  const w = stage ? stage.clientWidth : window.innerWidth;
  const h = stage ? stage.clientHeight : window.innerHeight;
  const k = Math.max(gvs.transform.k, 1.2);
  const target = zoomIdentity.translate(w / 2 - node.x * k, h / 2 - node.y * k).scale(k);
  select(state.canvas).call(gvs.zoomBehavior.transform, target);
};
