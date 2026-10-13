import { writeFileSync } from "node:fs";

const target = process.argv[2];

if (process.argv.length > 2) {
  writeFileSync(target, "ready");
}

export function double(value: number): number {
  return value * 2;
}
