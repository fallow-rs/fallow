import { describe, it, expect } from "vitest";
import { decodePngDataUrl, shotPath } from "../src/main/shots";

describe("decodePngDataUrl", () => {
  it("decodes a base64 png data url to the original bytes", () => {
    const png = Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a]);
    const dataUrl = `data:image/png;base64,${png.toString("base64")}`;
    expect(decodePngDataUrl(dataUrl).equals(png)).toBe(true);
  });

  it("rejects a non-png data url", () => {
    expect(() => decodePngDataUrl("data:text/plain;base64,aGk=")).toThrow();
  });
});

describe("shotPath", () => {
  it("places shots under .fallow-review/shots", () => {
    expect(shotPath("/repo", 42)).toBe("/repo/.fallow-review/shots/shot-42.png");
  });
});
