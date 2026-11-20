import { expect, it } from "vitest";
import { route } from "../src/router";

it("routes the cart", () => {
  expect(route("/cart")).toBe("cart");
});
