import { forceSimulation, forceLink, forceManyBody, forceCenter, forceCollide, forceX, forceY, type SimulationNodeDatum, type SimulationLinkDatum } from "d3-force";
import { select } from "d3-selection";
import { zoom, zoomIdentity, type D3ZoomEvent } from "d3-zoom";
import Graph from "graphology";
import louvain from "graphology-communities-louvain";
import type { VizFileStatus, VizFile } from "./types";
import type { AppState } from "./state";
import { splitPath, formatSize } from "./utils";

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
}

interface GroupInfo {
  name: string;
  cx: number;
  cy: number;
  radius: number;
  fileCount: number;
  unusedFiles: number;
  unusedExports: number;
}

export type GraphFilter = "all" | "unused" | "issues";
export type ClusterMode = "directory" | "imports";

interface GraphViewState {
  fileNodes: FileNode[];
  fileLinks: FileLink[];
  groups: GroupInfo[];
  transform: { x: number; y: number; k: number };
  hoveredNode: number | null;
  draggedNode: number | null;
  initialized: boolean;
  filter: GraphFilter;
  clusterMode: ClusterMode;
  simulation: ReturnType<typeof forceSimulation<FileNode>> | null;
}

const FONT = '10px -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif';
const FONT_SMALL = '9px -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif';
const FONT_GROUP = 'bold 11px -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif';

const NODE_R_MIN = 3;
const NODE_R_MAX = 12;

// Respect the OS "reduce motion" setting: skip the force-simulation reheat so a
// drag repositions the node without springing its neighbors around.
const prefersReducedMotion = (): boolean =>
  typeof window.matchMedia === "function" &&
  window.matchMedia("(prefers-reduced-motion: reduce)").matches;

// ── Opacity by status and filter ────────────────────────────────

const STATUS_OPACITY: Record<GraphFilter, Record<VizFileStatus, number>> = {
  all: { unused: 1, hasUnusedExports: 0.9, clean: 0.25, entryPoint: 0.4 },
  unused: { unused: 1, hasUnusedExports: 0.06, clean: 0.06, entryPoint: 0.06 },
  issues: { unused: 1, hasUnusedExports: 1, clean: 0.08, entryPoint: 0.08 },
};

const isHighlighted = (file: VizFile, filter: GraphFilter): boolean => {
  if (filter === "all") return file.status === "unused" || file.status === "hasUnusedExports";
  if (filter === "unused") return file.status === "unused";
  return file.status === "unused" || file.status === "hasUnusedExports";
};

// ── State accessor ──────────────────────────────────────────────

const getGVS = (state: AppState): GraphViewState => {
  const ext = state as AppState & { _gvs?: GraphViewState };
  if (!ext._gvs) {
    ext._gvs = {
      fileNodes: [], fileLinks: [], groups: [],
      transform: { x: 0, y: 0, k: 1 }, hoveredNode: null, draggedNode: null,
      initialized: false, filter: "all", clusterMode: "directory", simulation: null,
    };
  }
  return ext._gvs;
};

// ── Louvain community detection ──────────────────────────────────

const louvainCluster = (
  files: VizFile[],
  edges: [number, number][],
): Map<string, number[]> => {
  const g = new Graph({ type: "undirected" });

  // Add nodes
  for (let i = 0; i < files.length; i++) {
    g.addNode(String(i));
  }

  // Add edges (deduplicate for undirected)
  const seen = new Set<string>();
  for (const [src, tgt] of edges) {
    if (src >= files.length || tgt >= files.length || src === tgt) continue;
    const key = src < tgt ? `${src}-${tgt}` : `${tgt}-${src}`;
    if (seen.has(key)) continue;
    seen.add(key);
    g.addEdge(String(src), String(tgt));
  }

  // Run Louvain
  const communities = louvain(g, { resolution: 1.2 });

  // Group by community, name by most common directory prefix
  const communityMap = new Map<number, number[]>();
  for (let i = 0; i < files.length; i++) {
    const comm = communities[String(i)] ?? 0;
    if (!communityMap.has(comm)) communityMap.set(comm, []);
    communityMap.get(comm)!.push(i);
  }

  // Name each community by its top contributing directories
  const result = new Map<string, number[]>();
  for (const [, indices] of communityMap) {
    const dirCounts = new Map<string, number>();
    for (const idx of indices) {
      const parts = splitPath(files[idx].path);
      const dir = parts.length > 1 ? parts.slice(0, 2).join("/") : parts[0];
      // Use short name (last segment)
      const shortDir = dir.split("/").pop() ?? dir;
      dirCounts.set(shortDir, (dirCounts.get(shortDir) ?? 0) + 1);
    }

    // Build name from top contributing directories
    const sorted = Array.from(dirCounts.entries()).sort((a, b) => b[1] - a[1]);
    const topDirs = sorted.slice(0, 3).map(([dir]) => dir);
    let name = topDirs.join(" + ");

    // Deduplicate names
    if (result.has(name)) {
      let suffix = 2;
      while (result.has(`${name} (${suffix})`)) suffix++;
      name = `${name} (${suffix})`;
    }

    result.set(name, indices);
  }

  return result;
};

