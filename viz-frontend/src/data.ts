import type {
  Lens,
  TreeNode,
  VizAvailability,
  VizData,
  VizFile,
  VizFinding,
  VizHealthFile,
  VizSecurityCandidate,
} from "./types";
import type { Theme } from "./theme";
import { dupRamp, heatRamp, zoneColor } from "./theme";

export type AnalysisId =
  | "unused"
  | "duplication"
  | "architecture"
  | "health"
  | "security"
  | "dependencies"
  | "frameworks"
  | "styling"
  | "flags";

export type AnalysisState = "complete" | "disabled" | "notApplicable" | "unavailable";

export interface AnalysisAvailabilityView {
  state: AnalysisState;
  count: number;
  unit: string;
  reason?: string;
  truncated?: number;
}

export interface FindingActionView {
  label: string;
  command?: string;
  description?: string;
}

export interface SecurityCandidateView {
  id: string;
  fileIndex: number | null;
  path: string;
  line: number | null;
  column: number | null;
  title: string;
  category: string | null;
  cwe: string | null;
  severity: string;
  confidence: string | null;
  evidence: string | null;
  source: string | null;
  sink: string | null;
  urlShape: string | null;
  networkDestination: string | null;
  trace: string[];
  taintFlow: string | null;
  boundary: string | null;
  reachability: string | null;
  blastRadius: number | null;
  deadCode: boolean | null;
  runtime: string | null;
  observedControls: string[];
  verificationPrompt: string | null;
  actions: FindingActionView[];
}

export interface GenericFindingView {
  kind: string;
  title: string;
  detail: string | null;
  severity: string | null;
  fileIndex: number | null;
  fileIndices: number[];
  path: string | null;
  paths: string[];
  line: number | null;
  metrics: Array<[string, string]>;
  actions: FindingActionView[];
}

export interface SecurityBlindSpotView {
  kind: string;
  count: number;
  path: string | null;
  line: number | null;
  reason: string | null;
}

type UnknownRecord = Record<string, unknown>;

const asRecord = (value: unknown): UnknownRecord | null =>
  typeof value === "object" && value !== null && !Array.isArray(value)
    ? (value as UnknownRecord)
    : null;

const stringValue = (record: UnknownRecord, ...keys: string[]): string | null => {
  for (const key of keys) {
    const value = record[key];
    if (typeof value === "string" && value.trim() !== "") return value;
  }
  return null;
};

const displayValue = (value: unknown): string | null => {
  if (typeof value === "string") return value.trim() === "" ? null : value;
  if (typeof value === "number" || typeof value === "boolean") return String(value);
  if (value === null || value === undefined) return null;
  try {
    const serialized = JSON.stringify(value);
    return serialized.length > 500 ? `${serialized.slice(0, 497)}...` : serialized;
  } catch {
    return null;
  }
};

const humanLabel = (value: string): string =>
  value.replaceAll(/[_-]+/g, " ").replaceAll(/\b\w/g, (letter) => letter.toUpperCase());

const descriptiveValue = (value: unknown): string | null => {
  const record = asRecord(value);
  if (!record) return displayValue(value);
  const parts = Object.entries(record).flatMap(([key, entry]): string[] => {
    const displayed = displayValue(entry);
    return displayed === null || typeof entry === "object"
      ? []
      : [`${humanLabel(key)}: ${displayed}`];
  });
  return parts.length > 0 ? parts.slice(0, 6).join(", ") : null;
};

const availabilityView = (availability: VizAvailability): AnalysisAvailabilityView => ({
  state: availability.state,
  count: availability.count,
  unit: availability.unit,
  ...(availability.reason ? { reason: availability.reason } : {}),
  ...(availability.truncated === undefined ? {} : { truncated: availability.truncated }),
});

