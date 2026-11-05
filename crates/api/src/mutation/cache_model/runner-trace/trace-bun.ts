// Records each module that a `bun test` process loads. The cache model tests
// of the mutation result cache pass this file with `--preload`. Bun loads
// modules in native code, so no JavaScript hook sees each load. After the
// last test, `require.cache` holds each loaded module, ES modules included.
// Each record is one absolute path on its own line of the file that
// `FALLOW_RUNNER_TRACE` names.

import { appendFileSync } from "node:fs";
import { afterAll } from "bun:test";

afterAll(() => {
  const out = process.env.FALLOW_RUNNER_TRACE;
  if (!out) return;
  const paths = Object.keys(require.cache).filter((path) => path !== import.meta.path);
  appendFileSync(out, paths.map((path) => `${path}\n`).join(""));
});