// ── Init ────────────────────────────────────────────────────────

export const initGraphNodes = (state: AppState): void => {
  const { data, canvas } = state;
  const gvs = getGVS(state);
  if (gvs.initialized) { renderGraph(state); return; }

  const header = document.getElementById("fallow-header");
  const hh = header ? header.offsetHeight : 0;
  const w = window.innerWidth;
  const h = window.innerHeight - hh;
  const cx = w / 2;
  const cy = h / 2;

  const files = data.files;
  const maxSize = files.reduce((max, f) => Math.max(max, f.size), 1);

  // ── Directory clustering (iterative with cluster cap) ──────
  const MAX_CLUSTERS = 40;

  const directoryCluster = (): Map<string, number[]> => {
    // Start: group by top-level directory
    let clusters = new Map<string, { indices: number[]; depth: number }>();
    for (let i = 0; i < files.length; i++) {
      const parts = splitPath(files[i].path);
      const key = parts[0];
      const existing = clusters.get(key);
      if (existing) existing.indices.push(i);
      else clusters.set(key, { indices: [i], depth: 0 });
    }

    // Iteratively split the largest cluster until we reach MAX_CLUSTERS or can't split
    for (let round = 0; round < 10; round++) {
      if (clusters.size >= MAX_CLUSTERS) break;

      // Find the largest cluster
      let largestKey = "";
      let largestSize = 0;
      for (const [key, { indices }] of clusters) {
        if (indices.length > largestSize) { largestSize = indices.length; largestKey = key; }
      }

      // Stop if the largest cluster is small enough
      if (largestSize <= Math.max(20, files.length / MAX_CLUSTERS)) break;

      const largest = clusters.get(largestKey)!;
      const nextDepth = largest.depth + 1;

      // Try to split by next directory level
      const subMap = new Map<string, number[]>();
      for (const idx of largest.indices) {
        const parts = splitPath(files[idx].path);
        const key = parts.length > nextDepth + 1 ? parts.slice(0, nextDepth + 1).join("/") : parts.join("/");
        if (!subMap.has(key)) subMap.set(key, []);
        subMap.get(key)!.push(idx);
      }

      // Only split if it produces >1 sub-group and won't exceed cap
      if (subMap.size <= 1 || clusters.size + subMap.size - 1 > MAX_CLUSTERS) break;

      // Replace the largest with its sub-groups
      clusters.delete(largestKey);
      for (const [key, indices] of subMap) {
        clusters.set(key, { indices, depth: nextDepth });
      }
    }

    const result = new Map<string, number[]>();
    for (const [key, { indices }] of clusters) {
      result.set(key, indices);
    }
    return result;
  };

  let finalGroupMap: Map<string, number[]>;

  if (gvs.clusterMode === "imports") {
    finalGroupMap = louvainCluster(data.files, data.edges);
  } else {
    finalGroupMap = directoryCluster();
  }

  const groupEntries = Array.from(finalGroupMap.entries()).sort((a, b) => b[1].length - a[1].length);
  const groupCount = groupEntries.length;

  // Group info with health stats
  const groupInfos: GroupInfo[] = groupEntries.map(([name, indices]) => {
    let unusedFiles = 0;
    let unusedExports = 0;
    for (const idx of indices) {
      if (files[idx].status === "unused") unusedFiles++;
      unusedExports += files[idx].unused_export_count;
    }
    return {
      name, cx: 0, cy: 0,
      radius: Math.sqrt(indices.length) * 15 + 25,
      fileCount: indices.length, unusedFiles, unusedExports,
    };
  });

  // Position groups using a mini force simulation (compact, organic layout)
  const groupSim = forceSimulation(groupInfos as unknown as SimulationNodeDatum[])
    .force("charge", forceManyBody().strength(-30))
    .force("center", forceCenter(cx, cy))
    .force("collide", forceCollide<SimulationNodeDatum & { radius: number }>(d => d.radius * 0.6))
    .stop();
  // Init positions on a small circle
  for (let g = 0; g < groupCount; g++) {
    const a = (g / groupCount) * Math.PI * 2 - Math.PI / 2;
    const r = Math.min(w, h) * 0.15;
    (groupInfos[g] as unknown as SimulationNodeDatum).x = cx + Math.cos(a) * r;
    (groupInfos[g] as unknown as SimulationNodeDatum).y = cy + Math.sin(a) * r;
  }
  for (let i = 0; i < 200; i++) groupSim.tick();
  // Read settled positions back
  for (let g = 0; g < groupCount; g++) {
    const node = groupInfos[g] as unknown as SimulationNodeDatum;
    groupInfos[g].cx = node.x ?? cx;
    groupInfos[g].cy = node.y ?? cy;
  }

  gvs.groups = groupInfos;

  // ── File nodes ──────────────────────────────────────────────
  // Build file→group lookup from the final group map
  const fileToGroup = new Map<number, string>();
  for (const [name, indices] of finalGroupMap) {
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
      fileIndex: i, radius, group,
      groupX: gc.x, groupY: gc.y,
      x: gc.x + (Math.random() - 0.5) * 40,
      y: gc.y + (Math.random() - 0.5) * 40,
    };
  });

  // ── File links ──────────────────────────────────────────────
  gvs.fileLinks = data.edges
    .filter(([s, t]) => s < files.length && t < files.length)
    .map(([s, t]) => ({
      source: s, target: t,
      isCross: gvs.fileNodes[s].group !== gvs.fileNodes[t].group,
    }));

  // ── D3 force simulation ─────────────────────────────────────
  const sim = forceSimulation(gvs.fileNodes)
    .force("charge", forceManyBody<FileNode>().strength(-15).distanceMax(200))
    .force("link", forceLink<FileNode, FileLink>(gvs.fileLinks)
      .distance(d => d.isCross ? 150 : 30)
      .strength(d => d.isCross ? 0.005 : 0.12))
    .force("collide", forceCollide<FileNode>(d => d.radius + 1.5))
    .force("groupX", forceX<FileNode>(d => d.groupX).strength(0.5))
    .force("groupY", forceY<FileNode>(d => d.groupY).strength(0.5))
    .alphaDecay(0.02)
    .on("tick", () => renderGraph(state))
    .stop();

  // Pre-compute initial layout
  for (let i = 0; i < 300; i++) sim.tick();
  sim.stop();

  // Save simulation for drag reheating
  gvs.simulation = sim;

  // Recalculate group bounds from simulation result
  for (const gi of groupInfos) {
    let sx = 0, sy = 0, count = 0;
    for (const fn of gvs.fileNodes) {
      if (fn.group === gi.name) { sx += fn.x!; sy += fn.y!; count++; }
    }
    if (count > 0) { gi.cx = sx / count; gi.cy = sy / count; }
    let maxDist = 0;
    for (const fn of gvs.fileNodes) {
      if (fn.group !== gi.name) continue;
      const dx = fn.x! - gi.cx;
      const dy = fn.y! - gi.cy;
      maxDist = Math.max(maxDist, Math.sqrt(dx * dx + dy * dy) + fn.radius);
    }
    gi.radius = maxDist + 12;
  }

  // ── d3-zoom with fit-to-view ────────────────────────────────
  // Calculate bounding box of all nodes
  let minX = Infinity, minY = Infinity, maxX = -Infinity, maxY = -Infinity;
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
  const fitScale = Math.min(w / bboxW, h / bboxH); // fit graph to fill viewport
  const fitX = (w - bboxW * fitScale) / 2 - minX * fitScale + padding * fitScale;
  const fitY = (h - bboxH * fitScale) / 2 - minY * fitScale + padding * fitScale;

  gvs.transform = { x: fitX, y: fitY, k: fitScale };

  const zoomBehavior = zoom<HTMLCanvasElement, unknown>()
    .scaleExtent([0.1, 6])
    .filter((event: MouseEvent | WheelEvent) => {
      // Wheel always zooms. A primary-button press that lands on a node is
      // reserved for node dragging (handled in interactions.ts), so pan only
      // engages on empty space or with a non-primary button.
      if (event.type === "wheel") return !event.ctrlKey;
      if ((event as MouseEvent).button !== 0) return true;
      const rect = canvas.getBoundingClientRect();
      const cx = (event as MouseEvent).clientX - rect.left;
      const cy = (event as MouseEvent).clientY - rect.top;
      return graphHitTest(state, cx, cy) === null;
    })
    .on("zoom", (event: D3ZoomEvent<HTMLCanvasElement, unknown>) => {
      gvs.transform = { x: event.transform.x, y: event.transform.y, k: event.transform.k };
      renderGraph(state);
    });

  const initialTransform = zoomIdentity.translate(fitX, fitY).scale(fitScale);
  select(canvas).call(zoomBehavior).call(zoomBehavior.transform, initialTransform);

  gvs.initialized = true;
  state.graphSimulating = false;
  renderGraph(state);
};

