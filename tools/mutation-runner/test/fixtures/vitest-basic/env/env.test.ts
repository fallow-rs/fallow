import { expect, test } from "vitest";
import { add } from "../src/math";
import { databaseUrl } from "./env";

test("loads the env module and checks add", () => {
  expect(databaseUrl).toBe("file:test.db");
  expect(add(2, 3)).toBe(5);
});

test("sees the variables of the Vitest CLI", () => {
  expect({ TEST: process.env.TEST, VITEST: process.env.VITEST }).toEqual({
    TEST: "true",
    VITEST: "true",
  });
});
