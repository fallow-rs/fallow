import { expect, test } from "playwright/test";

test("shows the title", async ({ page }) => {
  await expect(page).toHaveTitle("Home");
});