// ── Rendering ───────────────────────────────────────────────────

export const renderGraph = (state: AppState): void => {
  const { canvas, ctx, data, theme, dpr } = state;
  const gvs = getGVS(state);
  if (!gvs.initialized) return;

  const header = document.getElementById("fallow-header");
  const hh = header ? header.offsetHeight : 0;
  const w = window.innerWidth;
  const h = window.innerHeight - hh;
  const pw = w * dpr;
  const ph = h * dpr;

  if (canvas.width !== pw || canvas.height !== ph) {
    canvas.style.width = `${w}px`;
    canvas.style.height = `${h}px`;
    canvas.width = pw;
    canvas.height = ph;
  }

  ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
  ctx.fillStyle = theme.bg;
  ctx.fillRect(0, 0, w, h);

  const { transform, fileNodes, fileLinks, groups, hoveredNode, filter } = gvs;
  const files = data.files;
  const opacityMap = STATUS_OPACITY[filter];

  ctx.save();
  ctx.translate(transform.x, transform.y);
  ctx.scale(transform.k, transform.k);

  // ── Group backgrounds + health annotations ──────────────────
  for (const g of groups) {
    ctx.fillStyle = theme.directory;
    ctx.globalAlpha = 0.12;
    ctx.beginPath();
    ctx.arc(g.cx, g.cy, g.radius, 0, Math.PI * 2);
    ctx.fill();

    ctx.strokeStyle = theme.border;
    ctx.globalAlpha = 0.15;
    ctx.lineWidth = 1 / transform.k;
    ctx.stroke();
    ctx.globalAlpha = 1;

    // Group label
    ctx.fillStyle = theme.textSecondary;
    ctx.globalAlpha = 0.5;
    ctx.font = FONT_GROUP;
    ctx.textAlign = "center";
    ctx.textBaseline = "bottom";
    // Show short label: last 2 segments of the path
    const nameParts = g.name.split("/");
    const shortName = nameParts.length > 2 ? nameParts.slice(-2).join("/") : g.name;
    ctx.fillText(shortName, g.cx, g.cy - g.radius - 6);

    // Health annotation under label
    if (g.unusedFiles > 0 || g.unusedExports > 0) {
      ctx.font = FONT_SMALL;
      const parts: string[] = [];
      if (g.unusedFiles > 0) parts.push(`${g.unusedFiles} unused files`);
      if (g.unusedExports > 0) parts.push(`${g.unusedExports} unused exports`);
      ctx.fillStyle = g.unusedFiles > 0 ? theme.statusColors.unused : theme.statusColors.hasUnusedExports;
      ctx.globalAlpha = 0.6;
      ctx.fillText(parts.join(" · "), g.cx, g.cy - g.radius + 6);
    }
    ctx.globalAlpha = 1;
  }

  // ── Compute hover neighborhood ──────────────────────────────
  // When hovering: the hovered node + all directly connected nodes are "active"
  let hoverNeighbors: Set<number> | null = null;
  if (hoveredNode !== null) {
    const hoveredFileIdx = fileNodes[hoveredNode].fileIndex;
    hoverNeighbors = new Set<number>([hoveredFileIdx]);
    for (const link of fileLinks) {
      const si = (link.source as FileNode).fileIndex;
      const ti = (link.target as FileNode).fileIndex;
      if (si === hoveredFileIdx) hoverNeighbors.add(ti);
      if (ti === hoveredFileIdx) hoverNeighbors.add(si);
    }
  }

  // ── Edges ───────────────────────────────────────────────────
  for (const link of fileLinks) {
    const src = link.source as FileNode;
    const tgt = link.target as FileNode;
    if (src.x == null || tgt.x == null) continue;

    const srcFile = files[src.fileIndex];
    const tgtFile = files[tgt.fileIndex];
    const isConnectedToHover = hoverNeighbors !== null &&
      (hoverNeighbors.has(src.fileIndex) && hoverNeighbors.has(tgt.fileIndex));
    const isDeadEnd = srcFile.status === "unused" || tgtFile.status === "unused";

    if (isConnectedToHover) {
      // Hover: color by source node status
      ctx.strokeStyle = theme.statusColors[srcFile.status];
      ctx.globalAlpha = 0.6;
      ctx.lineWidth = 1.5 / transform.k;
    } else if (hoverNeighbors !== null) {
      // Dim everything not connected to hover
      ctx.globalAlpha = 0.015;
      ctx.strokeStyle = theme.textSecondary;
      ctx.lineWidth = 0.3 / transform.k;
    } else if (filter !== "all" && !isHighlighted(srcFile, filter) && !isHighlighted(tgtFile, filter)) {
      ctx.globalAlpha = 0.01;
      ctx.strokeStyle = theme.textSecondary;
      ctx.lineWidth = 0.3 / transform.k;
    } else {
      // Default: neutral gray for all edges, red dashed only for dead-end
      ctx.strokeStyle = isDeadEnd ? theme.statusColors.unused : theme.textSecondary;
      ctx.globalAlpha = isDeadEnd ? 0.15 : 0.06;
      ctx.lineWidth = 0.5 / transform.k;
    }

    if (isDeadEnd && !isConnectedToHover) {
      ctx.setLineDash([4 / transform.k, 4 / transform.k]);
    }

    ctx.beginPath();
    ctx.moveTo(src.x!, src.y!);
    ctx.lineTo(tgt.x!, tgt.y!);
    ctx.stroke();
    ctx.setLineDash([]);
  }

  ctx.globalAlpha = 1;

  // ── Nodes (two passes: dimmed first, highlighted on top) ────
  const renderNode = (i: number, pass: "dim" | "bright"): void => {
    const node = fileNodes[i];
    if (node.x == null || node.y == null) return;

    const file = files[node.fileIndex];
    const hi = isHighlighted(file, filter);
    if (pass === "dim" && hi) return;
    if (pass === "bright" && !hi && filter !== "all") return;

    const isHoveredNode = hoveredNode === i;
    const isNeighbor = hoverNeighbors !== null && hoverNeighbors.has(node.fileIndex);
    const isDimmedByHover = hoverNeighbors !== null && !isNeighbor;
    const color = theme.statusColors[file.status];

    // Opacity: hover dimming overrides filter opacity
    let nodeOpacity = opacityMap[file.status];
    if (isHoveredNode) nodeOpacity = 1;
    else if (isDimmedByHover) nodeOpacity = 0.08;
    else if (isNeighbor) nodeOpacity = 1;

    // Size boost for problem nodes
    let r = node.radius;
    if (file.status === "unused") r *= 1.1;
    if (isHoveredNode) r *= 1.4;

    ctx.globalAlpha = nodeOpacity;

    // Glow on problem nodes + hovered
    if (file.status === "unused" && !isDimmedByHover) {
      ctx.shadowColor = theme.statusColors.unused;
      ctx.shadowBlur = 8 / transform.k;
    } else if (file.status === "hasUnusedExports" && !isDimmedByHover) {
      ctx.shadowColor = theme.statusColors.hasUnusedExports;
      ctx.shadowBlur = 5 / transform.k;
    } else if (isHoveredNode) {
      ctx.shadowColor = color;
      ctx.shadowBlur = 10 / transform.k;
    }

    // Fill
    ctx.fillStyle = color;
    ctx.beginPath();
    ctx.arc(node.x!, node.y!, r, 0, Math.PI * 2);
    ctx.fill();

    ctx.shadowColor = "transparent";
    ctx.shadowBlur = 0;

    // Border encoding status
    if (file.status === "unused" && !isDimmedByHover) {
      ctx.setLineDash([3 / transform.k, 3 / transform.k]);
      ctx.strokeStyle = theme.statusColors.unused;
      ctx.lineWidth = 2 / transform.k;
      ctx.stroke();
      ctx.setLineDash([]);
    } else if (file.status === "hasUnusedExports" && !isDimmedByHover) {
      ctx.strokeStyle = theme.statusColors.hasUnusedExports;
      ctx.lineWidth = 1.5 / transform.k;
      ctx.stroke();
    } else if (isHoveredNode) {
      ctx.strokeStyle = theme.text;
      ctx.lineWidth = 1.5 / transform.k;
      ctx.stroke();
    }

    ctx.globalAlpha = 1;
  };

  // Pass 1: dimmed nodes (behind)
  for (let i = 0; i < fileNodes.length; i++) renderNode(i, "dim");
  // Pass 2: highlighted nodes (on top, with glow)
  for (let i = 0; i < fileNodes.length; i++) renderNode(i, "bright");

  // ── Labels (drawn AFTER all nodes so they're never behind dots) ──
  if (hoverNeighbors !== null) {
    for (let i = 0; i < fileNodes.length; i++) {
      const node = fileNodes[i];
      if (node.x == null || node.y == null) continue;

      const file = files[node.fileIndex];
      const isHoveredNode = hoveredNode === i;
      const isNeighbor = hoverNeighbors.has(node.fileIndex);
      if (!isHoveredNode && !isNeighbor) continue;

      let r = node.radius;
      if (file.status === "unused") r *= 1.1;
      if (isHoveredNode) r *= 1.4;

      const fileName = file.path.split("/").pop() ?? file.path;

      // White background behind label for readability
      ctx.font = isHoveredNode ? FONT : FONT_SMALL;
      const textWidth = ctx.measureText(fileName).width;
      ctx.fillStyle = theme.bg;
      ctx.globalAlpha = 0.85;
      ctx.fillRect(node.x! - textWidth / 2 - 2, node.y! + r + 1, textWidth + 4, isHoveredNode ? 14 : 12);

      ctx.globalAlpha = isHoveredNode ? 1 : 0.8;
      ctx.fillStyle = theme.text;
      ctx.textAlign = "center";
      ctx.textBaseline = "top";
      ctx.fillText(fileName, node.x!, node.y! + r + 2);
    }
    ctx.globalAlpha = 1;
  }

  ctx.restore();

  // ── HUD ─────────────────────────────────────────────────────
  ctx.font = FONT;
  ctx.fillStyle = theme.textSecondary;
  ctx.textAlign = "left";
  ctx.textBaseline = "top";

  const unusedCount = files.filter(f => f.status === "unused").length;
  const issueCount = files.filter(f => f.status === "unused" || f.status === "hasUnusedExports").length;
  const filterLabel = filter === "all" ? "" : filter === "unused" ? ` · Showing ${unusedCount} unused` : ` · Showing ${issueCount} with issues`;

  ctx.fillText(
    `${fileNodes.length} files · ${fileLinks.length} edges · ${groups.length} directories${filterLabel}`,
    12, 8,
  );
};

