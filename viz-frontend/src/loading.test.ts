import { afterEach, describe, expect, it, vi } from "vitest";
import { runSteps, type LoadProgress, type Loader } from "./loading";

const makeLoader = (): { loader: Loader; seen: LoadProgress[] } => {
  const seen: LoadProgress[] = [];
  return { seen, loader: { update: (progress) => seen.push(progress), done: () => undefined } };
};

function* stepsOf(fractions: number[]): Generator<LoadProgress, void> {
  for (const fraction of fractions) yield { label: `at ${fraction}`, fraction };
}

afterEach(() => {
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

describe("runSteps", () => {
  it("shows only the last step when all steps fit in one slice", async () => {
    vi.spyOn(performance, "now").mockReturnValue(0);
    const { loader, seen } = makeLoader();
    await runSteps(stepsOf([0.1, 0.5, 0.9]), loader);
    expect(seen.map((step) => step.fraction)).toEqual([0.9]);
  });

  it("gives the page a frame and shows progress when a slice runs long", async () => {
    let clock = 0;
    vi.spyOn(performance, "now").mockImplementation(() => (clock += 60));
    const frames = vi.fn((callback: FrameRequestCallback) => callback(0));
    vi.stubGlobal("requestAnimationFrame", frames);
    const { loader, seen } = makeLoader();
    await runSteps(stepsOf([0.1, 0.5, 0.9]), loader);
    expect(frames).toHaveBeenCalledTimes(3);
    expect(seen.at(-1)?.fraction).toBe(0.9);
  });

  it("does nothing for a build with no steps", async () => {
    const { loader, seen } = makeLoader();
    await runSteps(stepsOf([]), loader);
    expect(seen).toEqual([]);
  });
});
