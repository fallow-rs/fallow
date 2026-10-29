import { expect, test } from "vitest";
import { Counter } from "./counter";
import { gauge } from "./harness";
import { useCounterStore } from "./store";

test("counter", () => {
  const counter = new Counter();
  counter.reset();
  expect(counter.read()).toBe(0);
  const store = useCounterStore();
  expect(store.debugLabel).toBe("count 1");
  expect(store.doubled).toBe(2);
  gauge.reset();
});