// ── Filter ──────────────────────────────────────────────────────

export const setGraphFilter = (state: AppState, filter: GraphFilter): void => {
  getGVS(state).filter = filter;
  renderGraph(state);
};


export const setClusterMode = (state: AppState, mode: ClusterMode): void => {
  const gvs = getGVS(state);
  if (gvs.clusterMode === mode) return;
  gvs.clusterMode = mode;
  gvs.initialized = false; // Force re-init
  initGraphNodes(state);
};


// ── Hit testing ─────────────────────────────────────────────────

export const graphHitTest = (
  state: AppState,
  canvasX: number,
  canvasY: number,
): number | null => {
  const gvs = getGVS(state);
  const { transform, fileNodes } = gvs;

  const gx = (canvasX - transform.x) / transform.k;
  const gy = (canvasY - transform.y) / transform.k;

  for (let i = fileNodes.length - 1; i >= 0; i--) {
    const node = fileNodes[i];
    if (node.x == null || node.y == null) continue;
    const dx = gx - node.x;
    const dy = gy - node.y;
    if (dx * dx + dy * dy <= (node.radius + 4) ** 2) return i;
  }
  return null;
};

export const handleGraphHover = (state: AppState, hitIdx: number | null): void => {
  getGVS(state).hoveredNode = hitIdx;
};

