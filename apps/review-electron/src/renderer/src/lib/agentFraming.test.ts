import { describe, it, expect } from "vitest";
import type { ValidationEnvelope } from "../../../model/agent";
import { acceptedReconstructedFraming, groupBySignalId, toInlineFraming } from "./agentFraming";

describe("toInlineFraming", () => {
  it("tags origin from the source and pins deterministic false", () => {
    const out = toInlineFraming(
      { signal_id: "sig:1", agent_framing: "is this coupling intended?", deterministic: true },
      "captured",
    );
    expect(out).toEqual({
      signalId: "sig:1",
      origin: "captured",
      framing: "is this coupling intended?",
      deterministic: false,
    });
  });

  it("carries concern through only when present", () => {
    const withConcern = toInlineFraming(
      { signal_id: "sig:2", agent_framing: "f", concern: "leaks infra", deterministic: false },
      "reconstructed",
    );
    expect(withConcern.concern).toBe("leaks infra");
    const without = toInlineFraming(
      { signal_id: "sig:3", agent_framing: "f", deterministic: false },
      "reconstructed",
    );
    expect("concern" in without).toBe(false);
  });
});

describe("acceptedReconstructedFraming", () => {
  it("returns [] for a null/empty envelope", () => {
    expect(acceptedReconstructedFraming(null)).toEqual([]);
    expect(acceptedReconstructedFraming({})).toEqual([]);
  });

  it("keeps only accepted judgments, tagged reconstructed, dropping rejected", () => {
    const envelope: ValidationEnvelope = {
      accepted: [
        { signal_id: "sig:a", agent_framing: "fa", deterministic: false },
        { signal_id: "sig:b", agent_framing: "fb", concern: "c", deterministic: false },
      ],
      rejected: [{ signal_id: "sig:x", reason: "unanchored" }],
    };
    const out = acceptedReconstructedFraming(envelope);
    expect(out).toHaveLength(2);
    expect(out.every((f) => f.origin === "reconstructed")).toBe(true);
    expect(out.map((f) => f.signalId)).toEqual(["sig:a", "sig:b"]);
    expect(out[1]?.concern).toBe("c");
  });
});

describe("groupBySignalId", () => {
  it("groups multiple framings under one signal preserving order", () => {
    const map = groupBySignalId([
      { signalId: "sig:a", origin: "captured", framing: "first", deterministic: false },
      { signalId: "sig:b", origin: "reconstructed", framing: "other", deterministic: false },
      { signalId: "sig:a", origin: "reconstructed", framing: "second", deterministic: false },
    ]);
    expect(map.get("sig:a")?.map((f) => f.framing)).toEqual(["first", "second"]);
    expect(map.get("sig:b")?.map((f) => f.origin)).toEqual(["reconstructed"]);
    expect(map.get("sig:missing")).toBeUndefined();
  });

  it("returns an empty map for no items", () => {
    expect(groupBySignalId([]).size).toBe(0);
  });
});
