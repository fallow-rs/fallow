import { expect, test } from "bun:test";

import { pause, settle, shout } from "../src/faults";

test("settles", () => {
  expect(settle(1)).toBe(1);
});

test("pauses", async () => {
  await pause();
});

test("shouts", () => {
  expect(shout("a")).toBe("A");
});
