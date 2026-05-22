import { appTestWithLocal } from '../playwright/fixture-with-local';

appTestWithLocal()('uses nested fixture member through helper with local setup', async ({ appUi }) => {
  await appUi.step.sidebar.openPatientsWithLocal();
});
