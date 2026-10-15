import { expect, test } from "vitest";
import helpers from "./codegen-helpers";
import legacy from "./legacy.cjs";
import { double } from "./math";

test("double", () => {
  expect(double(2)).toBe(4);
});

test("modules that are not tool config", () => {
  expect(helpers.prefix).toBe("gql");
  expect(legacy.enabled).toBe(true);
});
