// `shell: false` keeps the default: spawn runs no shell.
import { spawn } from "node:child_process";

export function run(binary: string): void {
  spawn(binary, ["--version"], { shell: false });
}
