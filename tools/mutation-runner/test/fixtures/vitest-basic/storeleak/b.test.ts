import { beforeEach, expect, test } from "vitest";
import { reset, store } from "../src/store";

beforeEach(() => reset());

test("starts empty", () => {
  expect(store.value).toBeUndefined();
});
