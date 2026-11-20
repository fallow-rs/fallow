import { expect, it } from "vitest";
import { cartTotal } from "./cart";

it("adds the prices", () => {
  expect(cartTotal([2, 3])).toBe(5);
});
