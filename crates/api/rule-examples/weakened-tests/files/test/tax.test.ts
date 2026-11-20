import { expect, it } from "vitest";
import { tax } from "../src/tax";

it.skip("charges tax", () => {
  expect(tax(100)).toBe(21);
});
