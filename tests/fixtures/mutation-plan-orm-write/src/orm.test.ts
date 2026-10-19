import { expect, test } from "vitest";

import { evict } from "./cache";
import { addPerson } from "./person";
import { clamp, save } from "./save";

test("pure helpers", () => {
  expect(evict(new Map([[1, "a"]]), 1)).toBe(true);
  expect(clamp(-1)).toBe(0);
});

test("writers exist", () => {
  expect(typeof save).toBe("function");
  expect(typeof addPerson).toBe("function");
});
