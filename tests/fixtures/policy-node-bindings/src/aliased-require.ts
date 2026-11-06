const { exec: run } = require("child_process");

export function start(cmd: string): void {
  run(cmd);
}