/** Read one analysis capability without confusing missing analysis with zero findings. */
export const analysisAvailability = (data: VizData, id: AnalysisId): AnalysisAvailabilityView => {
  switch (id) {
    case "unused":
      return {
        state: "complete",
        count: data.files.filter((file) => file.status === "unused" || file.unused_export_count > 0)
          .length,
        unit: "files",
      };
    case "duplication":
      return { state: "complete", count: data.summary.clone_groups, unit: "clone groups" };
    case "architecture":
      return availabilityView(data.architecture.availability);
    case "dependencies":
      return availabilityView(data.dependencies.availability);
    case "health":
      return availabilityView(data.health.availability);
    case "security":
      return availabilityView(data.security.availability);
    case "frameworks":
      return availabilityView(data.frameworks.availability);
    case "styling":
      return availabilityView(data.styling.availability);
    case "flags":
      return availabilityView(data.feature_flags.availability);
  }
};

const fileMatches = (
  data: VizData,
  record: { file?: number; path?: string; files?: number[]; paths?: string[] },
  fileIndex: number,
): boolean => {
  if (record.file === fileIndex || record.files?.includes(fileIndex)) return true;
  const path = data.files[fileIndex]?.path;
  return path !== undefined && (record.path === path || record.paths?.includes(path) === true);
};

const actionViews = (value: unknown): FindingActionView[] => {
  const record = asRecord(value);
  const directCommand = record
    ? stringValue(record, "command", "verify_command", "trace_command")
    : null;
  const direct = directCommand ? [{ label: "Verify", command: directCommand }] : [];
  const raw = record?.actions ?? value;
  if (Array.isArray(raw)) {
    return [
      ...direct,
      ...raw.flatMap((entry): FindingActionView[] => {
        const action = asRecord(entry);
        if (!action) return [];
        const command = stringValue(action, "command", "value");
        const description = stringValue(action, "description", "note");
        const label = humanLabel(stringValue(action, "label", "title", "type") ?? "Review");
        if (command) return [{ label, command }];
        return description ? [{ label, description }] : [];
      }),
    ];
  }
  const actions = asRecord(raw);
  if (!actions) return direct;
  return [
    ...direct,
    ...Object.entries(actions).flatMap(([label, command]): FindingActionView[] =>
      typeof command === "string" ? [{ label: humanLabel(label), command }] : [],
    ),
  ];
};

const reachabilityLabel = (candidate: VizSecurityCandidate): string | null => {
  const labels = [
    candidate.reachable_from_entry ? "entry point" : null,
    candidate.reachable_from_untrusted_source ? "untrusted source" : null,
  ].filter((label) => label !== null);
  if (labels.length > 0) return labels.join(", ");
  if (
    candidate.reachable_from_entry !== undefined ||
    candidate.reachable_from_untrusted_source !== undefined
  ) {
    return "not established";
  }
  return null;
};

/** Normalized static Security candidates for a file. */
export const securityCandidatesForFile = (
  data: VizData,
  fileIndex: number,
): SecurityCandidateView[] => {
  return data.security.candidates
    .filter((candidate) => fileMatches(data, candidate, fileIndex))
    .map((candidate) => {
      const boundaryLabels = [
        candidate.crosses_boundary ? "trust boundary" : null,
        candidate.client_server_boundary ? "client/server" : null,
        candidate.cross_module_boundary ? "module boundary" : null,
        candidate.architecture_zone ?? null,
      ].filter((label) => label !== null);
      return {
        id: candidate.id,
        fileIndex: candidate.file ?? null,
        path: candidate.path,
        line: candidate.line,
        column: candidate.col,
        title: candidate.category ?? candidate.kind,
        category: candidate.category ?? candidate.kind,
        cwe: candidate.cwe === undefined ? null : `CWE-${candidate.cwe}`,
        severity: candidate.severity,
        confidence: candidate.taint_confidence ?? null,
        evidence: candidate.evidence,
        source: candidate.source_kind ?? null,
        sink: candidate.sink ?? null,
        urlShape: candidate.url_shape ?? null,
        networkDestination: candidate.network_destination ?? null,
        trace: [
          ...new Set(
            [...candidate.trace, ...(candidate.taint_trace ?? [])].map(
              (step) => `${step.role}: ${step.path}:${step.line}:${step.col}`,
            ),
          ),
        ],
        taintFlow: candidate.taint_flow
          ? `${candidate.taint_flow.source.path}:${candidate.taint_flow.source.line} to ${candidate.taint_flow.sink.path}:${candidate.taint_flow.sink.line}, ${candidate.taint_flow.intra_module ? "same module" : `${candidate.taint_flow.cross_module_hops} module hops`}`
          : null,
        boundary: boundaryLabels.length > 0 ? boundaryLabels.join(", ") : null,
        reachability: reachabilityLabel(candidate),
        blastRadius: candidate.blast_radius ?? null,
        deadCode:
          candidate.dead_code === undefined || candidate.dead_code === null
            ? null
            : candidate.dead_code !== false,
        runtime: displayValue(candidate.runtime),
        observedControls: (candidate.observed_controls ?? []).flatMap((control): string[] => {
          const displayed = descriptiveValue(control);
          return displayed ? [displayed] : [];
        }),
        verificationPrompt: candidate.control_verification_prompt ?? null,
        actions: [
          {
            label: "Verify",
            command: `fallow security --file ${JSON.stringify(candidate.path)}`,
          },
          ...actionViews(candidate.actions),
        ],
      };
    });
};

