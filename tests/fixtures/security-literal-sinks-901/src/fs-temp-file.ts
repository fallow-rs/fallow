import * as fs from "node:fs";
import { writeFileSync } from "node:fs";

export function writeTemporaryToken(token: string): void {
  fs.writeFileSync("/tmp/fallow-token", token);
  writeFileSync("/var/tmp/fallow-token", token);
}
