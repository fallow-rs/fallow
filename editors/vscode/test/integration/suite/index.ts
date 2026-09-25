import * as fs from "node:fs";
import * as path from "node:path";
import Mocha from "mocha";

const collectTestFiles = (dir: string): string[] => {
  const entries = fs.readdirSync(dir, { withFileTypes: true });
  return entries.flatMap((entry) => {
    const resolved = path.join(dir, entry.name);
    if (entry.isDirectory()) {
      return collectTestFiles(resolved);
    }
    return entry.name.endsWith(".test.js") ? [resolved] : [];
  });
};

/** Run each `*.test.js` file under `testsRoot` in one Mocha run. */
export const runMochaSuite = async (testsRoot: string): Promise<void> => {
  const mocha = new Mocha({
    ui: "bdd",
    color: true,
    timeout: 20_000,
  });

  for (const file of collectTestFiles(testsRoot)) {
    mocha.addFile(file);
  }

  await new Promise<void>((resolve, reject) => {
    mocha.run((failures) => {
      if (failures > 0) {
        reject(new Error(`${failures} test(s) failed.`));
        return;
      }
      resolve();
    });
  });
};

export async function run(): Promise<void> {
  await runMochaSuite(__dirname);
}
