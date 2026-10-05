import { double } from "./src/index.ts";

if (double(3) !== 6) {
  throw new Error("double failed");
}
