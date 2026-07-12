import { ordersBaseFixture } from './orders-base-fixture';

// Function wrapping an IMPORTED base const via `.extend({})` (no type argument).
export function ordersUiTest() {
  return ordersBaseFixture.extend({});
}
