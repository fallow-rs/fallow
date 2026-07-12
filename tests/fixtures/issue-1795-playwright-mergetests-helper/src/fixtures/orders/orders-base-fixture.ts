import { test as base } from '@playwright/test';
import { OrdersPage } from '../../pages/orders-page';

export type OrdersUi = {
  invoke: {
    orders: OrdersPage;
  };
};

export type OrdersUiFixtures = {
  ordersUi: OrdersUi;
};

export const ordersBaseFixture = base.extend<OrdersUiFixtures>({
  ordersUi: async ({}, use) => {
    await use({ invoke: { orders: new OrdersPage() } });
  },
});
