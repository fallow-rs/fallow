import {
  test,
  expect,
  _electron as electron,
  type ElectronApplication,
  type Page,
} from "@playwright/test";
import { execFileSync } from "node:child_process";
import { appendFileSync, cpSync, existsSync, mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { resolve } from "node:path";
import { ensureReviewStarted } from "./review";

const appDir = resolve(__dirname, "..");
const worktreeRoot = resolve(appDir, "..", "..");

let app: ElectronApplication | undefined;
let temporaryReviewRoot: string | undefined;

test.afterEach(async () => {
  try {
    await app?.close();
    app = undefined;
  } finally {
    if (temporaryReviewRoot !== undefined) {
      rmSync(temporaryReviewRoot, { recursive: true, force: true });
      temporaryReviewRoot = undefined;
    }
  }
});

const resolveFallowBin = (): string => {
  const explicit = process.env["FALLOW_BIN"]?.trim();
  if (explicit) {
    return explicit;
  }

  for (const profile of ["release", "debug"] as const) {
    const candidate = resolve(worktreeRoot, "target", profile, "fallow");
    if (existsSync(candidate)) {
      return candidate;
    }
  }

  return "fallow";
};

const createReviewRoot = (dirty = false): string => {
  const temporaryRoot = mkdtempSync(resolve(tmpdir(), "fallow-review-e2e-"));
  temporaryReviewRoot = temporaryRoot;
  const reviewRoot = resolve(temporaryRoot, "project");
  cpSync(resolve(appDir, "fixtures", "sample-app"), reviewRoot, { recursive: true });

  const git = (args: string[]): void => {
    execFileSync("git", args, { cwd: reviewRoot, stdio: "ignore" });
  };
  git(["init", "-b", "main"]);
  git(["config", "user.email", "test@example.com"]);
  git(["config", "user.name", "Fallow E2E"]);
  git(["add", "."]);
  git(["commit", "-m", "baseline"]);

  if (dirty) {
    appendFileSync(resolve(reviewRoot, "src", "App.tsx"), "\nexport const e2eDiffMarker = true;\n");
  }
  return reviewRoot;
};

const launch = async (reviewRoot?: string): Promise<ElectronApplication> =>
  electron.launch({
    args: [resolve(appDir, "out", "main", "index.js")],
    cwd: worktreeRoot,
    env: {
      ...process.env,
      FALLOW_BIN: resolveFallowBin(),
      ...(reviewRoot === undefined ? {} : { FALLOW_REVIEW_ROOT: reviewRoot }),
    } as Record<string, string>,
  });

const launchLoadedReview = async (reviewRoot = createReviewRoot()): Promise<Page> => {
  app = await launch(reviewRoot);
  const win = await app.firstWindow();
  await ensureReviewStarted(win);
  await expect(win.getByTestId("review-loaded")).toBeVisible({ timeout: 150_000 });
  return win;
};

test("boots and renders the review shell", async () => {
  app = await launch(createReviewRoot());
  const win = await app.firstWindow();
  await expect(win.getByRole("heading", { name: "Fallow Review" })).toBeVisible();
  await ensureReviewStarted(win);
  await expect(win.getByTestId("mode-live")).toBeVisible();
});

test("loads a grounded walkthrough from the real engine", async () => {
  // `fallow review` runs on a real fixture project; wait for the focus headline to render.
  await launchLoadedReview();
});

test("opens a file diff from the walkthrough", async () => {
  const reviewRoot = createReviewRoot(true);
  const win = await launchLoadedReview(reviewRoot);
  await win.getByRole("button", { name: "open App.tsx" }).click();
  await expect(win.getByText(/@@/).first()).toBeVisible({ timeout: 20_000 });
});

test("inspector bridge pushes a grounded card to the UI", async () => {
  const win = await launchLoadedReview();

  // Simulate the in-page picker posting a selection to the localhost bridge.
  const res = await fetch("http://127.0.0.1:7787/fallow-select", {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({
      file: "src/App.tsx",
      line: 1,
      component: "App",
    }),
  });
  expect(res.ok).toBe(true);

  const inspector = win.getByTestId("inspector-card");
  await expect(inspector).toBeVisible({ timeout: 30_000 });
  await expect(inspector).toContainText("src/App.tsx:1");
});
