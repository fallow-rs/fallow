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
    await win.getByTestId("file-open").first().click();
    await win.getByText(/@@|no textual diff/).waitFor({ timeout: 20_000 });
    await win.screenshot({ path: `${shots}/02-diff.png` });
  });
  await safe(async () => {
    await win.getByTestId("mode-screenshot").click({ timeout: 10_000 });
    await win.screenshot({ path: `${shots}/03-screenshot-mode.png` });
  });
  await safe(async () => {
    await win.getByTestId("mode-live").click({ timeout: 10_000 });
    await win.screenshot({ path: `${shots}/04-live.png` });
  });

  await app.close();
});
