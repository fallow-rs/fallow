import { test as base } from '@playwright/test';
import { PromoPage } from '../../pages/promo-page';

export type PromoFixtures = {
  promo: PromoPage;
};

// Helper returning a typed `base.extend<T>(...)` directly (issue #491 shape),
// used as the call-expression argument of a `const` mergeTests below.
export function promoTest() {
  return base.extend<PromoFixtures>({
    promo: async ({}, use) => {
      await use(new PromoPage());
    },
  });
}
