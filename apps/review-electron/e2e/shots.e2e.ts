import { test, _electron as electron, type ElectronApplication } from "@playwright/test";
import { resolve } from "node:path";

const appDir = resolve(__dirname, "..");
const worktreeRoot = resolve(appDir, "..", "..");
const fallowBin = resolve(worktreeRoot, "target", "release", "fallow");
const shots = process.env["FRE_SHOTS_DIR"] ?? "/tmp/fre-qa";

// Capture-only (no assertions): launches the app, walks each screen, writes PNGs
// for the design-QA pass. Run with: npx playwright test shots.e2e.ts
test("capture screens for design QA", async () => {
  const app: ElectronApplication = await electron.launch({
    args: [resolve(appDir, "out", "main", "index.js")],
    cwd: worktreeRoot,
    env: { ...process.env, FALLOW_BIN: fallowBin } as Record<string, string>,
  });
  const win = await app.firstWindow();

  await win.getByRole("button", { name: "Load review" }).click();
  await win.getByText(/changed files, .* risk, verdict/).waitFor({ timeout: 50_000 });
  await win.screenshot({ path: `${shots}/01-walkthrough.png` });

  await win.getByTestId("file-open").first().click();
  await win.getByText(/@@|no textual diff/).waitFor({ timeout: 20_000 });
  await win.screenshot({ path: `${shots}/02-diff.png` });

  await win.getByRole("button", { name: "screenshot url" }).click();
  await win.screenshot({ path: `${shots}/03-screenshot-mode.png` });

  await win.getByRole("button", { name: "live app" }).click();
  await win.screenshot({ path: `${shots}/04-live.png` });

  await app.close();
});
