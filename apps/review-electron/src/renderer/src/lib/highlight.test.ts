import { describe, it, expect } from "vitest";
import { tokenize, type TokenType } from "./highlight";

const types = (line: string): TokenType[] => tokenize(line).map((t) => t.type);
const value = (line: string, type: TokenType): string =>
  tokenize(line)
    .filter((t) => t.type === type)
    .map((t) => t.value)
    .join("");

describe("tokenize", () => {
  it("classifies keywords, strings, numbers, and comments", () => {
    const line = 'const x = "hi"; // note';
    expect(value(line, "keyword")).toBe("const");
    expect(value(line, "string")).toBe('"hi"');
    expect(value(line, "comment")).toBe("// note");
  });

  it("treats import specifiers and numbers correctly", () => {
    const line = "import { resolve } from 'node:path';";
    expect(value(line, "keyword")).toContain("import");
    expect(value(line, "string")).toBe("'node:path'");
    expect(value("const n = 0xff + 42e3;", "number")).toBe("0xff42e3");
  });

  it("round-trips the original text exactly", () => {
    const line = "export const fn = (a: number): void => {};";
    expect(
      tokenize(line)
        .map((t) => t.value)
        .join(""),
    ).toBe(line);
  });

  it("does not throw on an unterminated string or block comment", () => {
    expect(() => tokenize('const s = "oops')).not.toThrow();
    expect(types("/* open comment")).toEqual(["comment"]);
  });
});
