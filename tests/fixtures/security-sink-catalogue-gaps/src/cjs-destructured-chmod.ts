const { chmodSync } = require("fs");

export function open(file: string): void {
  chmodSync(file, 0o777);
}
