import { shippingTest } from '../src/fixtures/shipping/shipping-fixture';

shippingTest()('tracks a shipment', async ({ shipping }) => {
  await shipping.trackShipment('SYN-SHIP-001');
});
