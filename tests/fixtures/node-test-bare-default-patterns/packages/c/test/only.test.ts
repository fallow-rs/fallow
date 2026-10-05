import { negate } from "../src/index.ts";

if (negate(1) !== -1) {
  throw new Error("negate failed");
}
