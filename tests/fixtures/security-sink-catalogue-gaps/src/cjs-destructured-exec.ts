const { exec } = require("node:child_process");

export function run(command: string): void {
  exec(command);
}
