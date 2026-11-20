/**
 * Outcome of one load of a lazy view.
 *
 * - `loaded`: the run completed and `value` goes to the view.
 * - `skipped`: the run completed with nothing to show. The view keeps its
 *   content and a later reveal does not start a new run.
 * - `failed`: a transient failure. A later reveal starts a new run. A run that
 *   throws counts as `failed`.
 */
export type LazyRunOutcome<T> =
  | { readonly kind: "loaded"; readonly value: T }
  | { readonly kind: "skipped" }
  | { readonly kind: "failed" };

interface LazyViewState {
  /** True after the first `reveal` or `reload` call. */
  readonly revealed: boolean;
}

interface LazyViewOptions<T> {
  /** The feature switch. While false, no run starts and no result is applied. */
  readonly enabled: () => boolean;
  /** Extra gate for `reveal` only. `reload` ignores it. */
  readonly autoLoad?: () => boolean;
  /** One string for all inputs of the run. Must be synchronous and stable. */
  readonly inputKey: () => string;
  /** Decides if a changed input key starts a run now or on the next reveal. */
  readonly reloadStaleWhen: (state: LazyViewState) => boolean;
  readonly run: () => Promise<LazyRunOutcome<T>>;
  /** Shows a loaded value. Called only for the newest run with current inputs. */
  readonly apply: (value: T) => void;
  /** Empties the view. Called by `clear` and when the feature is disabled. */
  readonly reset: () => void;
}

interface LazyView {
  /** Loads once per input key. Call it when the view becomes visible. */
  readonly reveal: () => Promise<void>;
  /** Starts a run in all cases, unless the feature is disabled. */
  readonly reload: () => Promise<void>;
  /** Call it after any event that can change `enabled` or `inputKey`. */
  readonly invalidate: () => Promise<void>;
  /** Empties the view and drops the result of a run in flight. */
  readonly clear: () => void;
}

/**
 * Creates the load latch of one lazy view. A result reaches `apply` only when
 * its run is the newest one, the feature is still enabled and the input key
 * did not change during the run.
 */
export const createLazyView = <T>(opts: LazyViewOptions<T>): LazyView => {
  // Each run, each clear and each stale mark takes a new generation. A run
  // applies its result only when its generation is still the newest one.
  let generation = 0;
  let lastKey: string | null = null;
  let latched = false;
  let shown = false;
  let revealed = false;

  const clear = (): void => {
    generation += 1;
    lastKey = null;
    latched = false;
    shown = false;
    opts.reset();
  };

  const clearIfDisabled = (): boolean => {
    if (opts.enabled()) {
      return false;
    }
    if (latched || shown) {
      clear();
    }
    lastKey = null;
    return true;
  };

  const start = async (key: string): Promise<void> => {
    generation += 1;
    const mine = generation;
    lastKey = key;
    latched = true;

    let outcome: LazyRunOutcome<T>;
    try {
      outcome = await opts.run();
    } catch {
      outcome = { kind: "failed" };
    }

    if (mine !== generation) {
      return;
    }
    if (clearIfDisabled()) {
      return;
    }
    if (outcome.kind === "failed" || opts.inputKey() !== key) {
      latched = false;
      return;
    }
    if (outcome.kind === "loaded") {
      shown = true;
      opts.apply(outcome.value);
    }
  };

  const reveal = async (): Promise<void> => {
    revealed = true;
    if (clearIfDisabled()) {
      return;
    }
    if (opts.autoLoad && !opts.autoLoad()) {
      return;
    }
    const key = opts.inputKey();
    if (latched && lastKey === key) {
      return;
    }
    await start(key);
  };

  const reload = async (): Promise<void> => {
    revealed = true;
    if (clearIfDisabled()) {
      return;
    }
    await start(opts.inputKey());
  };

  const invalidate = async (): Promise<void> => {
    if (clearIfDisabled()) {
      return;
    }
    const key = opts.inputKey();
    // Compare with the key and not with the latch: a failed run releases the
    // latch but keeps its key, and an unrelated change must not repeat it.
    if (key === lastKey) {
      return;
    }
    if (opts.reloadStaleWhen({ revealed })) {
      await start(key);
      return;
    }
    generation += 1;
    lastKey = key;
    latched = false;
  };

  return { reveal, reload, invalidate, clear };
};
