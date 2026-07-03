// Test file (excluded from "production"). Its runtime import of `vitest` must
// NOT flag vitest: a devDependency used only in tests is correctly placed.
import { expect, test } from "vitest";
import { load } from "../index";

test("load", () => {
  expect(load("value: 1")).toBeDefined();
});
