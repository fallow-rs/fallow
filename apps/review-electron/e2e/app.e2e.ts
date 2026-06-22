import { test, expect, _electron as electron, type ElectronApplication } from "@playwright/test";
import { resolve } from "node:path";

const appDir = resolve(__dirname, "..");
const worktreeRoot = resolve(appDir, "..", "..");

let app: ElectronApplication | undefined;

test.afterEach(async () => {
  await app?.close();
  app = undefined;
});

const launch = async (): Promise<ElectronApplication> =>
  electron.launch({
    args: [resolve(appDir, "out", "main", "index.js")],
    cwd: worktreeRoot,
    env: {
      ...process.env,
      FALLOW_BIN: process.env["FALLOW_BIN"] ?? resolve(worktreeRoot, "target", "release", "fallow"),
    } as Record<string, string>,
  });

test("boots and renders the review shell", async () => {
  app = await launch();
  const win = await app.firstWindow();
  await expect(win.getByRole("heading", { name: "Fallow Review" })).toBeVisible();
  await expect(win.getByRole("button", { name: "Load review" })).toBeVisible();
  await expect(win.getByTestId("mode-live")).toBeVisible();
});

test("loads a grounded walkthrough from the real engine", async () => {
  app = await launch();
  const win = await app.firstWindow();
  await win.getByRole("button", { name: "Load review" }).click();
  // `fallow review` runs on the worktree; wait for the focus headline to render.
  await expect(win.getByTestId("review-loaded")).toBeVisible({ timeout: 150_000 });
});

test("opens a file diff from the walkthrough", async () => {
  app = await launch();
  const win = await app.firstWindow();
  await win.getByRole("button", { name: "Load review" }).click();
  await expect(win.getByTestId("review-loaded")).toBeVisible({ timeout: 150_000 });
  await win.getByTestId("file-open").first().click();
  await expect(win.getByText(/@@|no textual diff/)).toBeVisible({ timeout: 20_000 });
});

test("inspector bridge pushes a grounded card to the UI", async () => {
  app = await launch();
  const win = await app.firstWindow();
  await win.getByRole("button", { name: "Load review" }).click();
  await expect(win.getByTestId("review-loaded")).toBeVisible({ timeout: 150_000 });

  // Simulate the in-page picker posting a selection to the localhost bridge.
  const res = await fetch("http://127.0.0.1:7787/fallow-select", {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({
      file: "apps/review-electron/src/main/index.ts",
      line: 1,
      component: "main",
    }),
  });
  expect(res.ok).toBe(true);

  const inspector = win.getByTestId("inspector-card");
  await expect(inspector).toBeVisible({ timeout: 30_000 });
  await expect(inspector).toContainText("src/main/index.ts:1");
});