/** Runtime Security availability is independent of static candidate analysis. */
export const securityRuntimeAvailability = (data: VizData): AnalysisAvailabilityView => {
  return availabilityView(data.security.runtime_availability);
};

const blindSpotView = (
  blindSpot: VizData["security"]["blind_spots"][number],
): SecurityBlindSpotView => ({
  kind: blindSpot.kind,
  count: blindSpot.count,
  path: blindSpot.path ?? null,
  line: blindSpot.line ?? null,
  reason: blindSpot.reason ?? null,
});

export const securityBlindSpots = (data: VizData): SecurityBlindSpotView[] =>
  data.security.blind_spots.map(blindSpotView);

export const securityBlindSpotsForFile = (
  data: VizData,
  fileIndex: number,
): SecurityBlindSpotView[] =>
  data.security.blind_spots
    .filter((blindSpot) => fileMatches(data, blindSpot, fileIndex))
    .map(blindSpotView);

const analysisFindings = (
  data: VizData,
  id: Exclude<AnalysisId, "unused" | "duplication" | "security" | "health">,
): VizFinding[] => {
  switch (id) {
    case "architecture":
      return data.architecture.findings;
    case "dependencies":
      return data.dependencies.findings;
    case "frameworks":
      return data.frameworks.findings;
    case "styling":
      return data.styling.findings;
    case "flags":
      return data.feature_flags.findings;
  }
};

const findingView = (finding: VizFinding): GenericFindingView => {
  return {
    kind: finding.kind,
    title: finding.title,
    detail: finding.description ?? null,
    severity: finding.severity ?? null,
    fileIndex: finding.file ?? null,
    fileIndices: finding.files ?? [],
    path: finding.path ?? null,
    paths: finding.paths ?? [],
    line: finding.line ?? null,
    metrics: (finding.facts ?? []).map((fact) => [humanLabel(fact.label), fact.value]),
    actions: finding.actions.map((action) => ({
      label: `${humanLabel(action.label)}${action.auto_fixable === true ? " (auto-fixable)" : ""}`,
      ...(action.command || action.comment ? { command: action.command ?? action.comment } : {}),
      ...(action.description || action.config_key
        ? {
            description:
              action.description ??
              `${action.config_key}: ${displayValue(action.value) ?? "review configuration"}`,
          }
        : {}),
    })),
  };
};

const healthFileView = (file: VizHealthFile): GenericFindingView => ({
  kind: "file-health",
  title: "File health",
  detail: file.ownership === undefined ? null : `Ownership: ${displayValue(file.ownership)}`,
  severity: (file.hotspot_score ?? file.crap_max) >= 20 ? "high" : null,
  fileIndex: file.file,
  fileIndices: [file.file],
  path: file.path,
  paths: [file.path],
  line: null,
  metrics: [
    ["Maintainability", String(file.maintainability_index)],
    ["CRAP", String(file.crap_max)],
    ["Complexity density", String(file.complexity_density)],
    ["Fan in", String(file.fan_in)],
    ["Fan out", String(file.fan_out)],
    ...(file.hotspot_score === undefined
      ? []
      : ([["Hotspot", String(file.hotspot_score)]] as Array<[string, string]>)),
    ...(file.commits === undefined
      ? []
      : ([["Churn", String(file.commits)]] as Array<[string, string]>)),
  ],
  actions: [],
});

