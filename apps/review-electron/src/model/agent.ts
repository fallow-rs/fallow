/** Shared types for the W3 agent-feedback channel (pure: no runtime deps). */

export type FeedTarget =
  | { kind: "signal_id"; value: string }
  | { kind: "file_line"; value: string }
  | { kind: "component"; value: string }
  // A model-inferred trade-off, identified by its anchor (`file:line`) or the
  // literal `cross-cutting` (which is NOT a `file_line`, so it gets its own kind).
  | { kind: "tradeoff"; value: string };

/** One human annotation/selection routed back toward the coding agent. */
export type FeedItem = {
  target: FeedTarget;
  note: string;
  imageRef?: string;
  verdict?: string;
  at: string;
};

/** One per-hunk change anchor fallow emits in the walkthrough guide: a stable,
 * content-addressed id for a changed region. The app maps a trade-off's
 * `file:line` to one of these to cite it for fallow-side validation. */
export type ChangeAnchor = {
  changeAnchor: string;
  file: string;
  startLine: number;
  lineCount: number;
  previousChangeAnchor?: string;
};

/** Result of `fallow review --walkthrough-guide`: the E5 agent-contract digest. */
export type Guide = {
  graphSnapshotHash: string;
  emittedSignalIds: string[];
  changeAnchors: ChangeAnchor[];
  order: string[];
  digest: unknown;
  schemaShape: string;
};

/** One judgment in the agent-walkthrough payload fallow post-validates. An anchor
 * is a `signal_id` (a graph finding) OR a `change_anchor` (a changed region). */
export type Judgment = {
  signal_id: string;
  change_anchor?: string;
  framing: string;
  concern?: string;
};

/** The payload `fallow review --walkthrough-file` ingests and graph-validates. */
export type AgentWalkthrough = {
  graph_snapshot_hash: string;
  judgments: Judgment[];
};

/** One accepted judgment in the fallow validation envelope (graph-anchored).
 * `anchor_kind` is `"signal"` (graph finding) or `"change"` (changed region);
 * `change_anchor` carries the cited `chg:` id when `anchor_kind === "change"`. */
export type AcceptedJudgment = {
  signal_id: string;
  change_anchor?: string;
  anchor_kind?: string;
  agent_framing: string;
  concern?: string;
  deterministic: boolean;
};

/** The fixed `fallow review --walkthrough-file` validation envelope shape. */
export type ValidationEnvelope = {
  stale?: boolean;
  accepted?: AcceptedJudgment[];
  rejected?: { signal_id: string; change_anchor?: string; reason: string }[];
  accepted_count?: number;
  rejected_count?: number;
};

/**
 * Where a piece of inline framing came from. `captured` = author-agent framing
 * recorded at write-time (fact-ish about authorial intent); `reconstructed` =
 * review-time inference produced by the opt-in agent run (must be confirmed with
 * the author). The label is load-bearing: a confident-wrong reconstruction is the
 * worst failure mode, so the two origins are never interchangeable.
 */
export type FramingOrigin = "captured" | "reconstructed";

/**
 * One inline framing block rendered next to its own decision, keyed by
 * `signalId`. Carries its {@link FramingOrigin} and `deterministic:false` so the
 * UI can fence it as non-graph-fact regardless of origin. Mirrors the fallow
 * envelope's accepted-judgment shape plus the origin tag.
 */
export type InlineFraming = {
  signalId: string;
  origin: FramingOrigin;
  framing: string;
  concern?: string;
  /** Always false: framing is never a deterministic graph fact. */
  deterministic: boolean;
};
