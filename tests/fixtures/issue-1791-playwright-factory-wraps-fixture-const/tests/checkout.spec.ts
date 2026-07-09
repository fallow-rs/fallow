import { ordersTest } from '../src/fixtures/orders-fixture';

ordersTest()('places and cancels an order', async ({ orders }) => {
  await orders.placeOrder('SYN-001');
  await orders.cancelOrder('SYN-001');
});
