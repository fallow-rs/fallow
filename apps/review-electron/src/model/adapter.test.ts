import { describe, it, expect } from "vitest";
import { toWalkthroughDocument, type AuditBrief } from "./adapter";
import fixture from "../../fixtures/sample-review-with-decisions.json";

/**
 * Proof that the adapter -> render-model path DecisionList consumes works
 * end-to-end against a hand-authored decision-producing brief. Driven through the
 * pure adapter (no DOM mount), since the Phase 1 Rust fields are not yet synced
 * into this checkout, so the fixture hand-authors them and the adapter guards
 * them. The two committed real fixtures show empty decisions[], so this is the
 * first end-to-end exercise of the decision render path.
 */
const brief = fixture as AuditBrief;

describe("toWalkthroughDocument over a decision-producing brief", () => {
  const doc = toWalkthroughDocument(brief);

  it("keeps both signal_id-anchored decisions (anti-hallucination filter passes valid)", () => {
    expect(doc.decisions).toHaveLength(2);
    expect(doc.decisions.map((d) => d.signalId)).toEqual([
      "boundary:src/services/api/client.ts->src/infra/db.ts",
      "public-api:src/services/api/index.ts#createClient",
    ]);
  });

  it("drops any decision lacking a Fallow signal_id (anti-hallucination)", () => {
    const withUnanchored: AuditBrief = {
      ...brief,
      decisions: {
        emitted_signal_ids: brief.decisions?.emitted_signal_ids,
        decisions: [
          ...(brief.decisions?.decisions ?? []),
          { category: "coupling-boundary", question: "no signal id, should drop" },
        ],
      },
    };
    expect(toWalkthroughDocument(withUnanchored).decisions).toHaveLength(2);
  });

  it("camelCases every field DecisionList reads on the coupling-boundary decision", () => {
    const boundary = doc.decisions[0];
    expect(boundary).toMatchObject({
      signalId: "boundary:src/services/api/client.ts->src/infra/db.ts",
      category: "coupling-boundary",
      question: "is the api layer meant to reach straight into the infra db module here?",
      internalConsumerCount: 4,
      anchorFile: "src/services/api/client.ts",
      anchorLine: 42,
      expert: ["api-team", "infra-team"],
      busFactorOne: false,
    });
    expect(boundary?.tradeoff).toContain("couples the api layer to infra");
  });

  it("carries the public-api decision's distinct fields, including bus_factor_one", () => {
    const publicApi = doc.decisions[1];
    expect(publicApi).toMatchObject({
      signalId: "public-api:src/services/api/index.ts#createClient",
      category: "public-api-contract",
      internalConsumerCount: 0,
      anchorFile: "src/services/api/index.ts",
      anchorLine: 7,
      expert: ["api-team"],
      busFactorOne: true,
    });
    expect(publicApi?.question.endsWith("?")).toBe(true);
  });

  it("keeps internal_consumer_count distinct from the displayed blast (fan-in)", () => {
    // The public-api export has fan-in 1 (one importer in focus) yet 0 internal
    // consumers: the two numbers must not collapse, or the UI would let a reader
    // infer 'low number = safe to remove' for a brand-new public export.
    const publicApiFile = doc.stages
      .flatMap((s) => s.files)
      .find((f) => f.path === "src/services/api/index.ts");
    expect(publicApiFile?.reason).toContain("new public export");
    expect(doc.decisions[1]?.internalConsumerCount).toBe(0);
    // The coupling-boundary anchor: 4 internal consumers, fan-in 4 here, but the
    // adapter never derives one from the other, so they stay independent facts.
    const boundaryFile = doc.stages
      .flatMap((s) => s.files)
      .find((f) => f.path === "src/services/api/client.ts");
    expect(boundaryFile?.score.fanIo).toBe(6);
    expect(doc.decisions[0]?.internalConsumerCount).toBe(4);
  });

  it("threads the graph_snapshot_hash through for the signalId join target", () => {
    expect(doc.graphSnapshotHash).toBe(
      "sha256:decisionfixture000000000000000000000000000000000000000000000001",
    );
  });

  it("builds the focus and cleared facts the rest of the surface reads", () => {
    expect(doc.focus.verdict).toBe("review");
    expect(doc.focus.changedFiles).toBe(6);
    expect(doc.focus.riskClass).toBe("medium");
    expect(doc.cleared.map((c) => c.kind)).toEqual(["dead-code", "duplication"]);
  });
});
