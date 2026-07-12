import { mergeTests } from '@playwright/test';
import { promoTest } from './promo-fixture';

// `const` mergeTests whose argument is a factory CALL (`promoTest()`), consumed
// directly as `promoMerged(title, cb)`. Exercises the widened const-form
// wrapper-argument filter (fix b). Issue #1795.
export const promoMerged = mergeTests(promoTest());
