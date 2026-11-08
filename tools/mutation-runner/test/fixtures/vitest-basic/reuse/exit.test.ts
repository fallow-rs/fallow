import { expect, test } from "vitest";
import { stays } from "../src/reuse";

test("stays", () => {
  expect(stays()).toBe(1);
});
