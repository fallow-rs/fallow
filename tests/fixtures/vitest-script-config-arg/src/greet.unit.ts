import { expect, test } from "vitest";
import { greet } from "./index";

test("greet", () => {
  expect(greet("unit")).toBe("hello unit");
});
