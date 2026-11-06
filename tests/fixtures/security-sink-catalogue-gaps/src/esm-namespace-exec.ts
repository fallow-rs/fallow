import * as proc from "child_process";

export function run(command: string): void {
  proc.execSync(command);
}
