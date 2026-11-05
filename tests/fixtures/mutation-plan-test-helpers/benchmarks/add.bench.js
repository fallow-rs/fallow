import { bench } from "vitest";
import { add } from "../src/add.js";
import { double } from "../src/fast.js";

bench("add", () => {
  double(add(1, 2));
});
