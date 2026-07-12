import { promoMerged } from '../src/fixtures/promo/promo-merged-fixture';

promoMerged('applies a promo code', async ({ promo }) => {
  await promo.applyPromo('SYN-PROMO');
});
