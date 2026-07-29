import { expect, it } from "vitest";

import { renderDependency } from "./dependency";

it("uses the real dependency", () => {
  expect(renderDependency()).toBe("real dependency");
});
