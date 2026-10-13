import { beforeAll, expect, test } from "vitest";
import { unused } from "../src/unused";

beforeAll(async () => {
  await new Promise((resolve) => setTimeout(resolve, 1500));
});

test("fast after a slow suite hook", () => {
  expect(typeof unused).toBe("function");
});
