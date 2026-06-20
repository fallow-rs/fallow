import { describe, it, expect } from "vitest";
import { parseSourceAttr, readSourceFromElement, type SourceElement } from "./source";

describe("parseSourceAttr", () => {
  it("parses file:line:col", () => {
    expect(parseSourceAttr("src/Button.tsx:42:7")).toEqual({
      file: "src/Button.tsx",
      line: 42,
      column: 7,
    });
  });

  it("rejects malformed values", () => {
    expect(parseSourceAttr("nope")).toBeNull();
    expect(parseSourceAttr("a:b:c")).toBeNull();
  });
});

describe("readSourceFromElement", () => {
  it("reads the nearest data-fallow-source ancestor", () => {
    const el: SourceElement = {
      closest: () => ({ getAttribute: () => "src/Card.tsx:9:3" }),
    };
    expect(readSourceFromElement(el)).toEqual({ file: "src/Card.tsx", line: 9, column: 3 });
  });

  it("returns null when no ancestor is stamped", () => {
    const el: SourceElement = { closest: () => null };
    expect(readSourceFromElement(el)).toBeNull();
  });
});
