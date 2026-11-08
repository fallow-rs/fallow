import { beforeEach, expect, test } from "vitest";
import { reset, store } from "../src/store";

beforeEach(() => reset());

test("fills the store", () => {
  // The longer file runs first in a shared worker.
  const until = performance.now() + 40;
  while (performance.now() < until) {
    // Busy wait.
  }
  store.value = 1;
  expect(store.value).toBe(1);
});
