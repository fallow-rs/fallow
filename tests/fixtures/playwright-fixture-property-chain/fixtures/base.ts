import { test as base } from '@playwright/test';
import { AppPage } from '../pages/AppPage';

type Fixtures = { app: AppPage };

export const test = base.extend<Fixtures>({
  app: async ({}, use) => {
    await use(new AppPage());
  },
});

export { expect } from '@playwright/test';
