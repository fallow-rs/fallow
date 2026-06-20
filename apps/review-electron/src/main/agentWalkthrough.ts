import type { AgentWalkthrough, FeedItem, Judgment } from "../model/agent";

/**
 * Build the agent-walkthrough payload from feed items. Only signal_id-anchored
 * items become judgments: file_line / component targets have no graph anchor for
 * `fallow review --walkthrough-file` to validate against. The guide's
 * `graph_snapshot_hash` is echoed so a moved tree is rejected as stale.
 */
export const buildAgentWalkthrough = (
  graphSnapshotHash: string,
  items: ReadonlyArray<FeedItem>,
): AgentWalkthrough => ({
  graph_snapshot_hash: graphSnapshotHash,
  judgments: items
    .filter((i) => i.target.kind === "signal_id" && i.target.value.length > 0)
    .map((i): Judgment => {
      const judgment: Judgment = { signal_id: i.target.value, framing: i.note };
      if (i.verdict) judgment.concern = i.verdict;
      return judgment;
    }),
});
