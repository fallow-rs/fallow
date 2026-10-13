import { expect, test } from "vitest";

test("leaves a rejection", () => {
  void Promise.reject(new Error("baseline unhandled"));
  expect(1).toBe(1);
});
