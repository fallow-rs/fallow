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
  architecture: VizFindingAnalysis;
  dependencies: VizFindingAnalysis;
  health: VizHealthData;
  security: VizSecurityData;
  frameworks: VizFrameworkData;
  styling: VizStylingData;
  feature_flags: VizFindingAnalysis;
}

export type VizAvailabilityState = "complete" | "disabled" | "notApplicable" | "unavailable";

export interface VizAvailability {
  state: VizAvailabilityState;
  count: number;
  unit: string;
  reason?: string;
  truncated?: number;
}

export interface VizFinding {
  kind: string;
  title: string;
  file?: number;
  path?: string;
  line?: number;
  files?: number[];
  paths?: string[];
  description?: string;
  severity?: string;
  facts?: VizFindingFact[];
  actions: VizFindingAction[];
}

export interface VizFindingFact {
  label: string;
  value: string;
}

export interface VizFindingAction {
  label: string;
  kind?: string;
  auto_fixable: boolean;
  command?: string;
  comment?: string;
  config_key?: string;
  value?: unknown;
  description?: string;
}

export interface VizFindingAnalysis {
  availability: VizAvailability;
  findings_truncated?: number;
  findings: VizFinding[];
}

export interface VizFrameworkDetector {
  id: string;
  framework: string;
  status: string;
  reason?: string;
}

export interface VizFrameworkData extends VizFindingAnalysis {
  detector_availability: VizAvailability;
  detected_frameworks: string[];
  detectors: VizFrameworkDetector[];
}

export interface VizHealthCapabilities {
  complexity: VizAvailability;
  maintainability: VizAvailability;
  crap: VizAvailability;
  coverage: VizAvailability;
  runtime: VizAvailability;
  churn: VizAvailability;
  hotspots: VizAvailability;
  ownership: VizAvailability;
}

export interface VizHealthFile {
  file: number;
  path: string;
  maintainability_index: number;
  crap_max: number;
  complexity_density: number;
  fan_in: number;
  fan_out: number;
  hotspot_score?: number;
  commits?: number;
  ownership?: unknown;
}

export interface VizHealthData {
  availability: VizAvailability;
  capabilities: VizHealthCapabilities;
  shared_parse?: boolean;
  score?: number;
  grade?: string;
  average_maintainability?: number;
  files: VizHealthFile[];
  files_truncated?: number;
  findings_truncated?: number;
  findings: VizFinding[];
}

export interface VizStylingData extends VizFindingAnalysis {
  score?: number;
  grade?: string;
  confidence?: string;
  summary?: unknown;
}

export interface VizSecurityTraceHop {
  file?: number;
  path: string;
  line: number;
  col: number;
  role: string;
}

export interface VizSecurityEndpoint {
  file?: number;
  path: string;
  line: number;
  col: number;
}

export interface VizSecurityTaintFlow {
  source: VizSecurityEndpoint;
  sink: VizSecurityEndpoint;
  intra_module: boolean;
  cross_module_hops: number;
}

export interface VizSecurityCandidate {
  id: string;
  kind: string;
  category?: string;
  cwe?: number;
  file?: number;
  path: string;
  line: number;
  col: number;
  evidence: string;
  severity: string;
  taint_confidence?: string;
  source_kind?: string;
  sink?: string;
  url_shape?: string;
  network_destination?: string;
  reachable_from_entry?: boolean;
  reachable_from_untrusted_source?: boolean;
  blast_radius?: number;
  crosses_boundary: boolean;
  client_server_boundary: boolean;
  cross_module_boundary: boolean;
  architecture_zone?: string;
  dead_code?: unknown;
  runtime?: unknown;
  taint_flow?: VizSecurityTaintFlow;
  observed_controls?: unknown[];
  control_verification_prompt?: string;
  trace: VizSecurityTraceHop[];
  taint_trace?: VizSecurityTraceHop[];
  actions: unknown;
}

export interface VizSecurityBlindSpot {
  kind: string;
  count: number;
  path?: string;
  file?: number;
  line?: number;
  reason?: string;
}

export interface VizSecurityData {
  availability: VizAvailability;
  runtime_availability: VizAvailability;
  candidates: VizSecurityCandidate[];
  blind_spot_count: number;
  blind_spots_truncated?: number;
  blind_spots: VizSecurityBlindSpot[];
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

export type Lens = "overview" | "unused" | "duplication" | "architecture" | "health" | "security";

export type SecondaryAnalysis = "dependencies" | "frameworks" | "styling" | "feature_flags";

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
