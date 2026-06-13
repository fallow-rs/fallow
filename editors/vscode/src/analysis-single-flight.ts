/**
 * Coalesces overlapping calls to a single async task so concurrent triggers
 * cannot race on the shared state the task writes (last-writer-wins).
 *
 * Two trigger classes need different handling:
 * - background triggers (`force === false`): the lazy view-visibility latch, a
 *   workspace-scope change, and a config-driven re-analysis. While a run is in
 *   flight these dedup onto the existing run; they only want the latest result,
 *   not a guaranteed fresh spawn.
 * - explicit triggers (`force === true`): a user-invoked re-analyze or a
 *   post-fix re-run. These must reflect the latest on-disk / config state, so a
 *   force call arriving mid-run schedules exactly one re-run after the current
 *   one settles. Multiple force calls in flight coalesce into a single re-run.
 */
export interface SingleFlight {
  /**
   * Run the task, coalescing with any in-flight run.
   *
   * @param force when true, guarantees a fresh run completes after the current
   *   one (re-running once if a run is already in flight).
   */
  readonly run: (force: boolean) => Promise<boolean>;
}

export const createSingleFlight = (task: (force: boolean) => Promise<boolean>): SingleFlight => {
  let inFlight: Promise<boolean> | null = null;
  // A force call that arrived while a run was in flight; satisfied by exactly
  // one re-run after the current run settles, regardless of how many force
  // calls coalesced into it.
  let pendingForce: Promise<boolean> | null = null;

  const start = (force: boolean): Promise<boolean> => {
    const current = task(force).finally(() => {
      inFlight = null;
    });
    inFlight = current;
    return current;
  };

  const run = (force: boolean): Promise<boolean> => {
    if (!inFlight) {
      return start(force);
    }
    if (!force) {
      return inFlight;
    }
    if (!pendingForce) {
      // Chain off the settled in-flight run (ignoring its outcome) so the
      // re-run observes the latest state. The re-run is forced by definition
      // (it exists only because a force call arrived mid-run). Clear the slot
      // before re-running so a later force call schedules its own re-run rather
      // than reusing a stale promise.
      pendingForce = inFlight.then(
        () => {
          pendingForce = null;
          return start(true);
        },
        () => {
          pendingForce = null;
          return start(true);
        },
      );
    }
    return pendingForce;
  };

  return { run };
};
