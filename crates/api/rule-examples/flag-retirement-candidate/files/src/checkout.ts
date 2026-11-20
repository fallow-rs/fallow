const newCheckout = (): string => "new checkout";
const oldCheckout = (): string => "old checkout";

export const checkout = (): string =>
  process.env.FEATURE_NEW_CHECKOUT === "true" ? newCheckout() : oldCheckout();
