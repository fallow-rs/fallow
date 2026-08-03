import { describe, expect, it, vi } from "vitest";

describe("renderWrapper", () => {
  it("uses the replacement for imports after the call", async () => {
    vi.doMock("./dependency", () => ({
      renderDependency: vi.fn<() => string>(() => "mocked dependency"),
    }));
    const { renderWrapper } = await import("./wrapper");
    expect(renderWrapper()).toBe("mocked dependency");
  });
});
