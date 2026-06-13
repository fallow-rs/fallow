import { describe, expect, it } from "vitest";
import { createSingleFlight } from "../src/analysis-single-flight.js";

interface Deferred {
  readonly promise: Promise<boolean>;
  readonly resolve: (value: boolean) => void;
}

const deferred = (): Deferred => {
  let resolve!: (value: boolean) => void;
  const promise = new Promise<boolean>((res) => {
    resolve = res;
  });
  return { promise, resolve };
};

describe("createSingleFlight", () => {
  it("runs the task once when no run is in flight", async () => {
    let runs = 0;
    const flight = createSingleFlight(() => {
      runs += 1;
      return Promise.resolve(true);
    });

    await expect(flight.run(false)).resolves.toBe(true);
    expect(runs).toBe(1);
  });

  it("dedups concurrent background calls onto the single in-flight run", async () => {
    let runs = 0;
    const gate = deferred();
    const flight = createSingleFlight(() => {
      runs += 1;
      return gate.promise;
    });

    const first = flight.run(false);
    const second = flight.run(false);

    expect(runs).toBe(1);
    gate.resolve(true);
    await expect(first).resolves.toBe(true);
    await expect(second).resolves.toBe(true);
    expect(runs).toBe(1);
  });

  it("re-runs once after the current run when a force call arrives in flight", async () => {
    let runs = 0;
    const gates = [deferred(), deferred()];
    const flight = createSingleFlight(() => {
      const gate = gates[runs];
      runs += 1;
      return gate.promise;
    });

    const background = flight.run(false);
    const forced = flight.run(true);
    expect(runs).toBe(1);

    // The forced re-run must not start until the in-flight run settles.
    gates[0].resolve(false);
    await background;
    await Promise.resolve();
    expect(runs).toBe(2);

    gates[1].resolve(true);
    await expect(forced).resolves.toBe(true);
  });

  it("coalesces multiple force calls in flight into a single re-run", async () => {
    let runs = 0;
    const gates = [deferred(), deferred()];
    const flight = createSingleFlight(() => {
      const gate = gates[runs];
      runs += 1;
      return gate.promise;
    });

    flight.run(false);
    const forcedA = flight.run(true);
    const forcedB = flight.run(true);
    expect(forcedA).toBe(forcedB);

    gates[0].resolve(true);
    gates[1].resolve(true);
    await Promise.all([forcedA, forcedB]);
    expect(runs).toBe(2);
  });

  it("starts a fresh run for a force call that arrives after the prior re-run cleared", async () => {
    let runs = 0;
    const flight = createSingleFlight(() => {
      runs += 1;
      return Promise.resolve(true);
    });

    await flight.run(true);
    await flight.run(true);
    expect(runs).toBe(2);
  });

  it("threads the force flag into the task and forces the coalesced re-run", async () => {
    const forces: boolean[] = [];
    const gates = [deferred(), deferred()];
    const flight = createSingleFlight((force) => {
      const gate = gates[forces.length];
      forces.push(force);
      return gate.promise;
    });

    const forced = flight.run(false);
    flight.run(true);

    gates[0].resolve(true);
    await forced;
    await Promise.resolve();
    gates[1].resolve(true);
    await Promise.resolve();
    // First run reflects the caller's flag (false); the coalesced re-run is
    // forced by definition.
    expect(forces).toEqual([false, true]);
  });
});
