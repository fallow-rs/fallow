import { expect, it, vi } from "vitest";

import { renderWrapper } from "./wrapper";

vi.mock("./dependency", () => ({
  renderDependency: vi.fn<() => string>(() => "mocked dependency"),
}));

it("uses the replacement", () => {
  expect(renderWrapper()).toBe("mocked dependency");
});
