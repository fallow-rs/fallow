import { describe, expect, it } from "vitest";
import { createLazyView, type LazyRunOutcome } from "../src/lazyView.js";

interface Deferred<T> {
  readonly promise: Promise<T>;
  readonly resolve: (value: T) => void;
  readonly reject: (error: unknown) => void;
}

const deferred = <T>(): Deferred<T> => {
  let resolve!: (value: T) => void;
  let reject!: (error: unknown) => void;
  const promise = new Promise<T>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
};

interface HarnessOptions {
  readonly autoLoad?: () => boolean;
}

const createHarness = (options: HarnessOptions = {}) => {
  const state = {
    enabled: true,
    key: "a",
    staleReload: false,
    runs: 0,
    pending: [] as Deferred<LazyRunOutcome<string>>[],
    applied: [] as string[],
    resets: 0,
  };
  const view = createLazyView<string>({
    enabled: () => state.enabled,
    autoLoad: options.autoLoad,
    inputKey: () => state.key,
    reloadStaleWhen: () => state.staleReload,
    run: () => {
      state.runs += 1;
      const run = deferred<LazyRunOutcome<string>>();
      state.pending.push(run);
      return run.promise;
    },
    apply: (value) => {
      state.applied.push(value);
    },
    reset: () => {
      state.resets += 1;
    },
  });
  const settle = (index: number, outcome: LazyRunOutcome<string>): void => {
    state.pending[index]?.resolve(outcome);
  };
  return { state, view, settle };
};

const loaded = (value: string): LazyRunOutcome<string> => ({ kind: "loaded", value });

describe("createLazyView", () => {
  it("L1: loads once for two reveals with the same key", async () => {
    const { state, view, settle } = createHarness();
    const first = view.reveal();
    settle(0, loaded("A"));
    await first;
    await view.reveal();

    expect(state.runs).toBe(1);
    expect(state.applied).toEqual(["A"]);
  });

  it("L2: runs again on the next reveal after a failed run", async () => {
    const { state, view, settle } = createHarness();
    const first = view.reveal();
    settle(0, { kind: "failed" });
    await first;
    void view.reveal();

    expect(state.runs).toBe(2);
  });

  it("L3: treats a run that throws as failed", async () => {
    const { state, view } = createHarness();
    const first = view.reveal();
    state.pending[0]?.reject(new Error("boom"));
    await expect(first).resolves.toBeUndefined();
    void view.reveal();

    expect(state.runs).toBe(2);
    expect(state.applied).toEqual([]);
  });

  it("L4: holds the latch after a skipped run and keeps the view", async () => {
    const { state, view, settle } = createHarness();
    const first = view.reveal();
    settle(0, { kind: "skipped" });
    await first;
    await view.reveal();

    expect(state.runs).toBe(1);
    expect(state.applied).toEqual([]);
    expect(state.resets).toBe(0);
  });

  it("L5: drops a late result after the feature is disabled", async () => {
    const { state, view, settle } = createHarness();
    const first = view.reveal();
    state.enabled = false;
    await view.invalidate();
    settle(0, loaded("A"));
    await first;

    expect(state.applied).toEqual([]);
    expect(state.resets).toBe(1);
  });

  it("L6: drops a late result after clear and loads again on reveal", async () => {
    const { state, view, settle } = createHarness();
    const first = view.reload();
    view.clear();
    settle(0, loaded("A"));
    await first;

    expect(state.applied).toEqual([]);
    expect(state.resets).toBe(1);

    void view.reveal();
    expect(state.runs).toBe(2);
  });

  it("L7: applies only the newest run when runs end out of order", async () => {
    const { state, view, settle } = createHarness();
    const first = view.reload();
    const second = view.reload();
    settle(1, loaded("B"));
    await second;
    settle(0, loaded("A"));
    await first;

    expect(state.applied).toEqual(["B"]);
  });

  it("L8: marks a changed key stale and loads on the next reveal", async () => {
    const { state, view, settle } = createHarness();
    const first = view.reload();
    settle(0, loaded("A"));
    await first;

    state.key = "b";
    state.staleReload = false;
    await view.invalidate();
    expect(state.runs).toBe(1);

    void view.reveal();
    expect(state.runs).toBe(2);
  });

  it("L9: loads at once on a changed key when reloadStaleWhen is true", async () => {
    const { state, view, settle } = createHarness();
    const first = view.reload();
    settle(0, loaded("A"));
    await first;

    state.key = "b";
    state.staleReload = true;
    void view.invalidate();

    expect(state.runs).toBe(2);
  });

  it("L10: does not repeat a failed run when the key did not change", async () => {
    const { state, view, settle } = createHarness();
    const first = view.reveal();
    settle(0, { kind: "failed" });
    await first;

    state.staleReload = true;
    await view.invalidate();

    expect(state.runs).toBe(1);
  });

  it("L11: starts no run and no reset while disabled", async () => {
    const { state, view } = createHarness();
    state.enabled = false;
    await view.reveal();
    await view.reload();

    expect(state.runs).toBe(0);
    expect(state.resets).toBe(0);
  });

  it("L12: autoLoad gates reveal but not reload", async () => {
    const { state, view } = createHarness({ autoLoad: () => false });
    await view.reveal();
    expect(state.runs).toBe(0);

    void view.reload();
    expect(state.runs).toBe(1);
  });

  it("L13: loads again after the feature is disabled and enabled", async () => {
    const { state, view, settle } = createHarness();
    const first = view.reload();
    settle(0, loaded("A"));
    await first;

    state.enabled = false;
    await view.invalidate();
    expect(state.resets).toBe(1);

    state.enabled = true;
    state.staleReload = true;
    void view.invalidate();
    expect(state.runs).toBe(2);
  });

  it("L14: drops a result whose key changed during the run", async () => {
    const { state, view, settle } = createHarness();
    const first = view.reveal();
    state.key = "b";
    settle(0, loaded("A"));
    await first;

    expect(state.applied).toEqual([]);

    void view.reveal();
    expect(state.runs).toBe(2);
  });
});
