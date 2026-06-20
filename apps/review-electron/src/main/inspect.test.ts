import { describe, it, expect } from "vitest";
import { buildInspectorCard } from "./inspect";
import type { WalkthroughDocument } from "../model/walkthrough";

const doc: WalkthroughDocument = {
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
      moduleDir: "apps/review-electron/fixtures/sample-app/src/components",
      order: 0,
      files: [
        {
          path: "apps/review-electron/fixtures/sample-app/src/components/Button.tsx",
          attention: 5,
          label: "review-here",
          reason: "high fan-in (1 importer)",
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
};

describe("buildInspectorCard", () => {
  it("joins a selection to grounded facts", () => {
    const card = buildInspectorCard(doc, {
      file: "apps/review-electron/fixtures/sample-app/src/components/Button.tsx",
      line: 6,
      component: "Button",
    });
    expect(card.component).toBe("Button");
    expect(card.line).toBe(6);
    expect(card.facts).toContain("high fan-in (1 importer)");
  });

  it("degrades when no review is loaded", () => {
    const card = buildInspectorCard(null, { file: "x.tsx", line: 1 });
    expect(card.facts[0]).toMatch(/no review loaded/);
  });
});
