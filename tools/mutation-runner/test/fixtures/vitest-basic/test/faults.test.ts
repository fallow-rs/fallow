import { expect, test } from "vitest";
import { pause, settle, shout } from "../src/faults";

test("settles", () => {
  expect(settle(1)).toBe(1);
});

test("pauses", async () => {
  await pause();
  expect(true).toBe(true);
});

test("shouts", () => {
  expect(shout("a")).toBe("A");
});
