import { expect, test } from "vitest";

import { isPositive } from "../src/sign";

test("a positive value", () => {
  expect(isPositive(1)).toBe(true);
});
