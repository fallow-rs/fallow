import { mergeTests } from '@playwright/test';
import { billingTest } from '../billing/billing-fixture';
import { ordersUiTest } from '../orders/orders-fixture';

// Function returning a locally-bound `mergeTests(...)` of two function-wrapped
// fixtures. Exercises the mergeTests-in-helper arm (fix a), the return-ident
// follow to a `const` declarator (fix c), and the imported `.extend` base alias
// fact (fix a2). Issue #1795.
export function checkoutTest() {
  const merge = mergeTests(billingTest(), ordersUiTest());
  return merge;
}
