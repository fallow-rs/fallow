import { describe, it, expect } from "vitest";
import { deriveFileBadges } from "./badges";
import type { WalkthroughFile } from "../../../model/walkthrough";

const file = (over: Partial<WalkthroughFile>): WalkthroughFile => ({
  path: "x.ts",
  attention: 0,
  label: "review-here",
  reason: "",
  deprioritized: false,
  score: { fanIo: 0, securityTaint: 0, riskZone: 0, changeShape: 0, total: 0 },
  ...over,
});

describe("deriveFileBadges", () => {
  it("flags review-here vs deprioritized", () => {
    expect(deriveFileBadges(file({})).some((b) => b.label === "REVIEW HERE")).toBe(true);
    expect(
      deriveFileBadges(file({ deprioritized: true })).some((b) => b.label === "DEPRIORITIZED"),
    ).toBe(true);
  });

  it("flags high fan-in and security from the score", () => {
    const badges = deriveFileBadges(
      file({ score: { fanIo: 9, securityTaint: 1, riskZone: 0, changeShape: 0, total: 10 } }),
    );
    expect(badges.some((b) => b.label === "HIGH FAN-IN")).toBe(true);
    expect(badges.some((b) => b.label === "SECURITY")).toBe(true);
  });
});
