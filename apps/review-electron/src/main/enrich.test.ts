import { describe, it, expect } from "vitest";
import { factsForFile } from "./enrich";
import type { WalkthroughDocument } from "../model/walkthrough";

const doc = (): WalkthroughDocument => ({
  schemaVersion: 1,
  focus: {
    verdict: "fail",
    changedFiles: 1,
    baseRef: "",
    baseDescription: "",
    riskClass: "high",
    reviewEffort: "deep_dive",
    headline: "",
  },
  stages: [
    {
      moduleDir: "src/components",
      order: 0,
      files: [
        {
          path: "src/components/Button.tsx",
          attention: 5,
          label: "review-here",
          reason: "high fan-in (3 importers)",
          deprioritized: false,
          score: { fanIo: 5, securityTaint: 0, riskZone: 0, changeShape: 0, total: 5 },
        },
      ],
    },
  ],
  decisions: [],
  cleared: [],
  coordinationGaps: [],
  weakening: [],
  graphSnapshotHash: null,
});

describe("factsForFile", () => {
  it("returns grounded facts for a known file", () => {
    const facts = factsForFile(doc(), "src/components/Button.tsx");
    expect(facts).toContain("high fan-in (3 importers)");
    expect(facts.some((f) => f.startsWith("stage 1"))).toBe(true);
    expect(facts.some((f) => f.startsWith("attention 5"))).toBe(true);
  });

  it("falls back when the file is not in the review", () => {
    expect(factsForFile(doc(), "src/unknown.ts")[0]).toMatch(/no Fallow signal/);
  });
});
