import { describe, it, expect } from "vitest";
import { buildAgentWalkthrough } from "../src/main/agentWalkthrough";
import type { FeedItem } from "../src/model/agent";

const item = (over: Partial<FeedItem>): FeedItem => ({
  target: { kind: "signal_id", value: "sig-1" },
  note: "look here",
  at: "t0",
  ...over,
});

describe("buildAgentWalkthrough", () => {
  it("echoes the hash and maps signal_id items to judgments", () => {
    const wt = buildAgentWalkthrough("graph:abc", [item({})]);
    expect(wt.graph_snapshot_hash).toBe("graph:abc");
    expect(wt.judgments).toHaveLength(1);
    expect(wt.judgments[0]).toMatchObject({ signal_id: "sig-1", framing: "look here" });
  });

  it("drops non-signal_id targets (no graph anchor to validate)", () => {
    const wt = buildAgentWalkthrough("h", [
      item({ target: { kind: "file_line", value: "a.ts:3" } }),
      item({ target: { kind: "component", value: "Button" } }),
      item({ target: { kind: "signal_id", value: "" } }),
    ]);
    expect(wt.judgments).toHaveLength(0);
  });

  it("maps verdict to concern", () => {
    const wt = buildAgentWalkthrough("h", [item({ verdict: "risky" })]);
    expect(wt.judgments[0]?.concern).toBe("risky");
  });
});
