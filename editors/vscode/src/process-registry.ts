import type { ChildProcess } from "node:child_process";

/**
 * Tracks every `fallow` child process spawned by the extension (analysis, audit,
 * health, security, fix, workspaces, and license invocations) so they can be
 * killed on extension deactivation.
 *
 * Without this, a window reload mid-analysis orphans the process: on Windows it
 * can keep file handles open on the project directory. Each spawn site registers
 * its child immediately and unregisters it from BOTH the `close` and `error`
 * handlers, so a finished process never lingers in the set.
 */
const activeChildren = new Set<ChildProcess>();

/** Track a freshly spawned child so {@link killActiveChildren} can reach it. */
export const registerChild = (child: ChildProcess): void => {
  activeChildren.add(child);
};

/** Drop a child once it has exited (settled via its `close` or `error` event). */
export const unregisterChild = (child: ChildProcess): void => {
  activeChildren.delete(child);
};

/**
 * Send SIGTERM to every tracked child and clear the registry. Called from
 * `deactivate()` so a window reload or extension shutdown does not orphan an
 * in-flight analysis. Errors from `kill` (e.g. the process already exited) are
 * swallowed: deactivation must never throw.
 */
export const killActiveChildren = (): void => {
  for (const child of activeChildren) {
    try {
      child.kill();
    } catch {
      // The process may have already exited between the iteration and the kill;
      // there is nothing actionable to do during teardown.
    }
  }
  activeChildren.clear();
};

/** Test-only: number of children currently tracked. */
export const activeChildCount = (): number => activeChildren.size;
