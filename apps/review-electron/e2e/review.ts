import type { Locator, Page } from "@playwright/test";

const REVIEW_START_TIMEOUT = 30_000;
const REVIEW_TRANSITION_TIMEOUT = 5_000;

type ReviewStartupState = "idle" | "running" | "loaded" | "error";

interface ReviewStartupOptions {
  allowError?: boolean;
}

const waitForVisible = async (
  locator: Locator,
  state: ReviewStartupState,
  timeout: number,
): Promise<ReviewStartupState> => {
  await locator.waitFor({ state: "visible", timeout });
  return state;
};

const waitForReviewState = async (
  page: Page,
  includeIdle: boolean,
  timeout: number,
): Promise<ReviewStartupState> => {
  const states: Array<Promise<ReviewStartupState>> = [
    waitForVisible(page.getByText(/running fallow review/), "running", timeout),
    waitForVisible(page.getByTestId("review-loaded"), "loaded", timeout),
    waitForVisible(page.getByTestId("review-error"), "error", timeout),
  ];

  if (includeIdle) {
    states.push(waitForVisible(page.getByRole("button", { name: "Load review" }), "idle", timeout));
  }

  return Promise.race(states);
};

const assertAcceptedState = (state: ReviewStartupState, allowError: boolean): void => {
  if (state === "error" && !allowError) {
    throw new Error("Review failed during startup");
  }
};

/** Starts a review only when startup has not already begun. */
export const ensureReviewStarted = async (
  page: Page,
  options: ReviewStartupOptions = {},
): Promise<void> => {
  const allowError = options.allowError ?? false;
  const state = await waitForReviewState(page, true, REVIEW_START_TIMEOUT);
  if (state !== "idle") {
    assertAcceptedState(state, allowError);
    return;
  }

  const loadReview = page.getByRole("button", { name: "Load review" });
  try {
    await loadReview.click({ timeout: REVIEW_TRANSITION_TIMEOUT });
  } catch (error: unknown) {
    let transitionedState: ReviewStartupState;
    try {
      transitionedState = await waitForReviewState(page, false, REVIEW_TRANSITION_TIMEOUT);
    } catch {
      throw error;
    }
    assertAcceptedState(transitionedState, allowError);
  }
};
