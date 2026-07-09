import { describe, it, expect } from "vitest";
import { isCapturableUrl } from "./capture";

describe("isCapturableUrl", () => {
  it("accepts http URLs", () => {
    expect(isCapturableUrl("http://localhost:5173/x")).toBe(true);
  });

  it("accepts https URLs", () => {
    expect(isCapturableUrl("https://example.test/")).toBe(true);
  });

  it("rejects file: URLs", () => {
    expect(isCapturableUrl("file:///etc/passwd")).toBe(false);
  });

  it("rejects chrome: URLs", () => {
    expect(isCapturableUrl("chrome://settings")).toBe(false);
  });

  it("rejects data: URLs", () => {
    expect(isCapturableUrl("data:text/html,x")).toBe(false);
  });

  it("rejects javascript: URLs", () => {
    expect(isCapturableUrl("javascript:alert(1)")).toBe(false);
  });

  it("rejects an empty string", () => {
    expect(isCapturableUrl("")).toBe(false);
  });

  it("rejects garbage input", () => {
    expect(isCapturableUrl("not a url at all")).toBe(false);
  });
});
