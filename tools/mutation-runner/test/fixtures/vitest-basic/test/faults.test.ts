import { expect, test, vi } from "vitest";
import { pause, settle, shout, spin } from "../src/faults";

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

test("spins", () => {
  expect(spin(3)).toBe(3);
});

test("spins on fake time", () => {
  vi.useFakeTimers({ toFake: ["performance", "Date", "setTimeout"] });
  vi.advanceTimersByTime(60_000);
  try {
    expect(spin(1000)).toBe(1000);
  } finally {
    vi.useRealTimers();
  }
});
