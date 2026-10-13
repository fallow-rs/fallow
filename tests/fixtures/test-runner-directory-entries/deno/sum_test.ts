import { assertEquals } from "jsr:@std/assert";
import { sum } from "./sum.ts";

Deno.test("sum", () => {
  assertEquals(sum([1, 2]), 3);
});
