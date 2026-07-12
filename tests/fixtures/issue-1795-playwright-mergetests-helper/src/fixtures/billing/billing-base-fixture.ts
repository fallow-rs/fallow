import { test as base } from '@playwright/test';
import { BillingPage } from '../../pages/billing-page';

export type BillingFixtures = {
  billing: BillingPage;
};

export const billingBaseFixture = base.extend<BillingFixtures>({
  billing: async ({}, use) => {
    await use(new BillingPage());
  },
});
