import { add } from "../src/index.ts";
import { fixture } from "./helpers.ts";

if (add(fixture, 1) !== 2) {
  throw new Error("add failed");
}
