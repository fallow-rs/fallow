import { test as base } from '@playwright/test';
import { OrdersPage } from '../pages/orders-page';

type OrdersFixtures = {
  orders: OrdersPage;
};

const ordersBaseFixture = base.extend<OrdersFixtures>({
  orders: async ({}, use) => {
    await use(new OrdersPage());
  },
});

// Exported as a function that wraps the local fixture const via `.extend({})`
// (no type argument), called in specs as `ordersTest()(...)`. Issue #1791.
export function ordersTest() {
  return ordersBaseFixture.extend({});
}
