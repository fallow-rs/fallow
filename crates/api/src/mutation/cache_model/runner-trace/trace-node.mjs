// Records each file that a Node process reads or loads as a module. The
// cache model tests of the mutation result cache start the test runner with
// `--import` of this file. Each record is one absolute path on its own line
// of the file that `FALLOW_RUNNER_TRACE` names.

import fs from "node:fs";
import { registerHooks, syncBuiltinESMExports } from "node:module";
import { resolve } from "node:path";
import { fileURLToPath } from "node:url";

const out = process.env.FALLOW_RUNNER_TRACE;
const appendFileSync = fs.appendFileSync;

const record = (target) => {
  if (!out) return;
  let path = null;
  if (typeof target === "string") path = target;
  else if (target instanceof URL && target.protocol === "file:") path = fileURLToPath(target);
  if (path === null) return;
  try {
    appendFileSync(out, `${resolve(path)}\n`);
  } catch {
    // A failed record must never fail the run of the runner.
  }
};

const wrap = (owner, name) => {
  const original = owner[name];
  if (typeof original !== "function") return;
  owner[name] = function traced(target, ...rest) {
    record(target);
    return original.call(this, target, ...rest);
  };
};

wrap(fs, "readFileSync");
wrap(fs, "readFile");
wrap(fs.promises, "readFile");
syncBuiltinESMExports();

registerHooks({
  load(url, context, next) {
    if (url.startsWith("file:")) record(new URL(url));
    return next(url, context);
  },
});
