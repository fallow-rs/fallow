import * as child_process from "node:child_process";

export function runUserCommand(command: string): void {
  child_process.exec(command);
}
