// A parameter named `exec` hides the node:child_process import.
import { exec } from "node:child_process";

export function run(exec: (command: string) => void, command: string): void {
  exec(command);
}

export { exec };
