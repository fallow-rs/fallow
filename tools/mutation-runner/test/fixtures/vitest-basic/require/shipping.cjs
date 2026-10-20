// A CommonJS module that the test loads with a native `require`, so the
// Vite module runner never sees it.
const FREE_SHIPPING_MINIMUM = 50;

const shippingFee = (subtotal) => (subtotal >= FREE_SHIPPING_MINIMUM ? 0 : 5);

module.exports = { shippingFee };
