import {
  parseFanInOut,
  type AttentionScore,
  type ClearedItem,
  type Decision,
  type ReviewFocus,
  type WalkthroughDocument,
  type WalkthroughFile,
  type WalkthroughStage,
} from "./walkthrough";
import type { Severity, TradeOff, TradeOffEnvelope } from "./tradeoff";

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

const asString = (v: unknown): string => (typeof v === "string" ? v : "");
const asNumber = (v: unknown): number => (typeof v === "number" ? v : 0);

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
 * Review-priority rank for one file, highest-impact first: security taint, then
 * risk zone, then fan-in (the importer count the row displays), then the engine's
 * attention score as a finer tiebreak. Sorting on the displayed fan-in rather
 * than the capped attention score keeps the visible ↓N column monotonic.
 */
type Rank = [number, number, number, number];
const rankOf = (f: WalkthroughFile): Rank => [
  f.score.securityTaint,
  f.score.riskZone,
  parseFanInOut(f.reason).fanIn,
  f.attention,
];
const compareRankDesc = (a: Rank, b: Rank): number => {
  for (let i = 0; i < a.length; i += 1) {
    if (b[i] !== a[i]) return (b[i] ?? 0) - (a[i] ?? 0);
  }
  return 0;
};
const byRankDesc = (a: WalkthroughFile, b: WalkthroughFile): number =>
  compareRankDesc(rankOf(a), rankOf(b));
const maxRank = (files: WalkthroughFile[]): Rank =>
  files.reduce<Rank>(
    (m, f) => m.map((v, i) => Math.max(v, rankOf(f)[i] ?? 0)) as Rank,
    [0, 0, 0, 0],
  );

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
  // Highest-blast-radius work first: stages and the files within them are ordered
  // by {@link byRankDesc} (security, risk, displayed fan-in, attention). Ties fall
  // back to the engine's original walkthrough sequence for stability.
  const stages: WalkthroughStage[] = (brief.partition?.units ?? [])
    .map((unit, originalIdx) => {
      const files = (unit.files ?? [])
        .map((path, fileIdx) => ({ file: fileFor(path), fileIdx }))
        .toSorted((a, b) => byRankDesc(a.file, b.file) || a.fileIdx - b.fileIdx)
        .map(({ file }) => file);
      return { moduleDir: unit.module_dir, files, peak: maxRank(files), originalIdx };
    })
    .toSorted((a, b) => {
      const byPeak = compareRankDesc(a.peak, b.peak);
      if (byPeak !== 0) return byPeak;
      return orderIndex(a.moduleDir) - orderIndex(b.moduleDir) || a.originalIdx - b.originalIdx;
    })
    .map(({ moduleDir, files }, i): WalkthroughStage => ({ moduleDir, order: i, files }));

  const decisions: Decision[] = (brief.decisions?.decisions ?? [])
    .filter((d) => typeof d["signal_id"] === "string" && (d["signal_id"] as string).length > 0)
    .map((d) => ({
      signalId: d["signal_id"] as string,
      category: asString(d["category"]),
      question: asString(d["question"]),
      tradeoff: asString(d["tradeoff"]),
      internalConsumerCount: asNumber(d["internal_consumer_count"]),
      anchorFile: asString(d["anchor_file"]),
      anchorLine: asNumber(d["anchor_line"]),
      expert: Array.isArray(d["expert"])
        ? (d["expert"] as unknown[]).filter((e): e is string => typeof e === "string")
        : [],
      busFactorOne: d["bus_factor_one"] === true,
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

/**
 * Coerce a model-reported severity to a {@link Severity}. Lowercase-normalize
 * first, then map `low|medium|high`. DEFAULTS to `"low"` on anything else (a
 * `"High"`/`"critical"`/garbage value must NOT drop the whole item); severity is
 * never the anti-hallucination key, the anchor is.
 */
const asSeverity = (v: unknown): Severity => {
  const s = String(v).toLowerCase();
  if (s === "medium") return "medium";
  if (s === "high") return "high";
  return "low";
};

/**
 * Normalize the raw trade-off envelope into a {@link TradeOffEnvelope}. TOTAL:
 * never returns null. A non-object `raw` is a PARSE FAILURE, not an abstain, so it
 * yields `{ graphSnapshotHash: "", abstained: false, tradeoffs: [] }` (never a fake
 * `abstained: true`). The only case-conversion is `graph_snapshot_hash` ->
 * `graphSnapshotHash`; every other key is the model's single-word wire key.
 *
 * Anti-hallucination: an item is DROPPED only when its `anchor` is empty (the
 * mirror of the decisions `signal_id` filter), never for a bad severity.
 * `deterministic` is pinned to `false` regardless of the model's self-report.
 * `tradeoffs` are sorted by `anchor` then `lens` for structural diffability, and
 * forced empty when `abstained === true`.
 */
export const toTradeOffEnvelope = (raw: unknown): TradeOffEnvelope => {
  if (typeof raw !== "object" || raw === null) {
    return { graphSnapshotHash: "", abstained: false, tradeoffs: [] };
  }
  const x = raw as Record<string, unknown>;
  const abstained = x["abstained"] === true;
  const items = Array.isArray(x["tradeoffs"]) ? (x["tradeoffs"] as unknown[]) : [];
  const tradeoffs: TradeOff[] = abstained
    ? []
    : items
        .filter((d): d is Record<string, unknown> => typeof d === "object" && d !== null)
        .map(
          (d): TradeOff => ({
            id: asString(d["id"]),
            anchor: asString(d["anchor"]),
            lens: asString(d["lens"]),
            observed: asString(d["observed"]),
            tradeoff: asString(d["tradeoff"]),
            question: asString(d["question"]),
            consequence: asSeverity(d["consequence"]),
            confidence: asSeverity(d["confidence"]),
            captured: d["captured"] === true,
            deterministic: false,
          }),
        )
        .filter((t) => t.anchor.length > 0)
        .toSorted((a, b) => a.anchor.localeCompare(b.anchor) || a.lens.localeCompare(b.lens));
  return { graphSnapshotHash: asString(x["graph_snapshot_hash"]), abstained, tradeoffs };
};
