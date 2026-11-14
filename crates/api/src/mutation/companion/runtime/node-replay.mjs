// Generated from tools/mutation-runner/src/runtime/node-replay.mjs. Do not edit.
import { AsyncLocalStorage } from "node:async_hooks";
import { readFileSync, writeFileSync } from "node:fs";
import { pathToFileURL } from "node:url";

import { installFallowMutation } from "./global.mjs";
import { runCall } from "./replay.mjs";

// The node:test adapter starts this file once for each replay call, so each
// call has a new process and new module instances. The subprocess preload in
// NODE_OPTIONS serves the instrumented files and sets the active mutant of
// the side, also for each Node process that the call starts. The arguments
// are the request file, the side and the result file.
const [requestPath, side, resultPath] = process.argv.slice(2);
const request = JSON.parse(readFileSync(requestPath, "utf8"));
const control = installFallowMutation();
control.enableContext(new AsyncLocalStorage());
const result = await runCall(request, side, {
  control,
  load: () => import(pathToFileURL(request.module_path).href),
});
writeFileSync(resultPath, JSON.stringify(result));
// A timer or a socket of the call must not keep the process alive.
process.exit(0);
