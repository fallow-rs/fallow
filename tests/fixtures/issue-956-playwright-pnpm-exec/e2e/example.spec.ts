import { expect, test } from "@playwright/test";

test("home page loads", async ({ page }) => {
  await page.goto("http://localhost:3000");
  await expect(page).toHaveTitle(/Example/);
});
