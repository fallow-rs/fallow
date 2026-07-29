import { expect, it, vi } from "vitest";

import { renderWrapper } from "./wrapper";

vi.mock("./dependency", () => ({
  renderDependency: vi.fn<() => string>(() => "mocked dependency"),
}));
vi.unmock("./dependency");

it("uses the restored dependency", () => {
  expect(renderWrapper()).toBe("real dependency");
});
