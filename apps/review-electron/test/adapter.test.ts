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

  it("orders stages and files by attention, impact-first", () => {
    const brief: AuditBrief = {
      partition: {
        // Engine module sequence lists the low-impact module first.
        order: ["low", "high"],
        units: [
          { module_dir: "low", files: ["low/a.ts", "low/b.ts"] },
          { module_dir: "high", files: ["high/x.ts", "high/y.ts"] },
        ],
      },
      focus: {
        review_here: [
          { file: "low/a.ts", score: { total: 1 } },
          { file: "low/b.ts", score: { total: 2 } },
          { file: "high/x.ts", score: { total: 9 } },
          { file: "high/y.ts", score: { total: 3 } },
        ],
      },
    };
    const doc = toWalkthroughDocument(brief);
    // The high-impact module leads despite being second in partition.order.
    expect(doc.stages.map((s) => s.moduleDir)).toEqual(["high", "low"]);
    // Files within a stage are sorted by attention, descending.
    expect(doc.stages[0]?.files.map((f) => f.path)).toEqual(["high/x.ts", "high/y.ts"]);
    expect(doc.stages[1]?.files.map((f) => f.path)).toEqual(["low/b.ts", "low/a.ts"]);
    // The display `order` stays a clean 0..n sequence.
    expect(doc.stages.map((s) => s.order)).toEqual([0, 1]);
  });

  it("ranks by displayed fan-in (importers), not the capped attention total", () => {
    const brief: AuditBrief = {
      partition: { order: ["m"], units: [{ module_dir: "m", files: ["m/hub.ts", "m/scored.ts"] }] },
      focus: {
        review_here: [
          { file: "m/hub.ts", reason: "high fan-in (17 importers)", score: { total: 10 } },
          {
            file: "m/scored.ts",
            reason: "high fan-in (5 importers), fan-out 7",
            score: { total: 12 },
          },
        ],
      },
    };
    const doc = toWalkthroughDocument(brief);
    // 17 importers outranks the file with a higher (but capped) attention total,
    // so the visible ↓N column stays monotonic.
    expect(doc.stages[0]?.files.map((f) => f.path)).toEqual(["m/hub.ts", "m/scored.ts"]);
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

  it("carries the enriched decision fields (honest count + trade-off), not just question", () => {
    const brief: AuditBrief = {
      decisions: {
        decisions: [
          {
            signal_id: "sig:abc",
            category: "public-api-contract",
            question: "Intended surface, or should it stay internal?",
            tradeoff: "Adds 1 maintained contract; 4 in-repo modules already consume this surface.",
            internal_consumer_count: 4,
            anchor_file: "src/ui/index.ts",
            anchor_line: 12,
            expert: ["alice"],
            bus_factor_one: true,
            blast: 99,
          },
        ],
      },
    };
    const [d] = toWalkthroughDocument(brief).decisions;
    expect(d?.category).toBe("public-api-contract");
    expect(d?.tradeoff).toContain("4 in-repo");
    // The honest display number, not the project-wide ranking proxy (`blast`).
    expect(d?.internalConsumerCount).toBe(4);
    expect(d?.anchorFile).toBe("src/ui/index.ts");
    expect(d?.anchorLine).toBe(12);
    expect(d?.expert).toEqual(["alice"]);
    expect(d?.busFactorOne).toBe(true);
  });

  it("builds the cleared panel from summary counts", () => {
    const doc = toWalkthroughDocument(loadFixture());
    expect(doc.cleared.find((c) => c.kind === "dead-code")?.count).toBe(23);
    expect(doc.cleared.find((c) => c.kind === "duplication")?.count).toBe(2);
  });
});