export const handleGraphClick = (_state: AppState, _hitIdx: number | null): void => {};

export const graphGoBack = (_state: AppState): boolean => false;

// ── Drag ────────────────────────────────────────────────────────

export const graphDragStart = (state: AppState, nodeIdx: number): void => {
  const gvs = getGVS(state);
  gvs.draggedNode = nodeIdx;
  const node = gvs.fileNodes[nodeIdx];
  node.fx = node.x;
  node.fy = node.y;
  // Reheat simulation so neighbors settle around the dragged node, unless the
  // user asked for reduced motion (then the node still tracks the cursor via
  // fx/fy + the explicit re-render in interactions.ts, with no spring).
  if (gvs.simulation && !prefersReducedMotion()) {
    gvs.simulation.alphaTarget(0.3).restart();
  }
};

export const graphDrag = (state: AppState, canvasX: number, canvasY: number): void => {
  const gvs = getGVS(state);
  if (gvs.draggedNode === null) return;
  const node = gvs.fileNodes[gvs.draggedNode];
  // Convert canvas coordinates to graph space using the active zoom transform
  // (same inversion as graphHitTest), so the node tracks the cursor at any zoom.
  const gx = (canvasX - gvs.transform.x) / gvs.transform.k;
  const gy = (canvasY - gvs.transform.y) / gvs.transform.k;
  node.fx = gx;
  node.fy = gy;
  // Also move x/y directly so the explicit re-render tracks the cursor without
  // waiting for the next simulation tick (the tick would copy fx -> x anyway).
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
  if (gvs.simulation) {
    gvs.simulation.alphaTarget(0);
  }
};


