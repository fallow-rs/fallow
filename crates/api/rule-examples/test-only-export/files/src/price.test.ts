import { expect, it } from "vitest";
import { formatPrice, legacyFormat } from "./price";

it("formats", () => {
  expect(formatPrice(150)).toBe("1.5 EUR");
  expect(legacyFormat(150)).toBe("EUR 1.5");
});
