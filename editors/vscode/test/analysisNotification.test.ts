import { describe, expect, it } from "vitest";
import { shouldAcceptLspAnalysisComplete } from "../src/analysisNotification.js";

describe("shouldAcceptLspAnalysisComplete", () => {
  it("rejects startup analysis notifications before any file document exists", () => {
    expect(shouldAcceptLspAnalysisComplete([])).toBe(false);
    expect(shouldAcceptLspAnalysisComplete([{ uri: { scheme: "output" } }])).toBe(false);
  });

  it("accepts analysis notifications once the editor has a file document", () => {
    expect(
      shouldAcceptLspAnalysisComplete([
        { uri: { scheme: "output" } },
        { uri: { scheme: "file" } },
      ]),
    ).toBe(true);
  });
});
