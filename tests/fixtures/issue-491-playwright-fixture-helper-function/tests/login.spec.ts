import { appTest } from '../playwright/fixtures';

appTest()('uses login through nested helper fixture', async ({ appUi }) => {
  await appUi.step.login.openLogin();
});
