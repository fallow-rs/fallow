import { describe, it, expect } from "vitest";
import { isCapturableUrl } from "./capture";

describe("isCapturableUrl", () => {
  it.each([
    ["http://localhost:5173/x", true],
    ["https://example.test/", true],
    ["file:///etc/passwd", false],
    ["chrome://settings", false],
    ["data:text/html,x", false],
    ["javascript:alert(1)", false],
    ["", false],
    ["not a url at all", false],
  ])("isCapturableUrl(%j) is %s", (url, expected) => {
    expect(isCapturableUrl(url)).toBe(expected);
  });
});
