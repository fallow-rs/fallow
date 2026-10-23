import { expect, test } from "vitest";
import { exposed } from "./exposed";
import { double, triple } from "./math";

test("double", () => {
  expect(double(2)).toBe(4);
  expect(triple(2)).toBe(6);
  expect(exposed()).toBe("exposed");
});
