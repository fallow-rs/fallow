import { test, _electron as electron, type ElectronApplication } from "@playwright/test";
import { chmodSync, writeFileSync } from "node:fs";
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
  await safe(async () => {
    // The real review takes a beat; capture the loading state before it resolves.
    await win.getByText(/running fallow review/).waitFor({ timeout: 3000 });
    await win.screenshot({ path: `${shots}/11-loading.png` });
  });
  await win.getByTestId("review-loaded").waitFor({ timeout: 150_000 });
  await win.screenshot({ path: `${shots}/01-walkthrough.png` });

  await safe(async () => {
    // The default diff shows all files; scroll to expose a file-to-file boundary.
    await win.getByTestId("diff-scroll").evaluate((el) => {
      el.scrollTop = 2300;
    });
    await win.waitForTimeout(150);
    await win.screenshot({ path: `${shots}/16-diff-all.png` });
  });

  await safe(async () => {
    // Keyboard-focus a file row to QA the focus-visible ring. Press Tab first so
    // the browser is in keyboard modality (otherwise :focus-visible won't match).
    await win.keyboard.press("Tab");
    await win.getByTestId("file-open").first().focus();
    await win.waitForTimeout(150);
    await win.screenshot({ path: `${shots}/10-focus.png` });
  });

  await safe(async () => {
    // Expand the cleared panel to QA the aligned count list.
    await win.getByTestId("cleared-toggle").click({ timeout: 10_000 });
    await win.waitForTimeout(150);
    await win.screenshot({ path: `${shots}/12-cleared.png` });
    await win.getByTestId("cleared-toggle").click({ timeout: 10_000 });
  });

  await safe(async () => {
    // Collapse the second stage group to QA the collapsed state.
    await win.getByTestId("stage-toggle").nth(1).click({ timeout: 10_000 });
    await win.waitForTimeout(150);
    await win.screenshot({ path: `${shots}/15-collapsed.png` });
    await win.getByTestId("stage-toggle").nth(1).click({ timeout: 10_000 });
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
    await win.getByTestId("inspector-card").waitFor({ timeout: 30_000 });
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
    // Drive a capture against an unreachable URL to QA the cleaned error message.
    await win.getByTestId("shot-url").fill("http://localhost:1");
    await win.getByTestId("shot-capture").click({ timeout: 10_000 });
    await win
      .locator('[data-testid="shot-overlay"][data-phase="error"]')
      .waitFor({ timeout: 20_000 });
    await win.screenshot({ path: `${shots}/14-shot-error.png` });
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

// Separate launch with a bad engine path so `fallow review` fails: QA the error.
test("capture the review error state", async () => {
  const app: ElectronApplication = await electron.launch({
    args: [resolve(appDir, "out", "main", "index.js")],
    cwd: worktreeRoot,
    env: { ...process.env, FALLOW_BIN: "/nonexistent/fallow-bin" } as Record<string, string>,
  });
  const win = await app.firstWindow();
  await safe(async () => {
    await win.getByRole("button", { name: "Load review" }).click();
    await win.getByTestId("review-error").waitFor({ timeout: 30_000 });
    await win.screenshot({ path: `${shots}/13-review-error.png` });
  });
  await app.close();
});

// Fixture-backed: point FALLOW_BIN at a stub that emits the with-decisions
// brief, so the (otherwise all-additions) review renders the decision surface.
test("capture the decision surface", async () => {
  const fixture = resolve(appDir, "fixtures", "sample-review-with-decisions.json");
  const stub = "/tmp/fre-fallow-stub.sh";
  writeFileSync(stub, `#!/bin/sh\ncat ${JSON.stringify(fixture)}\n`);
  chmodSync(stub, 0o755);
  const app: ElectronApplication = await electron.launch({
    args: [resolve(appDir, "out", "main", "index.js")],
    cwd: worktreeRoot,
    env: { ...process.env, FALLOW_BIN: stub } as Record<string, string>,
  });
  const win = await app.firstWindow();
  await safe(async () => {
    await win.getByRole("button", { name: "Load review" }).click();
    await win.getByTestId("review-loaded").waitFor({ timeout: 60_000 });
    await win.screenshot({ path: `${shots}/17-decisions.png` });
  });
  await app.close();
});
