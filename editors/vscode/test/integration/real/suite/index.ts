import * as path from "node:path";
import Mocha from "mocha";

export const run = (): Promise<void> => {
  const mocha = new Mocha({ color: true, timeout: 60_000, ui: "bdd" });
  mocha.addFile(path.resolve(__dirname, "extension.test.js"));

  return new Promise((resolve, reject) => {
    mocha.run((failures) => {
      if (failures > 0) {
        reject(new Error(`${failures} real-process integration test(s) failed`));
        return;
      }
      resolve();
    });
  });
};
