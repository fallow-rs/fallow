// Build code writes computed paths without untrusted input.
import { writeFileSync } from "node:fs";

export function emit(outFile: string, code: string): void {
  writeFileSync(outFile, code);
}
