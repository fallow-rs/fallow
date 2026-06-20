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
