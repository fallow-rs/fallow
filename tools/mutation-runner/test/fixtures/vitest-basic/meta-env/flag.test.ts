import { afterEach, expect, test, vi } from "vitest";
import { readFlag } from "./flag";

afterEach(() => {
  vi.unstubAllEnvs();
});

test("is off without the variable", () => {
  expect(readFlag()).toBe("off");
});

test("reads a stubbed variable", () => {
  vi.stubEnv("VITE_FLAG", "on");
  expect(readFlag()).toBe("on");
});
