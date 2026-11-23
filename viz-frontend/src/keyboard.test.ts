import { expect, it } from "vitest";
import { isDashboardKey } from "./keyboard";

const enter = {
  key: "Enter",
  keyCode: 13,
  defaultPrevented: false,
  isComposing: false,
  ctrlKey: false,
  metaKey: false,
  altKey: false,
};

it("leaves a handled Escape and browser commands to their owners", () => {
  const escape = { ...enter, key: "Escape", keyCode: 27 };
  const one = { ...enter, key: "1", keyCode: 49 };
  expect(isDashboardKey(escape)).toBe(true);
  expect(isDashboardKey({ ...escape, defaultPrevented: true })).toBe(false);
  expect(isDashboardKey(one)).toBe(true);
  expect(isDashboardKey({ ...one, ctrlKey: true })).toBe(false);
});

it("only accepts Enter for search after the IME commit keystroke", () => {
  expect(isDashboardKey({ ...enter, isComposing: true })).toBe(false);
  expect(isDashboardKey({ ...enter, keyCode: 229 })).toBe(false);
  expect(isDashboardKey(enter)).toBe(true);
});
