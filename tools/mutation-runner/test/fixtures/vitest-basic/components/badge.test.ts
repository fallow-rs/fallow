import { expect, test } from "vitest";

import { label, style } from "../src/Badge.vue";

test("label of one", () => {
  expect(label(1)).toBe("one");
});

test("label of two", () => {
  expect(label(2)).toBe("many");
});

test("style block", () => {
  expect(style).toBe(".badge {\n  color: red;\n}");
});
