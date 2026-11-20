import { expect, it } from "vitest";
import { formatPrice } from "../src/money";

it("formats a price", () => {
  expect(formatPrice(5)).toBe("EUR 5.00");
});
