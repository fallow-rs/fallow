// Generated from tools/mutation-runner/src/runtime/vitest-replay.mjs. Do not edit.
import { AsyncLocalStorage } from "node:async_hooks";

import { inject, test, vi } from "vitest";

import { installFallowMutation } from "./global.mjs";
import { runReplay } from "./replay.mjs";
import { setSubprocessPreload } from "./subprocess-options.mjs";

// The Vitest adapter runs this file as the only test file of a replay run,
// in a new worker for each replay request. The setup file of the adapter
// ran before it, so the runtime and the overlay are in place.
const REPLAY_KEY = "fallowReplay";
const SUBPROCESS_PRELOAD_KEY = "fallowSubprocessPreload";

const request = inject(REPLAY_KEY);
const preload = inject(SUBPROCESS_PRELOAD_KEY);
const control = installFallowMutation();
control.enableContext(new AsyncLocalStorage());

test(
  "fallow replay",
  async ({ task }) => {
    task.meta.fallowReplay = await runReplay(request, {
      control,
      // A new module registry for each call, so the module state and a
      // top-level mutant of one call never reach the next call.
      load: async () => {
        vi.resetModules();
        return import(/* @vite-ignore */ request.module_path);
      },
      // A Node process that the call starts loads the same side.
      beforeCall: (active) => {
        if (typeof preload === "string") setSubprocessPreload(preload, { active });
      },
    });
  },
  request.test_timeout_ms,
);
