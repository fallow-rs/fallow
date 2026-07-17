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

const packEdge = (n: number, from: number, to: number): number => from * n + to;

const percentile = (values: number[], p: number): number => {
  if (values.length === 0) return 0;
  const sorted = [...values].sort((a, b) => a - b);
  const idx = Math.min(sorted.length - 1, Math.floor(sorted.length * p));
  return sorted[idx];
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

  for (let i = 0; i < files.length; i++) {
    const parts = files[i].path.split("/");
    let node = root;
    let prefix = "";
    for (let d = 0; d < parts.length - 1; d++) {
      prefix = prefix ? `${prefix}/${parts[d]}` : parts[d];
      let child = byPath.get(prefix);
      if (!child) {
        child = {
          name: parts[d],
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
      path: files[i].path,
      size: Math.max(1, files[i].size),
      children: [],
      fileIndex: i,
      parent: node,
    };
    node.children.push(leaf);
  }

  // Roll up sizes bottom-up and sort children by size (largest first).
  const rollup = (node: TreeNode): number => {
    if (node.fileIndex !== null) return node.size;
    node.size = node.children.reduce((sum, c) => sum + rollup(c), 0);
    node.children.sort((a, b) => b.size - a.size);
    return node.size;
  };
  rollup(root);

  // Collapse single-child directory chains (src -> src/components).
  const collapse = (node: TreeNode): void => {
    for (let i = 0; i < node.children.length; i++) {
      let child = node.children[i];
      while (child.fileIndex === null && child.children.length === 1 && child.children[0].fileIndex === null) {
        const grand = child.children[0];
        grand.name = `${child.name}/${grand.name}`;
        grand.parent = node;
        node.children[i] = grand;
        child = grand;
      }
      collapse(node.children[i]);
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
  const n = data.files.length;
  const importersOf: number[][] = Array.from({ length: n }, () => []);
  const importsOf: number[][] = Array.from({ length: n }, () => []);
  for (const [from, to] of data.edges) {
    if (from >= n || to >= n) continue;
    importsOf[from].push(to);
    importersOf[to].push(from);
  }

  const cycleEdges = new Set<number>();
  for (const cycle of data.cycles) {
    for (let i = 0; i < cycle.length; i++) {
      const from = cycle[i];
      const to = cycle[(i + 1) % cycle.length];
      cycleEdges.add(packEdge(n, from, to));
      cycleEdges.add(packEdge(n, to, from));
    }
  }

  const violationEdges = new Map<number, number[]>();
  const violationSources = new Set<number>();
  for (let v = 0; v < data.violations.length; v++) {
    const { from, to } = data.violations[v];
    const key = packEdge(n, from, to);
    const list = violationEdges.get(key);
    if (list) list.push(v);
    else violationEdges.set(key, [v]);
    violationSources.add(from);
  }

  const dupRatios = data.files
    .filter((f) => f.dup_lines > 0)
    .map((f) => dupRatio(f));
  const heats = data.files
    .filter((f) => f.max_cyclomatic > 0)
    .map((f) => f.max_cyclomatic);

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
export const lensColor = (
  lens: Lens,
  theme: Theme,
  index: DataIndex,
  file: VizFile,
): string => {
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
      const t = (file.max_cyclomatic - 3) / Math.max(1, index.heatCeiling - 3);
      return t > 0 ? heatRamp(theme, t) : theme.cellNeutral;
    }
  }
};

/** Whether the file carries a finding under the active lens (drives texture). */
export const lensFlag = (lens: Lens, index: DataIndex, file: VizFile, fileIdx: number): boolean => {
  switch (lens) {
    case "overview":
      return false;
    case "deadcode":
      return file.status === "unused";
    case "dupes":
      return false;
    case "boundaries":
      return index.violationSources.has(fileIdx);
    case "hotspots":
      return false;
  }
};

/**
 * One-line canvas legend for the active lens, shared by both views so
 * their vocabulary cannot drift. Zero-findings lenses explain the
 * neutral map instead of advertising absent colors.
 */
export const legendText = (lens: Lens, data: VizData, view: "map" | "graph"): string => {
  const s = data.summary;
  const findings: Record<Lens, number> = {
    overview: -1,
    deadcode: s.unused_files + s.unused_exports,
    dupes: s.clone_groups,
    boundaries: s.circular_deps + s.boundary_violations,
    hotspots: s.hotspot_files,
  };
  if (findings[lens] === 0) {
    return "no findings in this lens · the map keeps its neutral colors";
  }
  if (lens === "overview") {
    return view === "map"
      ? "tile = file · area = bytes on disk · blue outline = entry point"
      : "dot = file, sized by bytes · blue = entry point · lines = imports, thick end is the importer · zoom in for more labels";
  }
  const lines: Record<Lens, string> = {
    overview: "",
    deadcode: "red = never imported · amber = has unused exports",
    dupes: "deeper amber = more duplicated lines",
    boundaries: "red = crosses a boundary rule or joins a cycle · amber outline = tangled folders",
    hotspots: "amber → red = harder to change safely",
  };
  return lines[lens];
};

// ── Formatting helpers ──────────────────────────────────────────

export const formatSize = (bytes: number): string => {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
};

export const formatCount = (n: number): string => n.toLocaleString("en-US");

export const basename = (path: string): string => path.split("/").pop() ?? path;

export const dirname = (path: string): string => {
  const idx = path.lastIndexOf("/");
  return idx === -1 ? "" : path.slice(0, idx);
};
