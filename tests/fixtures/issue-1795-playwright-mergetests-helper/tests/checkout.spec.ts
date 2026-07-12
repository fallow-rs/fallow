import { checkoutTest } from '../src/fixtures/checkout/checkout-fixture';

checkoutTest()(
  'places and cancels an order, opens an invoice',
  async ({ ordersUi, billing }) => {
    await ordersUi.invoke.orders.placeOrder('SYN-001');
    await ordersUi.invoke.orders.cancelOrder('SYN-001');
    await billing.openInvoice('SYN-INV-001');
  },
);
