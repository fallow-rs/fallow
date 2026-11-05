const fs = require("fs");

export async function save(data: string): Promise<void> {
  await fs.promises.writeFile("/tmp/report.txt", data);
}