// ── Tooltip ─────────────────────────────────────────────────────

export const showGraphTooltip = (
  state: AppState,
  hitIdx: number,
  mouseX: number,
  mouseY: number,
): void => {
  const { data, theme } = state;
  const gvs = getGVS(state);
  const node = gvs.fileNodes[hitIdx];
  const file = data.files[node.fileIndex];

  let tip = document.getElementById("fallow-tooltip") as HTMLDivElement | null;
  if (!tip) { tip = document.createElement("div"); tip.id = "fallow-tooltip"; document.body.appendChild(tip); }
  tip.replaceChildren();

  const el = (tag: string, cls: string, text: string, style?: string): HTMLElement => {
    const e = document.createElement(tag); e.className = cls; e.textContent = text;
    if (style) e.setAttribute("style", style); return e;
  };

  tip.appendChild(el("div", "tip-path", file.path));
  tip.appendChild(el("div", "tip-size", formatSize(file.size)));

  const labels: Record<string, string> = {
    clean: "Clean", hasUnusedExports: "Has unused exports",
    unused: "Unused file", entryPoint: "Entry point",
  };
  tip.appendChild(el("div", "tip-status", labels[file.status] ?? file.status, `color:${theme.statusColors[file.status]}`));

  const details: string[] = [];
  if (file.export_count > 0) details.push(`${file.export_count} exports`);
  if (file.import_count > 0) details.push(`${file.import_count} imports`);
  if (file.importer_count > 0) details.push(`${file.importer_count} importers`);
  if (details.length > 0) tip.appendChild(el("div", "tip-details", details.join(" · ")));

  if (file.unused_exports && file.unused_exports.length > 0) {
    const nm = file.unused_exports.slice(0, 6).join(", ");
    const sf = file.unused_exports.length > 6 ? ` (+${file.unused_exports.length - 6})` : "";
    tip.appendChild(el("div", "tip-unused", `Unused: ${nm}${sf}`, `color:${theme.statusColors.hasUnusedExports}`));
  }

  // Impact info for unused files
  if (file.status === "unused" && file.importer_count === 0) {
    tip.appendChild(el("div", "tip-details", "0 importers, safe to delete", "color:#10B981;font-weight:600"));
  }

  tip.style.backgroundColor = theme.tooltipBg;
  tip.style.color = theme.tooltipText;
  tip.style.borderColor = theme.tooltipBorder;
  tip.style.display = "block";

  const tr = tip.getBoundingClientRect();
  let left = mouseX + 12, top = mouseY + 12;
  if (left + tr.width > window.innerWidth - 12) left = mouseX - tr.width - 12;
  if (top + tr.height > window.innerHeight - 12) top = mouseY - tr.height - 12;
  tip.style.left = `${Math.max(12, left)}px`; tip.style.top = `${Math.max(12, top)}px`;
};
