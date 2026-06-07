import * as fs from "node:fs";
import { chmodSync } from "node:fs";

export function makeWritable(file: string): void {
  fs.chmodSync(file, 0o777);
  chmodSync(file, 0o777);
}
