import { double } from "./index.ts";

if (double(2) !== 4) {
  throw new Error("double failed");
}
