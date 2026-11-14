// An options variable can set `shell`, so the call stays a candidate.
import { spawn } from "node:child_process";

export function run(command: string, options: { shell?: boolean }): void {
  spawn(command, [], options);
}
