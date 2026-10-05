import { negate } from "../src/index.ts";

if (negate(2) !== -2) {
  throw new Error("negate failed");
}
