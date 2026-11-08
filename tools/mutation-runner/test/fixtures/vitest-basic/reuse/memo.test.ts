import { expect, test } from "vitest";
import { double } from "../src/reuse";

test("doubles", () => {
  expect(double(2)).toBe(4);
});
