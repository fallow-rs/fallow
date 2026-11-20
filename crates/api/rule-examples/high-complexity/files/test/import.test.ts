import { expect, it } from "vitest";
import { parseRow } from "../src/import";

it("reads an order row", () => {
  expect(parseRow(["A-1", "2"])).toEqual({ id: "A-1", quantity: 2 });
});
