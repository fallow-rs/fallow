import { expect, test } from "bun:test";

import { add, isPositive } from "../src/math";

const wait = (ms: number): Promise<void> => new Promise((resolve) => setTimeout(resolve, ms));

test.concurrent("adds later", async () => {
  await wait(60);
  expect(add(2, 3)).toBe(5);
});

test.concurrent("checks the sign of zero", async () => {
  const result = isPositive(0);
  await wait(150);
  expect(result).toBe(false);
});
