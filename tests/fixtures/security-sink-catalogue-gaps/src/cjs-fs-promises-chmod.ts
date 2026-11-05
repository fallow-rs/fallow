const fs = require("node:fs");

export async function open(file: string): Promise<void> {
  await fs.promises.chmod(file, 0o777);
}
