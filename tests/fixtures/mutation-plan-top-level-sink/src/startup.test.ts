import { expect, it } from "vitest";

import { double } from "./startup";

it("doubles", () => {
  expect(double(2)).toBe(4);
});
