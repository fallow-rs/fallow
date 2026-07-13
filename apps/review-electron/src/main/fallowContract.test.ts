import { describe, expect, it } from "vitest";
import { toWalkthroughDocument } from "../model/adapter";
import {
  REVIEW_BRIEF_SCHEMA_VERSION,
  parseGuideContract,
  parseReviewContract,
  parseWalkthroughValidationContract,
} from "./fallowContract";

const compatibilityError = (kind: string): string => {
  const command =
    kind === "audit-brief"
      ? "fallow review"
      : kind === "review-walkthrough-guide"
        ? "fallow review --walkthrough-guide"
        : "fallow review --walkthrough-file";
  return `${command} returned incompatible JSON; expected ${kind} schema version ${REVIEW_BRIEF_SCHEMA_VERSION}.`;
};

const minimumReview = (): Record<string, unknown> => ({
  kind: "audit-brief",
  command: "audit-brief",
  schema_version: REVIEW_BRIEF_SCHEMA_VERSION,
});

const minimumGuide = (): Record<string, unknown> => ({
  kind: "review-walkthrough-guide",
  command: "review-walkthrough-guide",
  schema_version: REVIEW_BRIEF_SCHEMA_VERSION,
  graph_snapshot_hash: "graph:1",
  digest: { decisions: { emitted_signal_ids: [] } },
  direction: { order: [] },
  change_anchors: [],
  agent_schema: { judgment_shape: "shape" },
});

const minimumValidation = (): Record<string, unknown> => ({
  kind: "review-walkthrough-validation",
  command: "review-walkthrough-validation",
  schema_version: REVIEW_BRIEF_SCHEMA_VERSION,
  graph_snapshot_hash: "graph:1",
  stale: false,
  accepted: [],
  rejected: [],
});

it("tracks the current Review Brief schema version", () => {
  expect(REVIEW_BRIEF_SCHEMA_VERSION).toBe(6);
});

describe("parseReviewContract", () => {
  it("preserves the existing unreadable-output error for non-JSON", () => {
    expect(() => parseReviewContract("{")).toThrow(
      "fallow returned output that couldn't be read as JSON.",
    );
  });

  it.each([null, true, 7, "text", []])("rejects syntactically valid primitive %j", (value) => {
    expect(() => parseReviewContract(JSON.stringify(value))).toThrow(
      compatibilityError("audit-brief"),
    );
  });

  it("rejects a wrong kind with one stable compatibility error", () => {
    expect(() =>
      parseReviewContract(JSON.stringify({ ...minimumReview(), kind: "health" })),
    ).toThrow(compatibilityError("audit-brief"));
  });

  it.each([
    ["missing", undefined],
    ["older", REVIEW_BRIEF_SCHEMA_VERSION - 1],
    ["newer", REVIEW_BRIEF_SCHEMA_VERSION + 1],
  ])("rejects a %s schema version", (_label, schemaVersion) => {
    const value = minimumReview();
    if (schemaVersion === undefined) delete value["schema_version"];
    else value["schema_version"] = schemaVersion;

    expect(() => parseReviewContract(JSON.stringify(value))).toThrow(
      compatibilityError("audit-brief"),
    );
  });

  it.each([
    { focus: { review_here: [null] } },
    { focus: { review_here: [{ file: 42 }] } },
    { focus: { deprioritized: [{ file: "src/a.ts", score: { total: "high" } }] } },
    { partition: { units: [{ module_dir: 42 }] } },
    { partition: { units: [{ module_dir: "src", files: [42] }] } },
    { partition: { order: "src" } },
    { summary: [] },
    { decisions: { decisions: [null] } },
    { impact_closure: { coordination_gap: [null] } },
    { weakening: [null] },
  ])("rejects a nested review shape the adapter would traverse", (nested) => {
    expect(() => parseReviewContract(JSON.stringify({ ...minimumReview(), ...nested }))).toThrow(
      compatibilityError("audit-brief"),
    );
  });

  it("accepts a minimum review envelope and additive unknown fields", () => {
    const parsed = parseReviewContract(
      JSON.stringify({ ...minimumReview(), future: { additive: true } }),
    );

    expect(toWalkthroughDocument(parsed)).toMatchObject({
      schemaVersion: REVIEW_BRIEF_SCHEMA_VERSION,
      stages: [],
      decisions: [],
    });
  });

  it("never includes response content in compatibility errors", () => {
    const secret = "private-response-marker";
    try {
      parseReviewContract(JSON.stringify({ ...minimumReview(), focus: secret }));
      throw new Error("expected contract rejection");
    } catch (error: unknown) {
      expect(error).toBeInstanceOf(Error);
      expect((error as Error).message).toBe(compatibilityError("audit-brief"));
      expect((error as Error).message).not.toContain(secret);
    }
  });
});

describe("parseGuideContract", () => {
  it.each([
    { digest: [] },
    { digest: { decisions: { emitted_signal_ids: [7] } } },
    { direction: { order: [7] } },
    { change_anchors: [null] },
    {
      change_anchors: [
        { change_anchor: "chg:1", file: "src/a.ts", start_line: "1", line_count: 1 },
      ],
    },
    { agent_schema: { judgment_shape: 7 } },
  ])("rejects a nested guide shape the mapper would traverse", (nested) => {
    expect(() => parseGuideContract(JSON.stringify({ ...minimumGuide(), ...nested }))).toThrow(
      compatibilityError("review-walkthrough-guide"),
    );
  });

  it("accepts a minimum guide envelope and additive unknown fields", () => {
    const parsed = parseGuideContract(
      JSON.stringify({ ...minimumGuide(), future: { additive: true } }),
    );

    expect(parsed.graph_snapshot_hash).toBe("graph:1");
    expect(parsed.change_anchors).toEqual([]);
  });
});

describe("parseWalkthroughValidationContract", () => {
  it.each([
    { stale: "false" },
    { accepted: [null] },
    {
      accepted: [
        {
          signal_id: "sig:1",
          change_anchor: "",
          anchor_kind: "signal",
          agent_framing: 7,
          deterministic: false,
        },
      ],
    },
    { rejected: [{ signal_id: "sig:1", change_anchor: "", reason: 7 }] },
  ])("rejects a nested validation shape consumers would traverse", (nested) => {
    expect(() =>
      parseWalkthroughValidationContract(JSON.stringify({ ...minimumValidation(), ...nested })),
    ).toThrow(compatibilityError("review-walkthrough-validation"));
  });

  it("accepts a minimum validation envelope and additive unknown fields", () => {
    const parsed = parseWalkthroughValidationContract(
      JSON.stringify({ ...minimumValidation(), future: { additive: true } }),
    );

    expect(parsed).toMatchObject({ stale: false, accepted: [], rejected: [] });
  });
});
