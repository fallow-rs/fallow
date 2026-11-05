const cp = require("child_process");

export function run(command: string): void {
  cp.execSync(command);
}
