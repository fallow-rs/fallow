/** Mirrors the Rust VizData struct */
export interface VizData {
  root: string;
  files: VizFile[];
  edges: [number, number][];
  summary: VizSummary;
  workspaces: VizWorkspace[];
  cycles: number[][];
}

export interface VizFile {
  path: string;
  size: number;
  status: VizFileStatus;
  export_count: number;
  unused_export_count: number;
  is_entry: boolean;
  importer_count: number;
  import_count: number;
  workspace: string;
  unused_exports?: string[];
}

export type VizFileStatus =
  | "clean"
  | "hasUnusedExports"
  | "unused"
  | "entryPoint";

export interface VizSummary {
  total_files: number;
  total_size: number;
  unused_files: number;
  unused_exports: number;
  unused_types: number;
  unused_deps: number;
  unresolved_imports: number;
  circular_deps: number;
}

// fallow-ignore-next-line unused-type
export interface VizWorkspace {
  name: string;
  root: string;
}

/** A node in the directory tree hierarchy */
export interface TreeNode {
  name: string;
  path: string;
  size: number;
  children: TreeNode[];
  /** Index into VizData.files, only set for leaf (file) nodes */
  fileIndex: number | null;
}

/** A laid-out rectangle from the squarify algorithm */
export interface LayoutNode {
  x: number;
  y: number;
  w: number;
  h: number;
  node: TreeNode;
}

/** A positioned node in the graph view */
export interface GraphNode {
  fileIndex: number;
  x: number;
  y: number;
  vx: number;
  vy: number;
  radius: number;
  /** Directory group for clustering */
  group: string;
}

export type ActiveView = "treemap" | "graph";

declare global {
  interface Window {
    __FALLOW_DATA__: VizData;
  }
}
