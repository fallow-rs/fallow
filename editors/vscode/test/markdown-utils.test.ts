import { describe, expect, it } from "vitest";
import {
  escapeMarkdownMultiline,
  escapeMarkdownText,
  normalizeInlineText,
} from "../src/markdown-utils.js";

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

describe("escapeMarkdownMultiline", () => {
  it("escapes backtick, asterisk, brackets, parentheses, and angle brackets (attack string)", () => {
    expect(escapeMarkdownMultiline("a`*[x](command:evil)*<b>")).toBe(
      "a\\`\\*\\[x\\]\\(command:evil\\)\\*\\<b\\>",
    );
  });

  it("benign case: _handleClick escapes the underscore (markdown-it consumes the backslash so the rendered hover shows _handleClick unchanged)", () => {
    // The escape is correct: markdown-it parses \_handleClick as the literal
    // text _handleClick with no visible backslash. Do not remove this escape.
    expect(escapeMarkdownMultiline("_handleClick")).toBe("\\_handleClick");
  });

  it("preserves newlines and internal spaces", () => {
    expect(escapeMarkdownMultiline("line one\nline two")).toBe("line one\nline two");
    expect(escapeMarkdownMultiline("a  b")).toBe("a  b");
  });

  it("empty string passes through", () => {
    expect(escapeMarkdownMultiline("")).toBe("");
  });
});
