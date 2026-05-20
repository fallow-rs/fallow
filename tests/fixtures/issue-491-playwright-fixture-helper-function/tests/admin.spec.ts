import { appTest } from '../playwright/fixtures';

appTest()('uses admin through nested helper fixture', async ({ appUi: { step: { admin } } }) => {
  await admin.openAdmin();
});
