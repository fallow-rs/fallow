import { appTestDirectReturn } from '../playwright/fixture-direct';

appTestDirectReturn()('uses nested fixture member through direct return helper', async ({ appUi }) => {
  await appUi.step.sidebar.openPatientsDirect();
});
