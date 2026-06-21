import { describe, it, expect } from "vitest";
import { deriveFileSignal, fanInTone } from "./badges";
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

describe("deriveFileSignal", () => {
  it("parses fan-in and fan-out from the reason", () => {
    const s = deriveFileSignal(file({ reason: "high fan-in (17 importers), fan-out 2" }));
    expect(s.fanIn).toBe(17);
    expect(s.fanOut).toBe(2);
    expect(s.fanInTone).toBe("hub");
  });

  it("handles a single importer and isolated changes", () => {
    expect(deriveFileSignal(file({ reason: "high fan-in (1 importer)" })).fanIn).toBe(1);
    const iso = deriveFileSignal(file({ reason: "isolated change, no blast beyond the diff" }));
    expect(iso.fanIn).toBe(0);
    expect(iso.isolated).toBe(true);
  });

  it("flags security and risk zones from the score", () => {
    const s = deriveFileSignal(
      file({ score: { fanIo: 9, securityTaint: 1, riskZone: 1, changeShape: 0, total: 10 } }),
    );
    expect(s.security).toBe(true);
    expect(s.riskZone).toBe(true);
  });
});

describe("fanInTone", () => {
  it("only grades genuine hubs as accented", () => {
    expect(fanInTone(1)).toBe("muted");
    expect(fanInTone(3)).toBe("elevated");
    expect(fanInTone(6)).toBe("hub");
  });
});
