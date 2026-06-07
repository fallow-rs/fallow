import * as fs from "node:fs";
import { chmodSync, writeFileSync } from "node:fs";

export function writeTemporaryToken(token: string, file: string): void {
  fs.chmodSync(file, 0o777);
  chmodSync(file, 0o777);
  fs.writeFileSync("/tmp/fallow-token", token);
  writeFileSync("/var/tmp/fallow-token", token);
}
