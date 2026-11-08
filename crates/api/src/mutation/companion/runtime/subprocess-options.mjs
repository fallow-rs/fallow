// Generated from tools/mutation-runner/src/runtime/subprocess-options.mjs. Do not edit.
// This module has no Node import, because the Vitest setup file loads it
// also in browser mode.

const escapeRegExp = (text) => text.replace(/[.*+?^${}()|[\]\\]/gu, "\\$&");

/**
 * Sets the `--import` flag of the subprocess preload in NODE_OPTIONS, so a
 * Node process that a test starts (spawn, exec, execFile or fork) loads the
 * instrumented files. `preload` is the preload URL with its base query, and
 * `params` adds query values; a null value is not added.
 *
 * The function first removes the flag of an earlier call, because a test
 * process keeps its environment from one test to the next. The preload URL
 * has no space or quote, so the NODE_OPTIONS parse keeps it as one argument.
 * The existing value comes first, so a loader of the project keeps its order.
 */
export const setSubprocessPreload = (preload, params) => {
  const url = new URL(preload);
  for (const [name, value] of Object.entries(params)) {
    if (value !== null) url.searchParams.set(name, value);
  }
  const base = preload.split("?", 1)[0];
  const earlier = new RegExp(`(?:^|\\s+)--import=${escapeRegExp(base)}(?:\\?\\S*)?(?=\\s|$)`, "gu");
  const current = (process.env.NODE_OPTIONS ?? "").replace(earlier, "").trim();
  const flag = `--import=${url.href}`;
  process.env.NODE_OPTIONS = current ? `${current} ${flag}` : flag;
};
