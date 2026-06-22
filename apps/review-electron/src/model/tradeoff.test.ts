import { describe, it, expect } from "vitest";
import { toTradeOffEnvelope } from "./adapter";

/**
 * Behaviour of the trade-off adapter, the consumer-side normalizer for the
 * MODEL-INFERRED surface. The load-bearing invariants are the honesty ones: a
 * parse failure must never masquerade as an abstain, a bad severity must never
 * drop the whole item, an empty anchor (the anti-hallucination key) must, and
 * `deterministic` is pinned to `false` regardless of what the model self-reports.
 */
const item = (over: Record<string, unknown> = {}): Record<string, unknown> => ({
  id: "to:src/x.ts:42:error-handling",
  anchor: "src/x.ts:42",
  lens: "error-handling",
  observed: "returns the raw error to the caller",
  tradeoff: "thin call site, but couples callers to storage error shapes",
  question: "how should this surface a storage failure?",
  consequence: "high",
  confidence: "medium",
  captured: false,
  deterministic: false,
  ...over,
});

describe("toTradeOffEnvelope", () => {
  it("maps the single-word wire keys and camelCases only graph_snapshot_hash", () => {
    const env = toTradeOffEnvelope({
      graph_snapshot_hash: "graph:abc",
      abstained: false,
      tradeoffs: [item()],
    });
    expect(env.graphSnapshotHash).toBe("graph:abc");
    expect(env.abstained).toBe(false);
    expect(env.tradeoffs[0]).toMatchObject({
      id: "to:src/x.ts:42:error-handling",
      anchor: "src/x.ts:42",
      lens: "error-handling",
      observed: "returns the raw error to the caller",
      question: "how should this surface a storage failure?",
      consequence: "high",
      confidence: "medium",
      captured: false,
    });
  });

  it("returns a NON-abstained empty envelope on a parse failure (never a fake abstain)", () => {
    for (const bad of [null, undefined, "not json", 42, []]) {
      const env = toTradeOffEnvelope(bad);
      expect(env).toEqual({ graphSnapshotHash: "", abstained: false, tradeoffs: [] });
    }
  });

  it("forces tradeoffs:[] when abstained is true", () => {
    const env = toTradeOffEnvelope({
      graph_snapshot_hash: "graph:abc",
      abstained: true,
      tradeoffs: [item()],
    });
    expect(env.abstained).toBe(true);
    expect(env.tradeoffs).toEqual([]);
  });

  it("normalizes severity case and DEFAULTS bad values to low (never drops the item)", () => {
    const env = toTradeOffEnvelope({
      graph_snapshot_hash: "g",
      abstained: false,
      tradeoffs: [item({ consequence: "High", confidence: "critical" })],
    });
    expect(env.tradeoffs).toHaveLength(1);
    expect(env.tradeoffs[0]?.consequence).toBe("high");
    expect(env.tradeoffs[0]?.confidence).toBe("low");
  });

  it("drops an item ONLY when its anchor is empty (the anti-hallucination key)", () => {
    const env = toTradeOffEnvelope({
      graph_snapshot_hash: "g",
      abstained: false,
      tradeoffs: [item(), item({ id: "to::error", anchor: "" })],
    });
    expect(env.tradeoffs).toHaveLength(1);
    expect(env.tradeoffs[0]?.anchor).toBe("src/x.ts:42");
  });

  it("keeps the literal cross-cutting anchor (not an empty anchor)", () => {
    const env = toTradeOffEnvelope({
      graph_snapshot_hash: "g",
      abstained: false,
      tradeoffs: [item({ anchor: "cross-cutting" })],
    });
    expect(env.tradeoffs).toHaveLength(1);
    expect(env.tradeoffs[0]?.anchor).toBe("cross-cutting");
  });

  it("pins deterministic:false even when the model self-reports true", () => {
    const env = toTradeOffEnvelope({
      graph_snapshot_hash: "g",
      abstained: false,
      tradeoffs: [item({ deterministic: true })],
    });
    expect(env.tradeoffs[0]?.deterministic).toBe(false);
  });

  it("sorts by anchor then lens for structural diffability", () => {
    const env = toTradeOffEnvelope({
      graph_snapshot_hash: "g",
      abstained: false,
      tradeoffs: [
        item({ anchor: "src/b.ts:1", lens: "naming" }),
        item({ anchor: "src/a.ts:9", lens: "testability" }),
        item({ anchor: "src/a.ts:9", lens: "data-model" }),
      ],
    });
    expect(env.tradeoffs.map((t) => `${t.anchor}|${t.lens}`)).toEqual([
      "src/a.ts:9|data-model",
      "src/a.ts:9|testability",
      "src/b.ts:1|naming",
    ]);
  });
});
