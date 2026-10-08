/**
 * Loading screen for the map. Building the layout of a large project takes
 * seconds, so the page shows a progress bar and the current step instead of a
 * blank stage. `shell.html` holds the same markup, so the bar is on screen
 * before the script runs.
 */

/** One reported step: what the build is doing and how far along it is (0 to 1). */
export interface LoadProgress {
  label: string;
  fraction: number;
}

export interface Loader {
  /** Show the latest progress. */
  update: (progress: LoadProgress) => void;
  /** Remove the loading screen. */
  done: () => void;
}

/** Longest stretch of work between two frames. */
const SLICE_MS = 50;
/** Wait for the next frame, but not for a page that never paints (a hidden tab). */
const FRAME_TIMEOUT_MS = 100;

const percent = (fraction: number): number => Math.round(Math.min(Math.max(fraction, 0), 1) * 100);

/** Build the loading screen inside `stage`. */
export const createLoader = (stage: HTMLElement): Loader => {
  const root = document.createElement("div");
  root.className = "map-loading";
  root.setAttribute("role", "status");
  root.innerHTML =
    '<div class="map-loading-card">' +
    '<p class="map-loading-title">Building the map</p>' +
    '<div class="map-loading-track" role="progressbar" aria-valuemin="0" aria-valuemax="100" aria-valuenow="0">' +
    '<div class="map-loading-fill"></div></div>' +
    '<p class="map-loading-step">Reading the data</p></div>';
  stage.appendChild(root);

  const track = root.querySelector<HTMLElement>(".map-loading-track");
  const fill = root.querySelector<HTMLElement>(".map-loading-fill");
  const step = root.querySelector<HTMLElement>(".map-loading-step");

  return {
    update: ({ label, fraction }) => {
      const value = percent(fraction);
      if (fill) fill.style.width = `${value}%`;
      track?.setAttribute("aria-valuenow", String(value));
      if (step) step.textContent = label;
    },
    done: () => root.remove(),
  };
};

/** Resolve after the browser has had a chance to paint. */
export const nextFrame = (): Promise<void> =>
  new Promise((resolve) => {
    const timer = setTimeout(resolve, FRAME_TIMEOUT_MS);
    requestAnimationFrame(() => {
      clearTimeout(timer);
      setTimeout(resolve, 0);
    });
  });

/**
 * Run `steps` to the end. The loading screen is updated and the page gets a
 * frame whenever a slice of work has used up its time.
 */
export const runSteps = async (
  steps: Iterator<LoadProgress, void>,
  loader: Loader,
): Promise<void> => {
  let sliceStart = performance.now();
  let latest: LoadProgress | null = null;
  for (let next = steps.next(); !next.done; next = steps.next()) {
    latest = next.value;
    if (performance.now() - sliceStart < SLICE_MS) continue;
    loader.update(latest);
    await nextFrame();
    sliceStart = performance.now();
  }
  if (latest) loader.update(latest);
};
