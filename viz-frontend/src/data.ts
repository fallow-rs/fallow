import type { TreeNode, VizData, VizFile, Lens } from "./types";
import type { Theme } from "./theme";
import { dupRamp, heatRamp, zoneColor } from "./theme";

/** Derived, immutable indexes computed once from the embedded payload. */
export interface DataIndex {
  /** Root of the full directory tree (path = ""). */
  tree: TreeNode;
  /** Directory nodes by path for drill navigation. */
  nodesByPath: Map<string, TreeNode>;
  /** file index -> indices of files importing it. */
  importersOf: number[][];
  /** file index -> indices of files it imports. */
  importsOf: number[][];
  /** Packed `from * N + to` keys for edges inside a dependency cycle. */
  cycleEdges: Set<number>;
  /** Packed `from * N + to` keys -> violation indices. */
  violationEdges: Map<number, number[]>;
  /** Files with at least one outgoing boundary violation. */
  violationSources: Set<number>;
  /** Normalization ceiling for the duplication lens (p95 dup ratio). */
  dupCeiling: number;
  /** Normalization ceiling for the hotspot lens (p95 max cyclomatic). */
  heatCeiling: number;
}

const packEdge = (fileCount: number, from: number, to: number): number => from * fileCount + to;

const percentile = (values: number[], fraction: number): number => {
  if (values.length === 0) return 0;
  const sorted = [...values].toSorted((left, right) => left - right);
  const sampleIndex = Math.min(sorted.length - 1, Math.floor(sorted.length * fraction));
  return sorted[sampleIndex];
};

const buildTree = (files: VizFile[]): { root: TreeNode; byPath: Map<string, TreeNode> } => {
  const root: TreeNode = {
    name: "",
    path: "",
    size: 0,
    children: [],
    fileIndex: null,
    parent: null,
  };
  const byPath = new Map<string, TreeNode>();
  byPath.set("", root);

  for (let fileIndex = 0; fileIndex < files.length; fileIndex++) {
    const parts = files[fileIndex].path.split("/");
    let node = root;
    let prefix = "";
    for (let depth = 0; depth < parts.length - 1; depth++) {
      prefix = prefix ? `${prefix}/${parts[depth]}` : parts[depth];
      let child = byPath.get(prefix);
      if (!child) {
        child = {
          name: parts[depth],
          path: prefix,
          size: 0,
          children: [],
          fileIndex: null,
          parent: node,
        };
        byPath.set(prefix, child);
        node.children.push(child);
      }
      node = child;
    }
    const leaf: TreeNode = {
      name: parts[parts.length - 1],
      path: files[fileIndex].path,
      size: Math.max(1, files[fileIndex].size),
      children: [],
      fileIndex,
      parent: node,
    };
    node.children.push(leaf);
  }

  // Roll up sizes bottom-up and sort children by size (largest first).
  const rollup = (node: TreeNode): number => {
    if (node.fileIndex !== null) return node.size;
    node.size = node.children.reduce((sum, child) => sum + rollup(child), 0);
    node.children = node.children.toSorted((nodeA, nodeB) => nodeB.size - nodeA.size);
    return node.size;
  };
  rollup(root);

  // Collapse single-child directory chains (src -> src/components).
  const collapse = (node: TreeNode): void => {
    for (let childIndex = 0; childIndex < node.children.length; childIndex++) {
      let child = node.children[childIndex];
      while (
        child.fileIndex === null &&
        child.children.length === 1 &&
        child.children[0].fileIndex === null
      ) {
        const grand = child.children[0];
        grand.name = `${child.name}/${grand.name}`;
        grand.parent = node;
        node.children[childIndex] = grand;
        child = grand;
      }
      collapse(node.children[childIndex]);
    }
  };
  collapse(root);

  // Rebuild byPath after collapsing (paths unchanged, but chain nodes dropped).
  byPath.clear();
  const reindex = (node: TreeNode): void => {
    if (node.fileIndex === null) byPath.set(node.path, node);
    for (const child of node.children) reindex(child);
  };
  reindex(root);

  return { root, byPath };
};

