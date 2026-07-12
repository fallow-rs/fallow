import { test as base } from '@playwright/test';
import { ShippingPage } from '../../pages/shipping-page';

export type ShippingFixtures = {
  shipping: ShippingPage;
};

export const shippingBaseFixture = base.extend<ShippingFixtures>({
  shipping: async ({}, use) => {
    await use(new ShippingPage());
  },
});
