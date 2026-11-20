import { expect, it } from "vitest";
import { applyDiscount } from "../src/discount";

it("gives no discount to a guest", () => {
  expect(applyDiscount([], false)).toBe(0);
});
