import { describe, it, expect } from "vitest";
import { sanitizeRef } from "./diff";

describe("sanitizeRef", () => {
  it("accepts well-formed refs unchanged", () => {
    expect(sanitizeRef("HEAD")).toBe("HEAD");
    expect(sanitizeRef("HEAD~2")).toBe("HEAD~2");
    expect(sanitizeRef("HEAD^")).toBe("HEAD^");
    expect(sanitizeRef("main")).toBe("main");
    expect(sanitizeRef("origin/main")).toBe("origin/main");
    expect(sanitizeRef("feature/foo-bar")).toBe("feature/foo-bar");
    expect(sanitizeRef("v1.2.3")).toBe("v1.2.3");
    expect(sanitizeRef("a1b2c3d")).toBe("a1b2c3d");
    expect(sanitizeRef("a1b2c3d4e5f6789012345678901234567890abcd")).toBe(
      "a1b2c3d4e5f6789012345678901234567890abcd",
    );
  });

  it("falls back to HEAD for option-injection attempts (a ref beginning with '-')", () => {
    expect(sanitizeRef("--output=/tmp/x")).toBe("HEAD");
    expect(sanitizeRef("-rf")).toBe("HEAD");
    expect(sanitizeRef("--upload-pack=evil")).toBe("HEAD");
  });

  it("falls back to HEAD for empty input", () => {
    expect(sanitizeRef("")).toBe("HEAD");
  });
});
