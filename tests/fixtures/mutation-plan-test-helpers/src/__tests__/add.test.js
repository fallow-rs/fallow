import { expect, test } from "vitest";
import { add } from "../add.js";
import { sum3 } from "./utils";

test("add", () => {
  expect(add(1, 2)).toBe(3);
  expect(sum3(1, 2, 3)).toBe(6);
});
