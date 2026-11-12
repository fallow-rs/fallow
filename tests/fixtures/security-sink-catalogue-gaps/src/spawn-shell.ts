// With `shell: true`, spawn passes the command to a shell.
import { spawn } from "node:child_process";

export function run(command: string): void {
  spawn(command, [], { shell: true });
}
