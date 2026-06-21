import type {
  AttentionScore,
  ClearedItem,
  Decision,
  ReviewFocus,
  WalkthroughDocument,
  WalkthroughFile,
  WalkthroughStage,
} from "./walkthrough";

/** Minimal structural view of `fallow review --format json` (kind: audit-brief). */
type RawScore = {
  fan_io?: number;
  security_taint?: number;
  risk_zone?: number;
  change_shape?: number;
  total?: number;
};

type RawFocusEntry = {
  file: string;
  label?: string;
  reason?: string;
  score?: RawScore;
};

type RawUnit = {
  module_dir: string;
  files?: string[];
};

export type AuditBrief = {
  schema_version?: number;
  verdict?: string;
  changed_files_count?: number;
  base_ref?: string;
  base_description?: string;
  triage?: { files?: number; risk_class?: string; review_effort?: string };
  summary?: {
    dead_code_issues?: number;
    duplication_clone_groups?: number;
    complexity_findings?: number;
  };
  decisions?: { decisions?: Array<Record<string, unknown>>; emitted_signal_ids?: string[] };
  partition?: { units?: RawUnit[]; order?: string[] };
  focus?: { review_here?: RawFocusEntry[]; deprioritized?: RawFocusEntry[] };
  impact_closure?: {
    affected_not_shown?: unknown[];
    coordination_gap?: Array<Record<string, unknown>>;
  };
  weakening?: Array<Record<string, unknown>>;
  graph_snapshot_hash?: string;
};

const toScore = (s: RawScore | undefined): AttentionScore => ({
  fanIo: s?.fan_io ?? 0,
  securityTaint: s?.security_taint ?? 0,
  riskZone: s?.risk_zone ?? 0,
  changeShape: s?.change_shape ?? 0,
  total: s?.total ?? 0,
});

const buildCleared = (brief: AuditBrief): ClearedItem[] => {
  const out: ClearedItem[] = [];
  const dead = brief.summary?.dead_code_issues ?? 0;
  const dupes = brief.summary?.duplication_clone_groups ?? 0;
  const cx = brief.summary?.complexity_findings ?? 0;
  if (dead > 0) out.push({ kind: "dead-code", label: "dead-code findings", count: dead });
  if (dupes > 0) out.push({ kind: "duplication", label: "duplication clone groups", count: dupes });
  if (cx > 0) out.push({ kind: "complexity", label: "complexity findings", count: cx });
  return out;
};

const buildFocus = (brief: AuditBrief): ReviewFocus => {
  const changedFiles = brief.changed_files_count ?? brief.triage?.files ?? 0;
  const riskClass = brief.triage?.risk_class ?? "unknown";
  const verdict = brief.verdict ?? "unknown";
  return {
    verdict,
    changedFiles,
    baseRef: brief.base_ref ?? "",
    baseDescription: brief.base_description ?? "",
    riskClass,
    reviewEffort: brief.triage?.review_effort ?? "unknown",
    headline: `${changedFiles} changed files, ${riskClass} risk, verdict ${verdict}`,
  };
};

/**
 * Normalize a raw audit-brief into a {@link WalkthroughDocument}. Pure: takes
 * parsed JSON, returns the render model. Anti-hallucination: decisions without a
 * Fallow `signal_id` are dropped.
 */
export const toWalkthroughDocument = (brief: AuditBrief): WalkthroughDocument => {
  const factByFile = new Map<string, WalkthroughFile>();
  const addFact = (e: RawFocusEntry, deprioritized: boolean): void => {
    factByFile.set(e.file, {
      path: e.file,
      attention: e.score?.total ?? 0,
      label: e.label ?? (deprioritized ? "not-prioritized" : "review-here"),
      reason: e.reason ?? "",
      deprioritized,
      score: toScore(e.score),
    });
  };
  (brief.focus?.review_here ?? []).forEach((e) => addFact(e, false));
  (brief.focus?.deprioritized ?? []).forEach((e) => addFact(e, true));

  const fileFor = (path: string): WalkthroughFile =>
    factByFile.get(path) ?? {
      path,
      attention: 0,
      label: "unscored",
      reason: "",
      deprioritized: false,
      score: { fanIo: 0, securityTaint: 0, riskZone: 0, changeShape: 0, total: 0 },
    };

  const order = brief.partition?.order ?? [];
  const orderIndex = (dir: string): number => {
    const i = order.indexOf(dir);
    return i === -1 ? Number.MAX_SAFE_INTEGER : i;
  };
  const stages: WalkthroughStage[] = (brief.partition?.units ?? [])
    .map((unit) => ({ unit, idx: orderIndex(unit.module_dir) }))
    .toSorted((a, b) => a.idx - b.idx)
    .map(
      ({ unit }, i): WalkthroughStage => ({
        moduleDir: unit.module_dir,
        order: i,
        files: (unit.files ?? []).map(fileFor),
      }),
    );

  const decisions: Decision[] = (brief.decisions?.decisions ?? [])
    .filter((d) => typeof d["signal_id"] === "string" && (d["signal_id"] as string).length > 0)
    .map((d) => ({
      signalId: d["signal_id"] as string,
      question: typeof d["question"] === "string" ? (d["question"] as string) : "",
      raw: d,
    }));

  return {
    schemaVersion: brief.schema_version ?? 0,
    focus: buildFocus(brief),
    stages,
    decisions,
    cleared: buildCleared(brief),
    coordinationGaps: brief.impact_closure?.coordination_gap ?? [],
    weakening: brief.weakening ?? [],
    graphSnapshotHash: brief.graph_snapshot_hash ?? null,
  };
};
