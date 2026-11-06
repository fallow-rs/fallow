const { exec: run } = require("node:child_process");

// A local function with the name of the destructured key is not the binding.
const exec = (cmd: string): string => cmd;

export function local(cmd: string): void {
  exec(cmd);
  void run;
}
