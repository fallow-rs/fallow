import { assertEquals } from "jsr:@std/assert";
import { span } from "../lib/span.ts";

Deno.test("span", () => {
  assertEquals(span([1, 2]), 3);
});
