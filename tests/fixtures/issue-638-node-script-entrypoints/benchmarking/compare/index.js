import { fork } from "node:child_process";
import path from "node:path";
import { fileURLToPath } from "node:url";

const filename = fileURLToPath(import.meta.url);
const runner = path.resolve(filename, "../runner.js");

for (const branch of ["main"]) {
  await new Promise((fulfil, reject) => {
    const child = fork(runner, [branch], { stdio: "inherit" });
    child.on("message", fulfil);
    child.on("error", reject);
  });
}
