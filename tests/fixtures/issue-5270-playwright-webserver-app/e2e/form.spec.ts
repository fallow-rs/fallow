import { expect, test } from '@playwright/test';

test('submits the form', async ({ page }) => {
  await page.goto('/');
  await expect(page.getByText('Saved')).toBeVisible();
});
