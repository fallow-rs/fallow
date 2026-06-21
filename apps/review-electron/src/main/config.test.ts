import { describe, it, expect } from "vitest";
import { parseConfig, stripJsonc, DEFAULT_CONFIG } from "./config";

describe("stripJsonc", () => {
  it("strips comments and trailing commas but preserves URLs", () => {
    const out = stripJsonc(`{
      // a comment
      /* block */
      "defaultUrl": "http://localhost:5999",
    }`);
    expect(out).toContain("http://localhost:5999");
    expect(JSON.parse(out)).toEqual({ defaultUrl: "http://localhost:5999" });
  });
});

describe("parseConfig", () => {
  it("merges a partial JSONC config over defaults", () => {
    const cfg = parseConfig(`{
      // override only the port
      "inspectPort": 8000,
      "fallowBin": "/usr/local/bin/fallow",
    }`);
    expect(cfg.inspectPort).toBe(8000);
    expect(cfg.fallowBin).toBe("/usr/local/bin/fallow");
    expect(cfg.defaultUrl).toBe(DEFAULT_CONFIG.defaultUrl);
    expect(cfg.agentBackend).toBe(DEFAULT_CONFIG.agentBackend);
  });

  it("ignores wrong types and bad json (falls back to defaults)", () => {
    expect(parseConfig(`{ "inspectPort": "nope" }`).inspectPort).toBe(DEFAULT_CONFIG.inspectPort);
    expect(parseConfig("not json at all")).toEqual(DEFAULT_CONFIG);
  });
});
