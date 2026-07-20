/** Mirrors the Rust `fallow_engine::viz::VizData` contract. */
export interface VizData {
  root: string;
  files: VizFile[];
  /** Import edges as [from, to, flags]; flags bit 0 = all imports type-only. */
  edges: [number, number, number][];
  summary: VizSummary;
  workspaces: VizWorkspace[];
  zones: VizZone[];
  cycles: number[][];
  clones: VizCloneGroup[];
  violations: VizViolation[];
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
  workspace?: number;
  zone?: number;
  unused_exports?: string[];
  fn_count: number;
  max_cyclomatic: number;
  max_cognitive: number;
  react_hooks: number;
  jsx_depth: number;
  functions?: VizFunction[];
  dup_lines: number;
  clone_groups?: number[];
  in_cycle: boolean;
}

export type VizFileStatus = "clean" | "hasUnusedExports" | "unused" | "entryPoint";

export interface VizFunction {
  name: string;
  line: number;
  cyclomatic: number;
  cognitive: number;
  lines: number;
  hooks: number;
  jsx_depth: number;
  props: number;
}

export interface VizSummary {
  total_files: number;
  total_size: number;
  total_edges: number;
  unused_files: number;
  unused_exports: number;
  unused_types: number;
  unused_deps: number;
  unresolved_imports: number;
  circular_deps: number;
  clone_groups: number;
  duplicated_lines: number;
  boundary_violations: number;
  hotspot_files: number;
  /** Clone groups dropped by the payload cap; absent when nothing was truncated. */
  clone_groups_truncated?: number;
}

export interface VizWorkspace {
  name: string;
  root: string;
}

export interface VizZone {
  name: string;
  files: number;
}

export interface VizCloneGroup {
  lines: number;
  tokens: number;
  instances: VizCloneInstance[];
  /** Context window around the copied block: the copied lines flanked by
   *  a few surrounding source lines on each side. */
  preview: string;
  /** 0-based index, among `preview` lines, of the first copied line. */
  highlight_start: number;
  /** Number of copied lines in `preview`; the rest are dimmed context. */
  highlight_lines: number;
}

export interface VizCloneInstance {
  file: number;
  start_line: number;
  end_line: number;
}

export interface VizViolation {
  from: number;
  to: number;
  from_zone: number;
  to_zone: number;
  line: number;
  specifier: string;
}

/** A node in the directory-tree hierarchy (built once for the full project). */
export interface TreeNode {
  name: string;
  /** Full path from the project root ("" for the root node). */
  path: string;
  size: number;
  children: TreeNode[];
  /** Index into VizData.files; null for directories. */
  fileIndex: number | null;
  parent: TreeNode | null;
}

/** A laid-out treemap rectangle. */
export interface LayoutCell {
  x: number;
  y: number;
  w: number;
  h: number;
  node: TreeNode;
  depth: number;
}

export type ActiveView = "map" | "graph";

export type Lens = "overview" | "deadcode" | "dupes" | "boundaries" | "hotspots";

/** A drilled-down aggregated road (cluster-to-cluster import bundle). */
export interface RoadSelection {
  srcKey: string;
  dstKey: string;
  count: number;
  violations: number;
  cycleEdges: number;
  /** Contributing file edges as [from, to] file indices. */
  pairs: Array<[number, number]>;
}

declare global {
  interface Window {
    __FALLOW_DATA__: VizData;
  }
}
