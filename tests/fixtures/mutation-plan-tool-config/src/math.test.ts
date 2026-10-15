import { expect, test } from "vitest";
import { double } from "./math";

test("double", () => {
  expect(double(2)).toBe(4);
});
