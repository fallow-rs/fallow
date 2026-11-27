// Write a PGO cargo build log to a file and show it without the profile noise.
//
// Usage:
//   cargo build ... 2>&1 | node pgo-build-log.mjs <log-file>
//
// The PGO build passes `-Cllvm-args=-pgo-warn-missing-function`, so LLVM
// prints one warning for each function without a profile record: thousands
// for each release leg. pgo-profile-match.mjs counts these warnings in the
// log file, so the file keeps every line. The console drops each warning and
// the blank line that cargo prints after it. Run the pipe with `pipefail`, so
// the step still fails when cargo fails.

import { createWriteStream, realpathSync } from "node:fs";
import { createInterface } from "node:readline";
import { pathToFileURL } from "node:url";

import { MISSING_FUNCTION } from "./pgo-profile-match.mjs";

export const main = async (argv = process.argv.slice(2), input = process.stdin) => {
  const [logPath] = argv;
  if (!logPath) {
    console.error("usage: pgo-build-log.mjs <log-file>");
    return 2;
  }
  const log = createWriteStream(logPath);
  input.on("data", (chunk) => log.write(chunk));
  let missing = 0;
  let afterWarning = false;
  for await (const line of createInterface({ input, crlfDelay: Infinity })) {
    if (MISSING_FUNCTION.test(line)) {
      missing += 1;
      afterWarning = true;
      continue;
    }
    if (afterWarning && line.trim() === "") {
      afterWarning = false;
      continue;
    }
    afterWarning = false;
    process.stdout.write(`${line}\n`);
  }
  await new Promise((resolve, reject) => log.end((error) => (error ? reject(error) : resolve())));
  process.stdout.write(`${missing} PGO missing-function warnings written to ${logPath}\n`);
  return 0;
};

// import.meta.url holds the real path, encoded as a URL. Compare it with the
// same form of argv[1], so a symlink, a space or a Windows path still runs main.
if (process.argv[1] && import.meta.url === pathToFileURL(realpathSync(process.argv[1])).href) {
  process.exitCode = await main();
}
