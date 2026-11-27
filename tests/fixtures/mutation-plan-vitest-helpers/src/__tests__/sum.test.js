import { expect, test } from "vitest";
import { sum } from "../sum.js";
import { pair } from "./support/fixtures.js";
import { three } from "./support/test-gateway.js";

test("sum", () => {
  expect(sum(...pair)).toBe(3);
  expect(three()).toBe(3);
});
