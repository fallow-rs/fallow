// The last `shell` key wins at runtime, so this spawn call runs a shell.
import { spawn } from "node:child_process";

export function run(command) {
  spawn(command, [], { shell: false, shell: true });
}
