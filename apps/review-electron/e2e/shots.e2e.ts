import { test, _electron as electron, type ElectronApplication } from "@playwright/test";
import { resolve } from "node:path";

const appDir = resolve(__dirname, "..");
const worktreeRoot = resolve(appDir, "..", "..");
const shots = process.env["FRE_SHOTS_DIR"] ?? "/tmp/fre-qa";

const safe = async (fn: () => Promise<void>): Promise<void> => {
  try {
    await fn();
  } catch {
    /* capture-only: skip a screen if it isn't reachable in this run */
  }
};

// Capture-only (no assertions): walks each screen, writes PNGs for design QA.
// Run with: npx playwright test shots.e2e.ts
test("capture screens for design QA", async () => {
  const app: ElectronApplication = await electron.launch({
    args: [resolve(appDir, "out", "main", "index.js")],
    cwd: worktreeRoot,
    env: {
      ...process.env,
      FALLOW_BIN: process.env["FALLOW_BIN"] ?? resolve(worktreeRoot, "target", "release", "fallow"),
    } as Record<string, string>,
  });
  const win = await app.firstWindow();

  await win.getByRole("button", { name: "Load review" }).click();
  await win.getByTestId("review-loaded").waitFor({ timeout: 150_000 });
  await win.screenshot({ path: `${shots}/01-walkthrough.png` });

  await safe(async () => {
    // Keyboard-focus a file row to QA the focus-visible ring. Press Tab first so
    // the browser is in keyboard modality (otherwise :focus-visible won't match).
    await win.keyboard.press("Tab");
    await win.getByTestId("file-open").first().focus();
    await win.waitForTimeout(150);
    await win.screenshot({ path: `${shots}/10-focus.png` });
  });

  await safe(async () => {
    // Simulate the in-page picker posting a selection to the localhost bridge.
    await fetch("http://127.0.0.1:7787/fallow-select", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({
        file: "apps/review-electron/src/main/index.ts",
        line: 1,
        component: "main",
      }),
    });
    await win.getByText("inspected").waitFor({ timeout: 30_000 });
    await win.screenshot({ path: `${shots}/07-inspector.png` });
  });

  await safe(async () => {
    // Capture the screenshot + annotate surface (drawing toolbar over a capture).
    await win.getByTestId("mode-shot").click({ timeout: 10_000 });
    await win.getByTestId("shot-capture").click({ timeout: 10_000 });
    await win.getByRole("button", { name: "send to agent" }).waitFor({ timeout: 20_000 });
    await win.screenshot({ path: `${shots}/08-annotate.png` });
  });

  await safe(async () => {
    // Scroll a high fan-in hub (accented metric) into view to QA the grading.
    await win
      .getByTestId("file-open")
      .filter({ hasText: "walkthrough.ts" })
      .first()
      .scrollIntoViewIfNeeded();
    await win.waitForTimeout(200);
    await win.screenshot({ path: `${shots}/06-files-scrolled.png` });
  });

  await safe(async () => {
    await win.getByTestId("file-open").first().click();
    await win.getByText(/@@|no textual diff/).waitFor({ timeout: 20_000 });
    await win.screenshot({ path: `${shots}/02-diff.png` });
  });
  await safe(async () => {
    // A heavily-rewritten file gives a mixed diff (deletions + context + adds).
    await win.getByTestId("file-open").filter({ hasText: "DiffView.tsx" }).first().click();
    await win.getByText(/@@/).first().waitFor({ timeout: 20_000 });
    await win.screenshot({ path: `${shots}/09-diff-mixed.png` });
  });
  await safe(async () => {
    await win.getByTestId("mode-shot").click({ timeout: 10_000 });
    await win.screenshot({ path: `${shots}/03-screenshot-mode.png` });
  });
  await safe(async () => {
    await win.getByTestId("mode-live").click({ timeout: 10_000 });
    // Let the live webview settle into its loaded (or overlay) state.
    await win.waitForTimeout(1500);
    await win.screenshot({ path: `${shots}/04-live.png` });
  });
  await safe(async () => {
    // Drive the live surface to its unreachable-server error state.
    await win.getByTestId("live-url").fill("http://localhost:1");
    await win.getByTestId("live-go").click({ timeout: 10_000 });
    await win
      .locator('[data-testid="live-overlay"][data-conn="failed"]')
      .waitFor({ timeout: 15_000 });
    await win.screenshot({ path: `${shots}/05-live-error.png` });
  });

  await app.close();
});
