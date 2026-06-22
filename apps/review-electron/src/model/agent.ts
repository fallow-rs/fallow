/** Shared types for the W3 agent-feedback channel (pure: no runtime deps). */

export type FeedTarget =
  | { kind: "signal_id"; value: string }
  | { kind: "file_line"; value: string }
  | { kind: "component"; value: string };

/** One human annotation/selection routed back toward the coding agent. */
export type FeedItem = {
  target: FeedTarget;
  note: string;
  imageRef?: string;
  verdict?: string;
  at: string;
};

/** Result of `fallow review --walkthrough-guide`: the E5 agent-contract digest. */
export type Guide = {
  graphSnapshotHash: string;
  emittedSignalIds: string[];
  order: string[];
  digest: unknown;
  schemaShape: string;
};

/** One judgment in the agent-walkthrough payload fallow post-validates. */
export type Judgment = {
  signal_id: string;
  framing: string;
  concern?: string;
};

/** The payload `fallow review --walkthrough-file` ingests and graph-validates. */
export type AgentWalkthrough = {
  graph_snapshot_hash: string;
  judgments: Judgment[];
};

/** One accepted judgment in the fallow validation envelope (graph-anchored). */
export type AcceptedJudgment = {
  signal_id: string;
  agent_framing: string;
  concern?: string;
  deterministic: boolean;
};

/** The fixed `fallow review --walkthrough-file` validation envelope shape. */
export type ValidationEnvelope = {
  stale?: boolean;
  accepted?: AcceptedJudgment[];
  rejected?: { signal_id: string; reason: string }[];
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
