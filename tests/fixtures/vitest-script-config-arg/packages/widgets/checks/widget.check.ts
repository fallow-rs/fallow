import { expect, test } from "vitest";
import { widget } from "../src/index";

test("widget", () => {
  expect(widget()).toBe("widget");
});
