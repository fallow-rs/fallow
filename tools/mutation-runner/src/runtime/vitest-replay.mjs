import { AsyncLocalStorage } from "node:async_hooks";

import { inject, test } from "vitest";

import { installFallowMutation } from "./global.mjs";
import { runCall } from "./replay.mjs";
import { setSubprocessPreload } from "./subprocess-options.mjs";

// The Vitest adapter runs this file as the only test file of a run, once for
// each replay call, so each call has a new worker and new modules. The setup
// file of the adapter ran before it, so the runtime and the overlay are in
// place.
const REPLAY_KEY = "fallowReplay";
const SUBPROCESS_PRELOAD_KEY = "fallowSubprocessPreload";
const REQUIRE_HOOK = Symbol.for("fallow.mutation.vitestRequireHook");

const request = inject(REPLAY_KEY);
const preload = inject(SUBPROCESS_PRELOAD_KEY);
const control = installFallowMutation();
control.enableContext(new AsyncLocalStorage());
// Without the synchronous hook API, a child process cannot load the
// instrumented files, so the preload stays unset as in the setup file.
const hooksReady = globalThis[REQUIRE_HOOK]?.hooksReady === true;

test(
  "fallow replay",
  async ({ task }) => {
    task.meta.fallowReplay = await runCall(request, request.side, {
      control,
      load: () => import(/* @vite-ignore */ request.module_path),
      // A Node process that the call starts loads the same side.
      beforeCall: (active) => {
        if (hooksReady && typeof preload === "string") setSubprocessPreload(preload, { active });
      },
    });
  },
  request.test_timeout_ms,
);
