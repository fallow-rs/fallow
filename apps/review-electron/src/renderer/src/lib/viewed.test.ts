import { describe, it, expect } from "vitest";
import { isViewed, setViewed, type KeyValueStore } from "./viewed";

const fakeStore = (): KeyValueStore => {
  const m = new Map<string, string>();
  return {
    getItem: (k) => m.get(k) ?? null,
    setItem: (k, v) => {
      m.set(k, v);
    },
  };
};

describe("viewed state", () => {
  it("round-trips viewed flags by path", () => {
    const store = fakeStore();
    expect(isViewed(store, "a.ts")).toBe(false);
    setViewed(store, "a.ts", true);
    expect(isViewed(store, "a.ts")).toBe(true);
    setViewed(store, "a.ts", false);
    expect(isViewed(store, "a.ts")).toBe(false);
  });
});
