/**
 * W1 render model: a surface-agnostic, structured walkthrough document derived
 * from `fallow review --format json` (kind: audit-brief). Renderable by any
 * surface (Electron renderer, CLI, web). Every element traces to a Fallow signal.
 */

export type AttentionScore = {
  fanIo: number;
  securityTaint: number;
  riskZone: number;
  changeShape: number;
  total: number;
};

export type WalkthroughFile = {
  path: string;
  attention: number;
  label: string;
  reason: string;
  deprioritized: boolean;
  score: AttentionScore;
};

export type WalkthroughStage = {
  moduleDir: string;
  order: number;
  files: WalkthroughFile[];
};

/** A consequential structural decision, anchored to a Fallow `signal_id`. */
export type Decision = {
  signalId: string;
  question: string;
  raw: Record<string, unknown>;
};

/** One line in the "Fallow already did the technical pass" cleared panel. */
export type ClearedItem = {
  kind: string;
  label: string;
  count: number;
};

export type ReviewFocus = {
  verdict: string;
  changedFiles: number;
  baseRef: string;
  baseDescription: string;
  riskClass: string;
  reviewEffort: string;
  headline: string;
};

export type WalkthroughDocument = {
  schemaVersion: number;
  focus: ReviewFocus;
  stages: WalkthroughStage[];
  decisions: Decision[];
  cleared: ClearedItem[];
  coordinationGaps: ReadonlyArray<Record<string, unknown>>;
  weakening: ReadonlyArray<Record<string, unknown>>;
  graphSnapshotHash: string | null;
};

const IMPORTERS = /(\d+)\s+importers?/;
const FAN_OUT = /fan-out\s+(\d+)/;

/**
 * Parse the literal fan-in (importer count) and fan-out from a focus `reason`
 * such as "high fan-in (17 importers), fan-out 2". Fan-in is the blast-radius
 * signal the UI both displays and sorts by, so the two stay in lockstep.
 */
export const parseFanInOut = (reason: string): { fanIn: number; fanOut: number } => ({
  fanIn: Number(IMPORTERS.exec(reason)?.[1] ?? 0),
  fanOut: Number(FAN_OUT.exec(reason)?.[1] ?? 0),
});