/** Capability-aware secondary and health findings, normalized for panel cards. */
export const findingsForAnalysis = (
  data: VizData,
  id: Exclude<AnalysisId, "unused" | "duplication" | "security">,
): GenericFindingView[] => {
  if (id === "health") {
    return [...data.health.files.map(healthFileView), ...data.health.findings.map(findingView)];
  }
  return analysisFindings(data, id).map(findingView);
};

export const findingsForFile = (
  data: VizData,
  id: Exclude<AnalysisId, "unused" | "duplication" | "security">,
  fileIndex: number,
): GenericFindingView[] => {
  const path = data.files[fileIndex]?.path;
  return findingsForAnalysis(data, id).filter(
    (finding) =>
      finding.fileIndex === fileIndex ||
      finding.fileIndices.includes(fileIndex) ||
      (path !== undefined && (finding.path === path || finding.paths.includes(path))),
  );
};

/** Health magnitude for map coloring. Returns null when Health was not retained. */
export const healthRiskForFile = (data: VizData, fileIndex: number): number | null => {
  const file = data.health.files.find((candidate) => fileMatches(data, candidate, fileIndex));
  return file ? (file.hotspot_score ?? file.crap_max) : null;
};

/** Whether Health emitted a threshold, coverage, or hotspot finding for a file. */
export const healthHasFindingForFile = (data: VizData, fileIndex: number): boolean =>
  data.health.findings.some((finding) => fileMatches(data, finding, fileIndex));

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
  /** Normalization ceiling for the Health lens (p95 retained risk). */
  heatCeiling: number;
  /** Static Security candidate severity by file: 0 none, 1 review, 2 high priority. */
  securityLevels: Array<0 | 1 | 2>;
  /** Architecture finding severity by file, including non-boundary records. */
  architectureLevels: Array<0 | 1 | 2>;
  /** Retained Health risk magnitude by file; null means no file-level Health artifact. */
  healthRisks: Array<number | null>;
  /** Files with a retained Health finding even when no file score is present. */
  healthFindingFiles: Set<string>;
  /** Root-relative file path to payload index. */
  fileIndexByPath: Map<string, number>;
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
  const heats = data.health.files.map((file) => file.hotspot_score ?? file.crap_max);

  const { root, byPath } = buildTree(data.files);
  const securityLevels = data.files.map((_, fileIndex): 0 | 1 | 2 => {
    const candidates = securityCandidatesForFile(data, fileIndex);
    if (
      candidates.some((candidate) =>
        ["critical", "high", "error"].includes(candidate.severity.toLowerCase()),
      )
    ) {
      return 2;
    }
    return candidates.length > 0 ? 1 : 0;
  });
  const healthRisks = data.files.map((_, fileIndex) => healthRiskForFile(data, fileIndex));
  const architectureLevels = data.files.map((_, fileIndex): 0 | 1 | 2 =>
    violationSources.has(fileIndex) || findingsForFile(data, "architecture", fileIndex).length > 0
      ? 2
      : 0,
  );
  const healthFindingFiles = new Set(
    data.health.findings.flatMap((finding): string[] => {
      const paths = [
        ...(finding.file !== undefined && data.files[finding.file]
          ? [data.files[finding.file].path]
          : []),
        ...(finding.files ?? []).flatMap((file) => data.files[file]?.path ?? []),
        ...(finding.path ? [finding.path] : []),
        ...(finding.paths ?? []),
      ];
      return [...new Set(paths)];
    }),
  );
  const fileIndexByPath = new Map(data.files.map((file, fileIndex) => [file.path, fileIndex]));

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
    securityLevels,
    architectureLevels,
    healthRisks,
    healthFindingFiles,
    fileIndexByPath,
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
  const lensId = lens as string;
  switch (lensId) {
    case "overview":
      return file.status === "entryPoint" ? theme.cellEntry : theme.cellNeutral;
    case "unused":
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
    case "duplication":
      return file.dup_lines > 0
        ? dupRamp(theme, dupRatio(file) / index.dupCeiling)
        : theme.cellNeutral;
    case "architecture":
      return zoneColor(theme, file.zone);
    case "health": {
      const risk = index.healthRisks[index.fileIndexByPath.get(file.path) ?? -1];
      if (risk === null || risk === undefined) return theme.cellNeutral;
      return heatRamp(theme, risk / Math.max(30, index.heatCeiling));
    }
    case "security": {
      const level = index.securityLevels[index.fileIndexByPath.get(file.path) ?? -1] ?? 0;
      if (level === 2) return theme.red;
      if (level === 1) return theme.amber;
      return theme.cellNeutral;
    }
    default:
      return theme.cellNeutral;
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
  const lensId = lens as string;
  switch (lensId) {
    case "overview":
      return 0;
    case "unused":
      if (file.status === "unused") return 2;
      return file.unused_export_count > 0 ? 1 : 0;
    case "duplication":
      if (dupRatio(file) >= 0.3) return 2;
      return file.dup_lines > 0 ? 1 : 0;
    case "architecture":
      return index.architectureLevels[fileIdx] ?? 0;
    case "health": {
      const risk = index.healthRisks[fileIdx];
      if (risk === null || risk === undefined) {
        return index.healthFindingFiles.has(file.path) ? 1 : 0;
      }
      if (risk >= 20) return 2;
      return risk >= 10 ? 1 : 0;
    }
    case "security":
      return index.securityLevels[fileIdx] ?? 0;
    default:
      return 0;
  }
};

