import { describe, expect, it, vi } from "vitest";

import { renderWrapper } from "./wrapper";

vi.mock("./dependency", () => ({
  renderDependency: vi.fn<() => string>(() => "mocked dependency"),
}));

describe("renderWrapper", () => {
  it("uses the replacement", () => {
    expect(renderWrapper()).toBe("mocked dependency");
  });
});
