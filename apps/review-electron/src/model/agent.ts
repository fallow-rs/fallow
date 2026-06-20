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
