import { describe, expect, it } from "vitest";
import { escapeMarkdownText, normalizeInlineText } from "../src/markdown-utils.js";

describe("normalizeInlineText", () => {
  it("collapses internal whitespace runs to single spaces", () => {
    expect(normalizeInlineText("a   b\t\tc")).toBe("a b c");
  });

  it("trims leading and trailing whitespace, including newlines", () => {
    expect(normalizeInlineText("\n  hello world \n")).toBe("hello world");
  });
});

describe("escapeMarkdownText", () => {
  it("backslash-escapes markdown control characters", () => {
    expect(escapeMarkdownText("feature/x_(y)")).toBe("feature/x\\_\\(y\\)");
  });

  it("escapes the link/bold metacharacters that could inject a command link", () => {
    expect(escapeMarkdownText("[click](command:evil)")).toBe(
      "\\[click\\]\\(command:evil\\)",
    );
  });

  it("normalizes whitespace before escaping", () => {
    expect(escapeMarkdownText("a   b")).toBe("a b");
  });

  it("leaves plain text untouched", () => {
    expect(escapeMarkdownText("main")).toBe("main");
  });
});
