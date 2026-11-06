import cp from "node:child_process";

export function run(command: string): void {
  cp.exec(command);
}