export const buildIndex = (data: VizData): DataIndex => {
  const fileCount = data.files.length;
  const importersOf: number[][] = Array.from({ length: fileCount }, () => []);
  const importsOf: number[][] = Array.from({ length: fileCount }, () => []);
  for (const [from, to] of data.edges) {
    if (from >= fileCount || to >= fileCount) continue;
    importsOf[from].push(to);
    importersOf[to].push(from);
  }

  const cycleEdges = new Set<number>();
  for (const cycle of data.cycles) {
    for (let index = 0; index < cycle.length; index++) {
      const from = cycle[index];
      const to = cycle[(index + 1) % cycle.length];
      if (from >= fileCount || to >= fileCount) continue;
      cycleEdges.add(packEdge(fileCount, from, to));
      cycleEdges.add(packEdge(fileCount, to, from));
    }
  }

  const violationEdges = new Map<number, number[]>();
  const violationSources = new Set<number>();
  for (let violationIndex = 0; violationIndex < data.violations.length; violationIndex++) {
    const { from, to } = data.violations[violationIndex];
    if (from >= fileCount || to >= fileCount) continue;
    const key = packEdge(fileCount, from, to);
    const list = violationEdges.get(key);
    if (list) list.push(violationIndex);
    else violationEdges.set(key, [violationIndex]);
    violationSources.add(from);
  }

  const dupRatios = data.files.filter((file) => file.dup_lines > 0).map((file) => dupRatio(file));
  const heats = data.files
    .filter((file) => file.max_cyclomatic > 0)
    .map((file) => file.max_cyclomatic);

  const { root, byPath } = buildTree(data.files);

  return {
    tree: root,
    nodesByPath: byPath,
    importersOf,
    importsOf,
    cycleEdges,
    violationEdges,
    violationSources,
    dupCeiling: Math.max(0.15, percentile(dupRatios, 0.95)),
    heatCeiling: Math.max(15, percentile(heats, 0.95)),
  };
};

/** Approximate share of a file's lines that are duplicated (0..1). */
export const dupRatio = (file: VizFile): number => {
  // ~34 bytes per line is a stable enough estimate for a ratio; the
  // absolute number is shown separately in the panel.
  const approxLines = Math.max(1, file.size / 34);
  return Math.min(1, file.dup_lines / approxLines);
};

// ── Lens coloring ───────────────────────────────────────────────

/** Fill color for one file under the active lens. */
export const lensColor = (lens: Lens, theme: Theme, index: DataIndex, file: VizFile): string => {
  switch (lens) {
    case "overview":
      return file.status === "entryPoint" ? theme.cellEntry : theme.cellNeutral;
    case "deadcode":
      switch (file.status) {
        case "unused":
          return theme.red;
        case "hasUnusedExports":
          return theme.amber;
        case "entryPoint":
          return theme.cellEntry;
        default:
          return theme.cellNeutral;
      }
    case "dupes":
      return file.dup_lines > 0
        ? dupRamp(theme, dupRatio(file) / index.dupCeiling)
        : theme.cellNeutral;
    case "boundaries":
      return zoneColor(theme, file.zone);
    case "hotspots": {
      // Floor at cc 3 so trivial functions stay neutral and real
      // complexity glows.
      const intensity = (file.max_cyclomatic - 3) / Math.max(1, index.heatCeiling - 3);
      return intensity > 0 ? heatRamp(theme, intensity) : theme.cellNeutral;
    }
  }
};

/**
 * Non-color finding channel for the active lens: 2 = severe (dense
 * hatch in the treemap, double ring in the graph), 1 = mild (light
 * hatch, dashed ring), 0 = none. Thresholds mirror the panel's
 * sev-error/sev-warn split so texture and text never disagree.
 */
