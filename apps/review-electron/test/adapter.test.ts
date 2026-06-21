import { describe, it, expect } from "vitest";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { toWalkthroughDocument, type AuditBrief } from "../src/model/adapter";

const loadFixture = (): AuditBrief =>
  JSON.parse(
    readFileSync(fileURLToPath(new URL("../fixtures/sample-review.json", import.meta.url)), "utf8"),
  ) as AuditBrief;

describe("toWalkthroughDocument", () => {
  it("normalizes the real audit-brief fixture", () => {
    const doc = toWalkthroughDocument(loadFixture());
    expect(doc.focus.verdict).toBe("fail");
    expect(doc.focus.changedFiles).toBe(77);
    expect(doc.focus.riskClass).toBe("high");
    expect(doc.stages.length).toBeGreaterThan(0);
    expect(doc.stages[0]?.order).toBe(0);
    expect(doc.stages.flatMap((s) => s.files).length).toBeGreaterThan(0);
  });

  it("orders stages by partition.order", () => {
    const doc = toWalkthroughDocument(loadFixture());
    const orders = doc.stages.map((s) => s.order);
    expect(orders).toEqual(orders.toSorted((a, b) => a - b));
  });

  it("drops decisions without a signal_id (anti-hallucination)", () => {
    const brief: AuditBrief = {
      decisions: {
        decisions: [
          { signal_id: "sig-1", question: "real?" },
          { question: "no anchor" },
          { signal_id: "", question: "empty anchor" },
        ],
      },
    };
    const doc = toWalkthroughDocument(brief);
    expect(doc.decisions).toHaveLength(1);
    expect(doc.decisions[0]?.signalId).toBe("sig-1");
  });

  it("builds the cleared panel from summary counts", () => {
    const doc = toWalkthroughDocument(loadFixture());
    expect(doc.cleared.find((c) => c.kind === "dead-code")?.count).toBe(23);
    expect(doc.cleared.find((c) => c.kind === "duplication")?.count).toBe(2);
  });
});
