// A parameter named `cp` hides the child_process namespace import.
import * as cp from "node:child_process";

export function run(cp: { exec(command: string): void }, command: string): void {
  cp.exec(command);
}

export { cp };
