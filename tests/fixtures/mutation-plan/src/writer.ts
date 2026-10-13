import { writeFileSync } from "node:fs";
import path from "node:path";

export function save(path: string, data: string): void {
  if (data.length > 0) {
    writeFileSync(path, data);
  }
}

export function size(data: string): number {
  return data.length + 1;
}

export function target(dir: string, name: string): string {
  return path.join(dir, name + ".txt");
}