/**
 * One-line canvas legend for the active lens, shared by both views so
 * their vocabulary cannot drift. Zero-findings lenses explain the
 * neutral map instead of advertising absent colors.
 */
export const legendText = (lens: Lens, data: VizData, view: "map" | "graph"): string => {
  const summary = data.summary;
  const lensId = lens as string;
  if (lensId === "overview") {
    return view === "map"
      ? "Each tile is a file, sized by bytes on disk. A blue outline marks an entry point."
      : "Each dot is a file, sized by bytes. Blue marks an entry point; a line's thick end is the importer.";
  }
  const analysisId: AnalysisId | null =
    lensId === "unused"
      ? "unused"
      : lensId === "duplication"
        ? "duplication"
        : lensId === "architecture"
          ? "architecture"
          : lensId === "health"
            ? "health"
            : lensId === "security"
              ? "security"
              : null;
  if (analysisId) {
    const availability = analysisAvailability(data, analysisId);
    if (availability.state !== "complete") {
      const reason = availability.reason ?? `${linesTitle(analysisId)} analysis is unavailable.`;
      return `${reason} The map keeps its neutral colors.`;
    }
    let count = availability.count;
    if (analysisId === "architecture") {
      count += summary.circular_deps;
      if (count === 0) return "Each color is an architecture layer. No violations or loops.";
      return "Each color is an architecture layer. Red marks a forbidden import or a loop.";
    }
    if (count === 0) return "No findings in this lens, so the map keeps its neutral colors.";
  }
  if (analysisId === null) {
    return "No findings in this lens, so the map keeps its neutral colors.";
  }
  const lines: Record<AnalysisId, string> = {
    unused: "Red is never imported, amber has unused exports.",
    duplication: "Deeper amber means more duplicated lines.",
    architecture:
      "Red is a forbidden import or part of a loop. An amber outline marks folders that import each other.",
    health: "Amber through red marks retained Health findings.",
    security: "Amber and red mark static Security candidates by review priority.",
    dependencies: "Dependency findings are marked by review priority.",
    frameworks: "Framework findings are marked by review priority.",
    styling: "Styling findings are marked by review priority.",
    flags: "Feature flag findings are marked by review priority.",
  };
  return lines[analysisId];
};

const linesTitle = (id: AnalysisId): string =>
  id === "flags" ? "Feature flag" : `${id.charAt(0).toUpperCase()}${id.slice(1)}`;

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