export const lensFindingLevel = (
  lens: Lens,
  index: DataIndex,
  file: VizFile,
  fileIdx: number,
): 0 | 1 | 2 => {
  switch (lens) {
    case "overview":
      return 0;
    case "deadcode":
      if (file.status === "unused") return 2;
      return file.unused_export_count > 0 ? 1 : 0;
    case "dupes":
      if (dupRatio(file) >= 0.3) return 2;
      return file.dup_lines > 0 ? 1 : 0;
    case "boundaries":
      return index.violationSources.has(fileIdx) ? 2 : 0;
    case "hotspots":
      if (file.max_cyclomatic >= 20) return 2;
      return file.max_cyclomatic >= 10 ? 1 : 0;
  }
};

/**
 * One-line canvas legend for the active lens, shared by both views so
 * their vocabulary cannot drift. Zero-findings lenses explain the
 * neutral map instead of advertising absent colors.
 */
export const legendText = (lens: Lens, data: VizData, view: "map" | "graph"): string => {
  const summary = data.summary;
  // Boundaries colors every node by its architecture zone, so it is never
  // "neutral" even with zero violations; the graph view draws a zone color key
  // (see drawZoneLegend), and this is the treemap-view / fallback wording.
  if (lens === "boundaries") {
    return summary.circular_deps + summary.boundary_violations > 0
      ? "Each color is an architecture layer. Red marks a forbidden import or a loop."
      : "Each color is an architecture layer.";
  }
  const findings: Record<Lens, number> = {
    overview: -1,
    deadcode: summary.unused_files + summary.unused_exports,
    dupes: summary.clone_groups,
    boundaries: summary.circular_deps + summary.boundary_violations,
    hotspots: summary.hotspot_files,
  };
  if (findings[lens] === 0) {
    return "No findings in this lens, so the map keeps its neutral colors.";
  }
  if (lens === "overview") {
    return view === "map"
      ? "Each tile is a file, sized by bytes on disk. A blue outline marks an entry point."
      : "Each dot is a file, sized by bytes. Blue marks an entry point; a line's thick end is the importer.";
  }
  const lines: Record<Lens, string> = {
    overview: "",
    deadcode: "Red is never imported, amber has unused exports.",
    dupes: "Deeper amber means more duplicated lines.",
    boundaries:
      "Red is a forbidden import or part of a loop. An amber outline marks folders that import each other.",
    hotspots: "Amber through red: harder to change safely.",
  };
  return lines[lens];
};

/**
 * Transitive reach over an adjacency list from `start` (the start file
 * itself is excluded). With `importsOf` this is everything the file
 * pulls in; with `importersOf` it is the file's blast radius, every
 * file that transitively depends on it.
 */
export const reachSet = (adj: number[][], start: number): Set<number> => {
  const seen = new Set<number>();
  const stack: number[] = [start];
  while (stack.length > 0) {
    const cur = stack.pop();
    if (cur === undefined) break;
    for (const neighbor of adj[cur] ?? []) {
      if (neighbor !== start && !seen.has(neighbor)) {
        seen.add(neighbor);
        stack.push(neighbor);
      }
    }
  }
  return seen;
};

/**
 * Union transitive reach over `adj` from every index in `starts`, in a
 * single multi-source traversal (the seeds themselves are excluded from
 * the result). One O(V+E) pass no matter how many seeds, so it replaces
 * looping `reachSet` per match with no per-match cap. With `importersOf`
 * this is the combined blast radius of the whole matched set.
 */
export const reachSetMulti = (adj: number[][], starts: Iterable<number>): Set<number> => {
  const seeds = new Set<number>(starts);
  const seen = new Set<number>();
  const stack: number[] = [...seeds];
  while (stack.length > 0) {
    const cur = stack.pop();
    if (cur === undefined) break;
    for (const neighbor of adj[cur] ?? []) {
      if (!seeds.has(neighbor) && !seen.has(neighbor)) {
        seen.add(neighbor);
        stack.push(neighbor);
      }
    }
  }
  return seen;
};

// ── Formatting helpers ──────────────────────────────────────────

export const formatSize = (bytes: number): string => {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
};

export const formatCount = (count: number): string => count.toLocaleString("en-US");

export const basename = (path: string): string => path.split("/").pop() ?? path;

export const dirname = (path: string): string => {
  const slashIndex = path.lastIndexOf("/");
  return slashIndex === -1 ? "" : path.slice(0, slashIndex);
};
